//! HTTP handlers. Thin by design: each one resolves the shared snapshot, hands the work to the
//! module that owns it, and serializes the result.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::embed;
use crate::graph::GraphData;
use crate::layout::{self, LayoutOptions};
use crate::search;
use crate::state::{cached_graph, AppState};

/// Requested layout dimensionality. Anything but an explicit `3` means 2D, so a malformed
/// parameter degrades to the cheaper layout rather than to a surprise.
fn dimensions_param(params: &HashMap<String, String>) -> u8 {
    if params.get("dims").map(String::as_str) == Some("3") {
        3
    } else {
        2
    }
}

/// `GET /api/graph[?refresh=1][&dims=3]` — the whole graph as `{nodes, links}`, cache-first.
/// Node properties are **not** included; fetch them per node from `/api/node/{id}`.
pub async fn handler_graph(
    Extension(state): Extension<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let force = matches!(
        params.get("refresh").map(String::as_str),
        Some("1" | "true")
    );
    let dims = dimensions_param(&params);

    match cached_graph(&state, force, dims).await {
        Some(data) => Json(data).into_response(),
        None => (StatusCode::BAD_GATEWAY, "graph fetch failed").into_response(),
    }
}

/// Iterations used when re-laying-out a search result. Lower than a full build: the surviving
/// subgraph is small, and a search must feel immediate.
const SEARCH_LAYOUT_ITERATIONS: usize = 140;

/// `GET /api/search?q=…&breadth=…` — the subgraph related to a query.
///
/// Returns the nodes that survive, **with fresh coordinates**: the survivors are laid out again
/// among themselves so the result reads as a compact graph, rather than as a scatter of points
/// left stranded in the holes where everything else used to be.
pub async fn handler_search(
    Extension(state): Extension<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let q = params.get("q").cloned().unwrap_or_default();
    let breadth: f32 = params
        .get("breadth")
        .and_then(|b| b.parse().ok())
        .unwrap_or(0.6);

    let Some(data) = cached_graph(&state, false, dimensions_param(&params)).await else {
        return (StatusCode::BAD_GATEWAY, "graph fetch failed").into_response();
    };

    // Semantic tier, when configured and ready. A failure here is not a failed search — it
    // degrades to fuzzy matching, which is still a perfectly good answer.
    let corpus = state.embeddings.read().await.clone();
    let query_vec = match (state.embed.enabled(), corpus.as_ref()) {
        (true, Some(_)) if !q.trim().is_empty() => {
            match embed::embed_query(&state.embed, &q).await {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!(error = ?e, "query embedding failed; falling back to lexical");
                    None
                }
            }
        }
        _ => None,
    };

    let search_opts = state.search;
    let layout_opts = LayoutOptions {
        iterations: SEARCH_LAYOUT_ITERATIONS,
        ..state.layout
    };
    let data_for_task = Arc::clone(&data);

    // Scoring plus a layout pass is CPU-bound; keep it off the async workers.
    let result = tokio::task::spawn_blocking(move || {
        let mut res = search::search(
            &data_for_task,
            &q,
            breadth,
            &search_opts,
            query_vec.as_deref(),
            corpus.as_deref().map(|v| v.as_slice()),
        );
        relayout_result(&data_for_task, &mut res, &layout_opts);
        res
    })
    .await;

    match result {
        Ok(res) => Json(res).into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "search task panicked");
            (StatusCode::INTERNAL_SERVER_ERROR, "search failed").into_response()
        }
    }
}

/// Lay the surviving subgraph out among itself, and write the new coordinates back.
fn relayout_result(
    data: &GraphData,
    res: &mut search::SearchResponse,
    layout_opts: &LayoutOptions,
) {
    if res.visible.len() < 2 {
        return;
    }
    // Map graph indices onto a dense 0..k range covering only the survivors.
    let mut local = HashMap::with_capacity(res.visible.len());
    for (slot, v) in res.visible.iter().enumerate() {
        if let Some(i) = data.index_of(&v.id) {
            local.insert(i as u32, slot as u32);
        }
    }
    let edges: Vec<(u32, u32)> = data
        .edge_pairs()
        .into_iter()
        .filter_map(|(s, t)| Some((*local.get(&s)?, *local.get(&t)?)))
        .collect();

    let pos = layout::compute(res.visible.len(), &edges, layout_opts);
    for (v, p) in res.visible.iter_mut().zip(pos) {
        v.x = p[0];
        v.y = p[1];
    }
}

/// `GET /api/node/{id}` — one node with its properties, served from the same cached snapshot.
pub async fn handler_node(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(data) = cached_graph(&state, false, 2u8).await else {
        return (StatusCode::BAD_GATEWAY, "graph fetch failed").into_response();
    };
    match data.detail(&id) {
        Some(detail) => Json(detail).into_response(),
        None => (StatusCode::NOT_FOUND, "no such node").into_response(),
    }
}
