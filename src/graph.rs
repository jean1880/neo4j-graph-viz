//! Fetch the Neo4j graph and shape it as `{nodes, links}` for the force-graph viewer.
//! Secrets stay server-side — the browser only ever receives this JSON via `GET /api/graph`,
//! never a Neo4j endpoint or credentials.
//!
//! Everything schema-specific is runtime config (see [`FetchOptions`]), so pointing the viewer
//! at a different database never requires a code change.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use neo4rs::{query, Graph};
use serde::Serialize;
use serde_json::Value;

use nuvek_web::config;

/// Property keys dropped from the payload by default: vector embeddings, which are large,
/// unreadable, and would dominate the response. Extend via `GRAPH_SKIP_PROPS`.
const DEFAULT_SKIP_PROPS: &str = "embedding,vector";
/// Caps exist because this viewer renders the *whole* graph. A force simulation stops being
/// usable long before a real database stops being able to return rows, so the honest failure
/// is a truncated graph with a warning, not a hung request.
const DEFAULT_MAX_NODES: &str = "10000";
const DEFAULT_MAX_RELS: &str = "20000";
const DEFAULT_MAX_PROP_CHARS: &str = "600";

/// Runtime shaping options — all of it env-driven so the viewer stays schema-agnostic.
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// Labels that namespace a node rather than describe it (e.g. a base label every node in
    /// one subgraph carries). They are dropped when picking the display label, and the first
    /// one a node carries becomes its `group`. Empty by default.
    pub wrapper_labels: Vec<String>,
    /// Property keys never sent to the browser.
    pub skip_props: Vec<String>,
    pub max_nodes: i64,
    pub max_rels: i64,
    /// Longest property value rendered before truncation.
    pub max_prop_chars: usize,
}

fn csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

impl FetchOptions {
    pub fn from_env() -> Result<Self> {
        let num = |key: &str, default: &str| -> Result<i64> {
            let raw = config::env_or(key, default);
            raw.parse()
                .with_context(|| format!("{key} must be a positive integer (got {raw:?})"))
        };
        Ok(Self {
            wrapper_labels: csv(&config::env_or("GRAPH_WRAPPER_LABELS", "")),
            skip_props: csv(&config::env_or("GRAPH_SKIP_PROPS", DEFAULT_SKIP_PROPS)),
            max_nodes: num("GRAPH_MAX_NODES", DEFAULT_MAX_NODES)?,
            max_rels: num("GRAPH_MAX_RELS", DEFAULT_MAX_RELS)?,
            max_prop_chars: num("GRAPH_MAX_PROP_CHARS", DEFAULT_MAX_PROP_CHARS)? as usize,
        })
    }
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            wrapper_labels: Vec::new(),
            skip_props: csv(DEFAULT_SKIP_PROPS),
            max_nodes: 10_000,
            max_rels: 20_000,
            max_prop_chars: 600,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub label: String,
    pub group: String,
    pub deg: u32,
    pub props: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GraphLink {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub rel: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
}

/// Stringify + truncate a property value for display. Char-safe truncation.
fn stringify(val: &Value, max_chars: usize) -> String {
    let text = match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => val.to_string(),
        other => other.to_string(),
    };
    if text.chars().count() > max_chars {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{truncated}…")
    } else {
        text
    }
}

fn clean_props(props: &Value, opts: &FetchOptions) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Value::Object(map) = props {
        for (key, val) in map {
            if opts.skip_props.iter().any(|s| s == key) {
                continue;
            }
            out.insert(key.clone(), stringify(val, opts.max_prop_chars));
        }
    }
    out
}

/// Return `(display_label, group)`. The display label is the first label that is not a
/// configured wrapper; `group` is the first wrapper the node carries, or empty when none
/// are configured or matched (the UI then omits it).
fn pick_label(labels: &[String], wrappers: &[String]) -> (String, String) {
    let is_wrapper = |l: &String| wrappers.iter().any(|w| w == l);
    let specific = labels
        .iter()
        .find(|l| !is_wrapper(l))
        .or_else(|| labels.first())
        .cloned()
        .unwrap_or_else(|| "Node".to_string());
    let group = labels
        .iter()
        .find(|l| is_wrapper(l))
        .cloned()
        .unwrap_or_default();
    (specific, group)
}

/// Pick a display name: the first non-empty priority key (`name`/`title`/`id`/`hostname`/
/// `canonical`), else the first non-empty string property. `serde_json`'s object is a
/// `BTreeMap`, so the no-priority-key fallback is **deterministic (alphabetical by key)**
/// rather than driver-return order.
fn pick_name(props: &Value) -> String {
    let Value::Object(obj) = props else {
        return "?".to_string();
    };
    for key in ["name", "title", "id", "hostname", "canonical"] {
        if let Some(Value::String(s)) = obj.get(key) {
            if !s.trim().is_empty() {
                return s.clone();
            }
        }
    }
    for val in obj.values() {
        if let Value::String(s) = val {
            if !s.trim().is_empty() {
                return s.clone();
            }
        }
    }
    "?".to_string()
}

