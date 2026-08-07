//! Everything the service reads from the environment, in one place.
//!
//! No endpoint, credential, or private identifier is ever compiled in; if a default would need
//! to name one, it has no default. Keeping the parsing together makes that easy to audit — and
//! makes it obvious where a new knob belongs.

use anyhow::{Context, Result};
use neo4rs::{ConfigBuilder, Graph};

use nuvek_web::config;

use crate::layout::LayoutOptions;
use crate::state::Source;

/// Layout tuning from the environment. Defaults suit a few thousand to a few tens of thousands
/// of nodes; raise `GRAPH_LAYOUT_ITERATIONS` for a tighter layout at the cost of build time.
pub fn layout_options_from_env() -> Result<LayoutOptions> {
    let d = LayoutOptions::default();
    let parse =
        |key: &str, default: String| -> Result<String> { Ok(config::env_or(key, &default)) };
    Ok(LayoutOptions {
        iterations: parse("GRAPH_LAYOUT_ITERATIONS", d.iterations.to_string())?
            .parse()
            .context("GRAPH_LAYOUT_ITERATIONS must be a non-negative integer")?,
        theta: parse("GRAPH_LAYOUT_THETA", d.theta.to_string())?
            .parse()
            .context("GRAPH_LAYOUT_THETA must be a number")?,
        scale: parse("GRAPH_LAYOUT_SCALE", d.scale.to_string())?
            .parse()
            .context("GRAPH_LAYOUT_SCALE must be a number")?,
        gravity: parse("GRAPH_LAYOUT_GRAVITY", d.gravity.to_string())?
            .parse()
            .context("GRAPH_LAYOUT_GRAVITY must be a number")?,
        seed: parse("GRAPH_LAYOUT_SEED", d.seed.to_string())?
            .parse()
            .context("GRAPH_LAYOUT_SEED must be a non-negative integer")?,
    })
}

/// Pick the graph source from the environment.
///
/// `GRAPH_FIXTURE_NODES` selects the synthetic benchmark graph and **skips Neo4j entirely** —
/// deliberately, so a fixture run needs no endpoint and no credential and cannot accidentally
/// touch a real database.
pub async fn build_source() -> Result<Source> {
    let num = |key: &str, default: &str| -> Result<u64> {
        let raw = config::env_or(key, default);
        raw.parse()
            .with_context(|| format!("{key} must be a non-negative integer (got {raw:?})"))
    };

    let fixture_nodes = num("GRAPH_FIXTURE_NODES", "0")? as usize;
    if fixture_nodes > 0 {
        let edges_per_node = num("GRAPH_FIXTURE_EDGES", "3")? as usize;
        let seed = num("GRAPH_FIXTURE_SEED", "42")?;
        tracing::warn!(
            nodes = fixture_nodes,
            edges_per_node,
            seed,
            "GRAPH_FIXTURE_NODES is set — serving a synthetic graph; Neo4j will not be contacted"
        );
        return Ok(Source::Fixture {
            nodes: fixture_nodes,
            edges_per_node,
            seed,
        });
    }

    // Connection settings are runtime config only — NEVER baked into the image or binary, so
    // no endpoint or credential ships in the published image. NEO4J_HOST takes a full URI, so
    // `neo4j+s://…` (TLS / Aura) works without a code change.
    let uri = config::required_env("NEO4J_HOST")
        .context("NEO4J_HOST not set — inject it at runtime; never bake an endpoint in")?;
    let user = config::env_or("NEO4J_USER", "neo4j");
    let pass = config::required_env("NEO4J_PASSWORD")
        .context("NEO4J_PASSWORD not set — inject it at runtime (see .env.example)")?;
    let database = config::env_or("NEO4J_DATABASE", "neo4j");

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
    Ok(Source::Neo4j(graph))
}
