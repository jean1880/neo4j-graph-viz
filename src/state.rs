//! Shared server state, and the cache it guards.
//!
//! One graph snapshot backs every endpoint, so they can never disagree about which version of
//! the world they are describing. Everything expensive — the Bolt fetch, the layout, the
//! optional embedding pass — is orchestrated from here and kept off the request path.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use neo4rs::Graph;
use tokio::sync::RwLock;

use crate::embed::{self, EmbedOptions};
use crate::graph::{self, FetchOptions, GraphData};
use crate::layout::LayoutOptions;
use crate::search::SearchOptions;

/// Where the graph comes from. The fixture arm exists so benchmarks and CI can drive the whole
/// pipeline at any scale with **no database and no credentials** — a benchmark that depends on
/// the live graph's current contents cannot be compared against itself a week later.
pub enum Source {
    Neo4j(Graph),
    Fixture {
        nodes: usize,
        edges_per_node: usize,
        seed: u64,
    },
}

impl Source {
    async fn fetch(&self, opts: &FetchOptions) -> Result<GraphData> {
        match self {
            Source::Neo4j(g) => graph::fetch(g, opts).await,
            Source::Fixture {
                nodes,
                edges_per_node,
                seed,
            } => Ok(graph::fixture(*nodes, *edges_per_node, *seed, opts)),
        }
    }
}

/// Fetch a graph and lay it out.
///
/// The layout is CPU-bound for seconds at 25k nodes, so it runs on the blocking pool — leaving
/// it on an async worker would stall every other request on this runtime for its duration.
async fn build_graph(
    source: &Source,
    fetch_opts: &FetchOptions,
    layout_opts: LayoutOptions,
    dimensions: u8,
) -> Result<GraphData> {
    let mut data = source.fetch(fetch_opts).await?;
    let data = tokio::task::spawn_blocking(move || {
        data.apply_layout(&layout_opts, dimensions);
        data
    })
    .await
    .context("layout task panicked")?;
    Ok(data)
}

/// Per-node embedding vectors for the whole cached graph. `None` in a slot means that node was
/// not embedded (below the degree cut, or its batch failed) — search reads that as "no semantic
/// opinion" rather than as a zero score.
type Corpus = Arc<Vec<Option<Vec<f32>>>>;

/// Shared server state: the graph source plus an in-memory cache.
/// The cached graph is `Arc`-wrapped so a cache hit is a refcount bump, not a full deep clone
/// of the whole node/link payload on every request.
pub struct AppState {
    pub source: Source,
    pub opts: FetchOptions,
    pub layout: LayoutOptions,
    pub search: SearchOptions,
    pub embed: EmbedOptions,
    /// One snapshot per dimensionality (2D / 3D). They are different layouts of the same graph,
    /// so the 3D one is only ever computed if a client actually asks for it — a deployment that
    /// never leaves 2D never pays for it.
    pub cache: RwLock<HashMap<u8, (Instant, Arc<GraphData>)>>,
    pub ttl: Duration,
    /// Dimensionalities with a refresh in flight, so a burst of requests arriving after the TTL
    /// lapses triggers **one** recompute each rather than one per request.
    pub refreshing: Mutex<HashSet<u8>>,
    /// Per-node embeddings for the optional semantic tier, keyed to the currently cached graph.
    /// Populated in the background after a graph build; `None` until (and unless) that lands.
    pub embeddings: RwLock<Option<Corpus>>,
}

/// The cached graph, refetching when the TTL has expired (or when `force`). Returns `None` only
/// when a fetch failed *and* there is no previous snapshot to fall back on.
///
/// Both endpoints share this so they can never disagree about which snapshot they are serving —
/// a detail request that refetched independently could describe a node the map does not show.
pub async fn cached_graph(
    state: &Arc<AppState>,
    force: bool,
    dimensions: u8,
) -> Option<Arc<GraphData>> {
    if !force {
        let cached = {
            let guard = state.cache.read().await;
            guard
                .get(&dimensions)
                .map(|(at, data)| (*at, Arc::clone(data)))
        };
        if let Some((at, data)) = cached {
            if at.elapsed() < state.ttl {
                return Some(data);
            }
            // Expired but present. Building a replacement means a fetch plus a layout that takes
            // seconds at this scale, and making a user wait for it — to be handed a map that is
            // at most an hour out of date — is the wrong trade. Serve what we have and refresh
            // behind them.
            spawn_refresh(state, dimensions);
            return Some(data);
        }
    }

    // Cold start (or an explicit ?refresh=1): there is nothing to serve but the real thing.
    match build_graph(&state.source, &state.opts, state.layout, dimensions).await {
        Ok(data) => {
            let data = Arc::new(data);
            state
                .cache
                .write()
                .await
                .insert(dimensions, (Instant::now(), Arc::clone(&data)));
            spawn_embed(state, Arc::clone(&data));
            Some(data)
        }
        Err(e) => {
            tracing::error!(error = ?e, "graph build failed");
            // Stale-if-error: serve the last good snapshot rather than a hard failure.
            state
                .cache
                .read()
                .await
                .get(&dimensions)
                .map(|(_, data)| Arc::clone(data))
        }
    }
}

/// Embed the freshly-cached graph, in the background.
///
/// Never blocks a request and never fails a request: on error the previous corpus is dropped and
/// search silently falls back to fuzzy matching, which is a perfectly serviceable answer.
fn spawn_embed(state: &Arc<AppState>, data: Arc<GraphData>) {
    if !state.embed.enabled() {
        return;
    }
    let state = Arc::clone(state);
    tokio::spawn(async move {
        match embed::embed_corpus(&state.embed, &data).await {
            Ok(vectors) => *state.embeddings.write().await = Some(Arc::new(vectors)),
            Err(e) => {
                tracing::warn!(error = ?e, "corpus embedding failed; search stays lexical");
                *state.embeddings.write().await = None;
            }
        }
    });
}

/// Rebuild the cached graph in the background, at most one at a time.
fn spawn_refresh(state: &Arc<AppState>, dimensions: u8) {
    // Claim the slot before spawning: two requests arriving in the same instant must not both
    // win the race and both start a multi-second layout.
    {
        let mut inflight = match state.refreshing.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !inflight.insert(dimensions) {
            return;
        }
    }

    let state = Arc::clone(state);
    tokio::spawn(async move {
        match build_graph(&state.source, &state.opts, state.layout, dimensions).await {
            Ok(data) => {
                let data = Arc::new(data);
                state
                    .cache
                    .write()
                    .await
                    .insert(dimensions, (Instant::now(), Arc::clone(&data)));
                spawn_embed(&state, data);
                tracing::info!(dimensions, "background graph refresh complete");
            }
            // The stale snapshot stays in the cache and keeps being served; the next expiry
            // will try again.
            Err(e) => tracing::error!(error = ?e, dimensions, "background graph refresh failed"),
        }
        if let Ok(mut inflight) = state.refreshing.lock() {
            inflight.remove(&dimensions);
        }
    });
}
