//! Fetch the whole Neo4j graph and shape it as `{nodes, links}` for the force-graph viewer.
//! Ported from the original `fetch_graph.py`. Secrets stay server-side — the browser only ever
//! receives this JSON via `GET /api/graph`, never a Neo4j endpoint or credentials.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use neo4rs::{query, Graph};
use serde::Serialize;
use serde_json::Value;

// Base/wrapper labels that scope orphan-pruning per writer rather than naming a display type
// (MH = D&D campaign namespace, HL = homelab namespace).
const GENERIC: [&str; 2] = ["MH", "HL"];
// Property keys omitted from the detail overlay (embeddings, internal stamps).
const SKIP_PROPS: [&str; 3] = ["embedding", "vector", "sync_id"];
const MAX_VAL: usize = 600;

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

/// Stringify + truncate a property value for display (mirrors `clean_props`). Char-safe truncation.
fn stringify(val: &Value) -> String {
    let text = match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => val.to_string(),
        other => other.to_string(),
    };
    if text.chars().count() > MAX_VAL {
        let truncated: String = text.chars().take(MAX_VAL).collect();
        format!("{truncated}…")
    } else {
        text
    }
}

fn clean_props(props: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Value::Object(map) = props {
        for (key, val) in map {
            if SKIP_PROPS.contains(&key.as_str()) {
                continue;
            }
            out.insert(key.clone(), stringify(val));
        }
    }
    out
}

/// Return `(specific_label, group)`. `group` folds any `MH` node into one domain.
fn pick_label(labels: &[String]) -> (String, String) {
    let specific = labels
        .iter()
        .find(|l| !GENERIC.contains(&l.as_str()))
        .or_else(|| labels.first())
        .cloned()
        .unwrap_or_else(|| "Node".to_string());
    let group = if labels.iter().any(|l| l == "MH") {
        "MH"
    } else {
        "infra"
    };
    (specific, group.to_string())
}

/// Pick a display name: the first non-empty priority key (`name`/`title`/`id`/`hostname`/
/// `canonical`), else the first non-empty string property. `serde_json`'s object is a
/// `BTreeMap`, so the no-priority-key fallback is **deterministic (alphabetical by key)** —
/// not driver-return order like the Python original; the priority keys cover the real nodes.
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

/// Fetch the whole graph and build the `{nodes, links}` payload (with per-node degree).
pub async fn fetch(graph: &Graph) -> Result<GraphData> {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();

    let mut node_rows = graph
        .execute(query(
            "MATCH (n) RETURN elementId(n) AS id, labels(n) AS labels, properties(n) AS props",
        ))
        .await
        .context("node query failed")?;
    while let Some(row) = node_rows.next().await? {
        let id: String = row.get("id").context("node id")?;
        let labels: Vec<String> = row.get("labels").unwrap_or_default();
        let props: Value = row.get("props").unwrap_or(Value::Null);
        let (label, group) = pick_label(&labels);
        nodes.insert(
            id.clone(),
            GraphNode {
                id,
                name: pick_name(&props),
                label,
                group,
                deg: 0,
                props: clean_props(&props),
            },
        );
    }

    let mut links: Vec<GraphLink> = Vec::new();
    let mut rel_rows = graph
        .execute(query(
            "MATCH (a)-[r]->(b) RETURN elementId(a) AS s, elementId(b) AS t, type(r) AS rel",
        ))
        .await
        .context("relationship query failed")?;
    while let Some(row) = rel_rows.next().await? {
        let s: String = row.get("s").context("rel source")?;
        let t: String = row.get("t").context("rel target")?;
        let rel: String = row.get("rel").unwrap_or_default();
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
    fn pick_label_drops_generic_wrappers_and_groups() {
        // MH present → group MH; specific label is the non-generic one.
        assert_eq!(
            pick_label(&labels(&["MH", "Deity"])),
            ("Deity".to_string(), "MH".to_string())
        );
        // Only infra labels → group infra.
        assert_eq!(
            pick_label(&labels(&["Host", "HL"])),
            ("Host".to_string(), "infra".to_string())
        );
        // No labels at all → fallback.
        assert_eq!(pick_label(&[]), ("Node".to_string(), "infra".to_string()));
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
    fn clean_props_skips_internal_keys_and_stringifies() {
        let props = json!({
            "name": "x",
            "embedding": [0.1, 0.2],
            "sync_id": "abc",
            "count": 3,
            "tags": ["a", "b"],
        });
        let out = clean_props(&props);
        assert!(!out.contains_key("embedding"));
        assert!(!out.contains_key("sync_id"));
        assert_eq!(out.get("count").map(String::as_str), Some("3"));
        assert_eq!(out.get("tags").map(String::as_str), Some("[\"a\",\"b\"]"));
    }

    #[test]
    fn stringify_truncates_on_char_boundary() {
        let long = "é".repeat(MAX_VAL + 50); // multi-byte; byte-slicing would panic
        let out = stringify(&Value::String(long));
        assert_eq!(out.chars().count(), MAX_VAL + 1); // MAX_VAL chars + the ellipsis
        assert!(out.ends_with('…'));
    }
}
