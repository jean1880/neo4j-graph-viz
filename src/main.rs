//! Rust/axum backend for the Neo4j graph viewer.
//!
//! Serves the built Vue SPA and a single `GET /api/graph` endpoint (the graph as
//! `{nodes, links}`), cached in memory with an hourly TTL and a `?refresh=1` override. The
//! Neo4j credentials live only here — the browser never sees a Bolt endpoint or a password.
//!
//! NOTE: `/api/graph` is unauthenticated and returns every node and property the fetch
//! options admit. Put an authenticating proxy in front of it on any network where the graph
//! contents are not public, and use `GRAPH_SKIP_PROPS` to withhold sensitive properties.

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
use neo4rs::{ConfigBuilder, Graph};
use tokio::sync::RwLock;
use tower_http::compression::CompressionLayer;

use nuvek_web::{config, health, serve, telemetry};

use crate::graph::{FetchOptions, GraphData};

/// Shared server state: the Neo4j handle plus an in-memory graph cache.
/// The cached graph is `Arc`-wrapped so a cache hit is a refcount bump, not a full deep clone
/// of the whole node/link payload on every request.
struct AppState {
    graph: Graph,
    opts: FetchOptions,
    cache: RwLock<Option<(Instant, Arc<GraphData>)>>,
    ttl: Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init();

    // Connection settings are runtime config only — NEVER baked into the image or binary, so
    // no endpoint or credential ships in the published image. NEO4J_HOST takes a full URI, so
    // `neo4j+s://…` (TLS / Aura) works without a code change.
    let uri = config::required_env("NEO4J_HOST")
        .context("NEO4J_HOST not set — inject it at runtime; never bake an endpoint in")?;
    let user = config::env_or("NEO4J_USER", "neo4j");
    let pass = config::required_env("NEO4J_PASSWORD")
        .context("NEO4J_PASSWORD not set — inject it at runtime (see .env.example)")?;
    let database = config::env_or("NEO4J_DATABASE", "neo4j");
    // How long a fetched graph is reused. 0 disables caching (every request refetches).
    let ttl_secs: u64 = config::env_or("GRAPH_CACHE_TTL_SECS", "3600")
        .parse()
        .context("GRAPH_CACHE_TTL_SECS must be a non-negative integer")?;
    // Local dev binds 127.0.0.1:8901 (view.sh); the container overrides BIND=0.0.0.0 PORT=8080.
    let bind = config::env_or("BIND", "127.0.0.1");
    let port = config::env_or("PORT", "8901");

    let opts = FetchOptions::from_env()?;

    let neo_config = ConfigBuilder::new()
        .uri(&uri)
        .user(&user)
        .password(&pass)
        .db(database.as_str())
        .build()
        .context("invalid Neo4j connection config")?;
    let graph = Graph::connect(neo_config)
        .await
        .context("neo4rs failed to connect / authenticate")?;

    let state = Arc::new(AppState {
        graph,
        opts,
        cache: RwLock::new(None),
        ttl: Duration::from_secs(ttl_secs),
    });

    let api = Router::new()
        .route("/api/graph", get(handler_graph))
        .merge(health::routes())
        .layer(Extension(state));

    let app = serve::attach_spa(
        api,
        serve::SpaAssets::detect(&["/app/dist", "frontend/dist"]),
    )
    // Applied outside the SPA layer so the static bundle is compressed too. Negotiated via
    // Accept-Encoding, so a client that cannot decompress still gets plain JSON.
    .layer(CompressionLayer::new());

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

    match graph::fetch(&state.graph, &state.opts).await {
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
