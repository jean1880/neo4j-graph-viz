//! Fetch the Neo4j graph and shape it as `{nodes, links}` for the viewer.
//!
//! Secrets stay server-side — the browser only ever receives this JSON, never a Neo4j endpoint
//! or credentials. Everything schema-specific is runtime config (see [`FetchOptions`]), so
//! pointing the viewer at a different database never requires a code change.
//!
//! The module splits by ownership: [`options`] holds the env-driven configuration, [`shape`] the
//! pure row-to-display-value transforms, [`model`] the payload types and their indices,
//! [`fixture`] the synthetic benchmark graph, and this file the Bolt query itself.

mod fixture;
mod model;
mod options;
mod shape;

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use neo4rs::{query, Graph};
use serde_json::Value;

pub use fixture::fixture;
pub use model::{GraphData, GraphLink, GraphNode};
pub use options::FetchOptions;

use shape::{clean_props, pick_label, pick_name};

pub async fn fetch(graph: &Graph, opts: &FetchOptions) -> Result<GraphData> {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();

    // Label / type allow-lists are operator config, so they are passed as parameters rather
    // than spliced into the Cypher — the query text stays fixed and plan-cacheable.
    let node_filter = if opts.node_labels.is_empty() {
        ""
    } else {
        "WHERE any(l IN labels(n) WHERE l IN $labels) "
    };
    let node_cypher = format!(
        "MATCH (n) {node_filter}RETURN elementId(n) AS id, labels(n) AS labels, \
         properties(n) AS props LIMIT $limit"
    );
    let mut node_rows = graph
        .execute(
            query(&node_cypher)
                .param("labels", opts.node_labels.clone())
                .param("limit", opts.max_nodes),
        )
        .await
        .context("node query failed")?;
    while let Some(row) = node_rows.next().await? {
        let id: String = row.get("id").context("node id")?;
        let labels: Vec<String> = row.get("labels").unwrap_or_default();
        let props: Value = row.get("props").unwrap_or(Value::Null);
        let (label, group) = pick_label(&labels, &opts.wrapper_labels);
        nodes.insert(
            id.clone(),
            GraphNode {
                id,
                name: pick_name(&props, &opts.name_keys),
                label,
                group,
                deg: 0,
                props: clean_props(&props, opts),
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
    }
    if nodes.len() as i64 >= opts.max_nodes {
        tracing::warn!(
            limit = opts.max_nodes,
            "node limit reached — the graph is truncated; raise GRAPH_MAX_NODES to see more"
        );
    }

    let mut links: Vec<GraphLink> = Vec::new();
    let rel_filter = if opts.rel_types.is_empty() {
        ""
    } else {
        "WHERE type(r) IN $types "
    };
    let rel_cypher = format!(
        "MATCH (a)-[r]->(b) {rel_filter}RETURN elementId(a) AS s, elementId(b) AS t, \
         type(r) AS rel LIMIT $limit"
    );
    let mut rel_rows = graph
        .execute(
            query(&rel_cypher)
                .param("types", opts.rel_types.clone())
                .param("limit", opts.max_rels),
        )
        .await
        .context("relationship query failed")?;
    let mut rel_rows_seen: i64 = 0;
    while let Some(row) = rel_rows.next().await? {
        rel_rows_seen += 1;
        let s: String = row.get("s").context("rel source")?;
        let t: String = row.get("t").context("rel target")?;
        let rel: String = row.get("rel").unwrap_or_default();
        // Endpoints outside the fetched node set are skipped — otherwise a truncated node
        // query would leave links pointing at nodes the browser never received.
        if nodes.contains_key(&s) && nodes.contains_key(&t) {
            if let Some(n) = nodes.get_mut(&s) {
                n.deg += 1;
            }
            if let Some(n) = nodes.get_mut(&t) {
                n.deg += 1;
            }
            links.push(GraphLink {
                source: s,
                target: t,
                rel,
            });
        }
    }
    if rel_rows_seen >= opts.max_rels {
        tracing::warn!(
            limit = opts.max_rels,
            "relationship limit reached — the graph is truncated; raise GRAPH_MAX_RELS to see more"
        );
    }

    Ok(GraphData::new(nodes.into_values().collect(), links))
}