/// Fetch the graph and build the `{nodes, links}` payload (with per-node degree).
///
/// Both queries are `LIMIT`-bounded (see [`FetchOptions`]) — this endpoint returns the entire
/// graph, so an unbounded scan against a large database would exhaust memory here and hand the
/// browser a payload no force simulation can lay out.
pub async fn fetch(graph: &Graph, opts: &FetchOptions) -> Result<GraphData> {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();

    let mut node_rows = graph
        .execute(
            query(
                "MATCH (n) RETURN elementId(n) AS id, labels(n) AS labels, \
                 properties(n) AS props LIMIT $limit",
            )
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
                name: pick_name(&props),
                label,
                group,
                deg: 0,
                props: clean_props(&props, opts),
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
    let mut rel_rows = graph
        .execute(
            query(
                "MATCH (a)-[r]->(b) RETURN elementId(a) AS s, elementId(b) AS t, \
                 type(r) AS rel LIMIT $limit",
            )
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

    Ok(GraphData {
        nodes: nodes.into_values().collect(),
        links,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pick_label_drops_configured_wrappers_and_groups() {
        let wrappers = labels(&["Base", "Legacy"]);
        // Wrapper present → it becomes the group; the display label is the other one.
        assert_eq!(
            pick_label(&labels(&["Base", "Service"]), &wrappers),
            ("Service".to_string(), "Base".to_string())
        );
        // Label order must not matter.
        assert_eq!(
            pick_label(&labels(&["Host", "Legacy"]), &wrappers),
            ("Host".to_string(), "Legacy".to_string())
        );
        // No wrapper matched → empty group, which the UI omits.
        assert_eq!(
            pick_label(&labels(&["Host"]), &wrappers),
            ("Host".to_string(), String::new())
        );
        // Nothing configured (the default) → label passes through, never a group.
        assert_eq!(
            pick_label(&labels(&["Host"]), &[]),
            ("Host".to_string(), String::new())
        );
        // Every label is a wrapper → fall back to the first rather than inventing one.
        assert_eq!(
            pick_label(&labels(&["Base"]), &wrappers),
            ("Base".to_string(), "Base".to_string())
        );
        // No labels at all → fallback.
        assert_eq!(
            pick_label(&[], &wrappers),
            ("Node".to_string(), String::new())
        );
    }

    #[test]
    fn csv_parses_and_tolerates_whitespace_and_blanks() {
        assert_eq!(csv(""), Vec::<String>::new());
        assert_eq!(csv(" a , b ,, c "), vec!["a", "b", "c"]);
    }

    #[test]
    fn pick_name_prefers_priority_keys_then_falls_back() {
        assert_eq!(
            pick_name(&json!({ "hostname": "nas", "name": "NAS" })),
            "NAS"
        );
        assert_eq!(pick_name(&json!({ "hostname": "nas" })), "nas");
        // No priority key → first stringy value.
        assert_eq!(pick_name(&json!({ "foo": "bar" })), "bar");
        // No priority key, multiple props → deterministic alphabetical-by-key fallback
        // ("alpha" < "zeta"), regardless of JSON declaration order.
        assert_eq!(pick_name(&json!({ "zeta": "z", "alpha": "a" })), "a");
        // Blank strings are skipped.
        assert_eq!(pick_name(&json!({ "name": "  ", "title": "Real" })), "Real");
        assert_eq!(pick_name(&json!({})), "?");
    }

    #[test]
    fn clean_props_skips_configured_keys_and_stringifies() {
        let opts = FetchOptions {
            skip_props: vec!["embedding".into(), "secret".into()],
            ..FetchOptions::default()
        };
        let props = json!({
            "name": "x",
            "embedding": [0.1, 0.2],
            "secret": "hunter2",
            "count": 3,
            "tags": ["a", "b"],
        });
        let out = clean_props(&props, &opts);
        assert!(!out.contains_key("embedding"));
        // A property named in GRAPH_SKIP_PROPS never reaches the browser.
        assert!(!out.contains_key("secret"));
        assert_eq!(out.get("count").map(String::as_str), Some("3"));
        assert_eq!(out.get("tags").map(String::as_str), Some("[\"a\",\"b\"]"));
    }

    #[test]
    fn stringify_truncates_on_char_boundary() {
        let max = 600;
        let long = "é".repeat(max + 50); // multi-byte; byte-slicing would panic
        let out = stringify(&Value::String(long), max);
        assert_eq!(out.chars().count(), max + 1); // max chars + the ellipsis
        assert!(out.ends_with('…'));
    }
}
