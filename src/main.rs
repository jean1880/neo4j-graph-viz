//! Rust/axum backend for the Neo4j graph viewer.
//!
//! Serves the built Vue SPA plus three endpoints:
//!
//! - `GET /api/graph` — the whole graph as `{nodes, links}`, **laid out**, without properties
//! - `GET /api/node/{id}` — one node *with* its properties
//! - `GET /api/search` — the subgraph related to a query
//!
//! The Neo4j credentials live only here — the browser never sees a Bolt endpoint or a password.
//!
//! Module ownership: [`config`] reads the environment, [`state`] owns the cached snapshot and
//! everything expensive that produces it, [`api`] is the HTTP surface, and [`graph`],
//! [`layout`], [`search`] and [`embed`] each own one problem.
//!
//! NOTE: these endpoints are unauthenticated and return every node and property the fetch
//! options admit. Put an authenticating proxy in front of them on any network where the graph
//! contents are not public, and use `GRAPH_SKIP_PROPS` to withhold sensitive properties.

mod api;
mod config;
mod embed;
mod graph;
mod layout;
mod search;
mod state;
mod util;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::{routing::get, Router};
use tokio::sync::RwLock;
use tower_http::compression::CompressionLayer;

use nuvek_web::{config as web_config, health, serve, telemetry};

use crate::embed::EmbedOptions;
use crate::graph::FetchOptions;
use crate::search::SearchOptions;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init();

    // How long a fetched graph is reused. 0 disables caching (every request refetches).
    let ttl_secs: u64 = web_config::env_or("GRAPH_CACHE_TTL_SECS", "3600")
        .parse()
        .context("GRAPH_CACHE_TTL_SECS must be a non-negative integer")?;
    // Local dev binds 127.0.0.1:8901 (view.sh); the container overrides BIND=0.0.0.0 PORT=8080.
    let bind = web_config::env_or("BIND", "127.0.0.1");
    let port = web_config::env_or("PORT", "8901");

    let opts = FetchOptions::from_env()?;
    let layout = config::layout_options_from_env()?;
    let source = config::build_source().await?;

    let embed_opts = EmbedOptions::from_env()?;
    if embed_opts.enabled() {
        tracing::info!(
            model = %embed_opts.model,
            max_nodes = embed_opts.max_nodes,
            "semantic search enabled (SEARCH_EMBED_URL is set)"
        );
    }

    let state = Arc::new(AppState {
        source,
        opts,
        layout,
        search: SearchOptions::default(),
        embed: embed_opts,
        cache: RwLock::new(HashMap::new()),
        ttl: Duration::from_secs(ttl_secs),
        refreshing: Mutex::new(HashSet::new()),
        embeddings: RwLock::new(None),
    });

    let api = Router::new()
        .route("/api/graph", get(api::handler_graph))
        .route("/api/node/{id}", get(api::handler_node))
        .route("/api/search", get(api::handler_search))
        .merge(health::routes())
        .layer(axum::Extension(state));

    let mut app = serve::attach_spa(
        api,
        serve::SpaAssets::detect(&["/app/dist", "frontend/dist"]),
    );

    // Compression is a clear win behind the NAS Nginx vhost and a pessimization over loopback,
    // where it burns CPU on both ends to shorten a transfer that is already effectively free.
    // Default on so the deployed path is unchanged; `view.sh` turns it off for local dev.
    // Accepts the same spellings as `?refresh=` — an operator who writes `GRAPH_COMPRESSION=false`
    // means it, and silently leaving compression on would be a confusing way to disagree.
    let compression = util::truthy(&web_config::env_or("GRAPH_COMPRESSION", "1"));
    if compression {
        // Applied outside the SPA layer so the static bundle is compressed too. Negotiated via
        // Accept-Encoding, so a client that cannot decompress still gets plain JSON.
        app = app.layer(CompressionLayer::new());
    } else {
        tracing::info!("response compression disabled (GRAPH_COMPRESSION=0)");
    }

    let addr = format!("{bind}:{port}");
    tracing::info!("neo4j-graph-viz listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}
