//! Rust/axum backend for the homelab Neo4j graph viewer.
//!
//! Serves the built Vue SPA and a single `GET /api/graph` endpoint (the whole graph as
//! `{nodes, links}`), cached in memory with an hourly TTL and a `?refresh=1` override. The
//! Neo4j credentials live only here — the browser never sees a Bolt endpoint or a password.

mod graph;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use neo4rs::Graph;
use tokio::sync::RwLock;

use nuvek_web::{config, health, serve, telemetry};

use crate::graph::GraphData;

/// Shared server state: the Neo4j handle plus an in-memory graph cache.
/// The cached graph is `Arc`-wrapped so a cache hit is a refcount bump, not a full deep clone
/// of the (427-node × per-node props + 2265-link) payload on every request.
struct AppState {
    graph: Graph,
    cache: RwLock<Option<(Instant, Arc<GraphData>)>>,
    ttl: Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init();

    // NEO4J_HOST and NEO4J_PASSWORD are runtime config (Dockerman env / docker secret /
    // ~/.env) — NEVER baked into the image or binary, so no private endpoint or
    // credential ships in the published (public) image.
    let uri = config::required_env("NEO4J_HOST")
        .context("NEO4J_HOST not set — inject it at runtime; never bake a private endpoint in")?;
    let user = config::env_or("NEO4J_USER", "neo4j");
    let pass = config::required_env("NEO4J_PASSWORD")
        .context("NEO4J_PASSWORD not set (inject at runtime; source ~/.env locally)")?;
    // Local dev binds 127.0.0.1:8901 (view.sh); the container overrides BIND=0.0.0.0 PORT=8080.
    let bind = config::env_or("BIND", "127.0.0.1");
    let port = config::env_or("PORT", "8901");

    let graph = Graph::new(&uri, &user, &pass)
        .await
        .context("neo4rs failed to connect / authenticate")?;

    let state = Arc::new(AppState {
        graph,
        cache: RwLock::new(None),
        ttl: Duration::from_secs(3600),
    });

    let api = Router::new()
        .route("/api/graph", get(handler_graph))
        .merge(health::routes())
        .layer(Extension(state));

    let app = serve::attach_spa(
        api,
        serve::SpaAssets::detect(&["/app/dist", "frontend/dist"]),
    );

    let addr = format!("{bind}:{port}");
    tracing::info!("neo4j-graph-viz listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}

/// `GET /api/graph[?refresh=1]` — the whole graph as `{nodes, links}`, cache-first.
async fn handler_graph(
    Extension(state): Extension<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let force = matches!(
        params.get("refresh").map(String::as_str),
        Some("1" | "true")
    );

    if !force {
        let guard = state.cache.read().await;
        if let Some((at, data)) = guard.as_ref() {
            if at.elapsed() < state.ttl {
                return Json(Arc::clone(data)).into_response();
            }
        }
    }

    match graph::fetch(&state.graph).await {
        Ok(data) => {
            let data = Arc::new(data);
            *state.cache.write().await = Some((Instant::now(), Arc::clone(&data)));
            Json(data).into_response()
        }
        Err(e) => {
            tracing::error!(error = ?e, "graph fetch failed");
            // Stale-if-error: serve the last good snapshot rather than a hard failure.
            if let Some((_, data)) = state.cache.read().await.as_ref() {
                return Json(Arc::clone(data)).into_response();
            }
            (StatusCode::BAD_GATEWAY, "graph fetch failed").into_response()
        }
    }
}
