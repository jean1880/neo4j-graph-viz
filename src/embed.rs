//! Optional semantic-search tier: text embeddings from an Ollama-compatible endpoint.
//!
//! **Entirely opt-in.** With `SEARCH_EMBED_URL` unset — which is how the published image ships —
//! nothing here ever runs, no request is made, and search falls back to fuzzy matching. That
//! matters because this repo is public and self-contained by design: a hard dependency on an
//! embedding service would make the image useless to anyone who does not happen to run one.
//!
//! The endpoint is runtime configuration like every other endpoint in this service. Nothing
//! about it is compiled in.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use nuvek_web::config;

use crate::graph::GraphData;

#[derive(Debug, Clone)]
pub struct EmbedOptions {
    /// Base URL of the embedding service, e.g. `http://host:11434`. `None` disables the tier.
    pub url: Option<String>,
    pub model: String,
    /// Nodes sent per request. Embedding is the slow part of a refresh, and batching is what
    /// keeps it to minutes rather than hours.
    pub batch: usize,
    /// Ceiling on how many nodes get embedded, highest-degree first. Embedding 25 000 nodes on
    /// every cache fill is rarely worth the wall-clock; the hubs carry most of the meaning.
    pub max_nodes: usize,
    pub timeout: Duration,
}

impl EmbedOptions {
    pub fn from_env() -> Result<Self> {
        let url = config::env_or("SEARCH_EMBED_URL", "");
        let num = |key: &str, default: &str| -> Result<usize> {
            let raw = config::env_or(key, default);
            raw.parse()
                .with_context(|| format!("{key} must be a non-negative integer (got {raw:?})"))
        };
        Ok(Self {
            url: if url.trim().is_empty() {
                None
            } else {
                Some(url.trim().trim_end_matches('/').to_string())
            },
            model: config::env_or("SEARCH_EMBED_MODEL", "nomic-embed-text"),
            batch: num("SEARCH_EMBED_BATCH", "64")?.max(1),
            max_nodes: num("SEARCH_EMBED_MAX_NODES", "5000")?,
            timeout: Duration::from_secs(num("SEARCH_EMBED_TIMEOUT_SECS", "120")? as u64),
        })
    }

    pub fn enabled(&self) -> bool {
        self.url.is_some()
    }
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

/// The text a node is represented by. Name and label first because they carry the most signal,
/// then property values — which is the whole reason search moved server-side.
fn node_text(node: &crate::graph::GraphNode) -> String {
    let mut s = format!("{} ({})", node.name, node.label);
    for (k, v) in &node.props {
        if k == "name" {
            continue;
        }
        s.push_str("; ");
        s.push_str(k);
        s.push_str(": ");
        s.push_str(v);
        if s.len() > 900 {
            break;
        }
    }
    s
}

async fn embed_batch(
    client: &reqwest::Client,
    opts: &EmbedOptions,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    let url = opts
        .url
        .as_ref()
        .context("embedding URL not configured")?
        .clone();
    let res = client
        .post(format!("{url}/api/embed"))
        .json(&EmbedRequest {
            model: &opts.model,
            input: texts,
        })
        .send()
        .await
        .context("embedding request failed")?;
    if !res.status().is_success() {
        anyhow::bail!("embedding endpoint returned {}", res.status());
    }
    let body: EmbedResponse = res.json().await.context("malformed embedding response")?;
    Ok(body.embeddings)
}

/// Embed a single query string.
pub async fn embed_query(opts: &EmbedOptions, query: &str) -> Result<Vec<f32>> {
    let client = reqwest::Client::builder()
        .timeout(opts.timeout)
        .build()
        .context("failed to build embedding HTTP client")?;
    let mut out = embed_batch(&client, opts, &[query.to_string()]).await?;
    if out.is_empty() {
        anyhow::bail!("embedding endpoint returned no vector for the query");
    }
    Ok(out.remove(0))
}

/// Embed the graph's nodes, highest-degree first up to `max_nodes`.
///
/// Returns one slot per node, `None` where a node was not embedded (below the degree cut, or
/// its batch failed). Search treats a missing embedding as "no semantic opinion" rather than
/// as a zero score, so a partial corpus degrades cleanly instead of burying those nodes.
pub async fn embed_corpus(opts: &EmbedOptions, data: &GraphData) -> Result<Vec<Option<Vec<f32>>>> {
    let n = data.nodes.len();
    let mut out: Vec<Option<Vec<f32>>> = vec![None; n];
    if !opts.enabled() || n == 0 {
        return Ok(out);
    }

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        data.nodes[b]
            .deg
            .cmp(&data.nodes[a].deg)
            .then_with(|| a.cmp(&b))
    });
    order.truncate(opts.max_nodes.min(n));

    let client = reqwest::Client::builder()
        .timeout(opts.timeout)
        .build()
        .context("failed to build embedding HTTP client")?;

    let started = std::time::Instant::now();
    let mut embedded = 0usize;
    let mut failed_batches = 0usize;
    for chunk in order.chunks(opts.batch) {
        let texts: Vec<String> = chunk.iter().map(|&i| node_text(&data.nodes[i])).collect();
        match embed_batch(&client, opts, &texts).await {
            Ok(vectors) => {
                for (&i, v) in chunk.iter().zip(vectors) {
                    out[i] = Some(v);
                    embedded += 1;
                }
            }
            // One bad batch must not abandon the whole corpus — the rest is still useful.
            Err(e) => {
                failed_batches += 1;
                tracing::warn!(error = ?e, "embedding batch failed; continuing without it");
            }
        }
    }

    tracing::info!(
        embedded,
        of = n,
        failed_batches,
        elapsed_ms = started.elapsed().as_millis(),
        model = %opts.model,
        "semantic search corpus embedded"
    );
    Ok(out)
}
