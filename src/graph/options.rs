//! Runtime shaping options for a graph fetch.
//!
//! Everything schema-specific lives here and comes from the environment, so pointing the viewer
//! at a different database is configuration rather than a code change.

use anyhow::{Context, Result};

use nuvek_web::config;

/// Property keys dropped from the payload by default: vector embeddings, which are large,
/// unreadable, and would dominate the response. Extend via `GRAPH_SKIP_PROPS`.
const DEFAULT_SKIP_PROPS: &str = "embedding,vector";
/// Caps exist because this viewer renders the *whole* graph. A force simulation stops being
/// usable long before a real database stops being able to return rows, so the honest failure
/// is a truncated graph with a warning, not a hung request.
const DEFAULT_MAX_NODES: &str = "30000";
const DEFAULT_MAX_RELS: &str = "60000";
const DEFAULT_MAX_PROP_CHARS: &str = "600";
/// Property keys tried, in order, when choosing a node's display name. Deliberately broad:
/// the first one a node actually has wins, so one default suits several schemas.
const DEFAULT_NAME_KEYS: &str = "name,title,displayName,id,hostname,canonical";

/// Runtime shaping options — all of it env-driven so the viewer stays schema-agnostic.
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// Labels that namespace a node rather than describe it (e.g. a base label every node in
    /// one subgraph carries). They are dropped when picking the display label, and the first
    /// one a node carries becomes its `group`. Empty by default.
    pub wrapper_labels: Vec<String>,
    /// Property keys never sent to the browser.
    pub skip_props: Vec<String>,
    /// Property keys tried in order when choosing a node's display name.
    pub name_keys: Vec<String>,
    /// Only fetch nodes carrying one of these labels. Empty = every label.
    pub node_labels: Vec<String>,
    /// Only fetch relationships of these types. Empty = every type.
    pub rel_types: Vec<String>,
    pub max_nodes: i64,
    pub max_rels: i64,
    /// Longest property value rendered before truncation.
    pub max_prop_chars: usize,
}

pub(super) fn csv(raw: &str) -> Vec<String> {
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
            name_keys: csv(&config::env_or("GRAPH_NAME_KEYS", DEFAULT_NAME_KEYS)),
            node_labels: csv(&config::env_or("GRAPH_NODE_LABELS", "")),
            rel_types: csv(&config::env_or("GRAPH_REL_TYPES", "")),
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
            name_keys: csv(DEFAULT_NAME_KEYS),
            node_labels: Vec::new(),
            rel_types: Vec::new(),
            max_nodes: 30_000,
            max_rels: 60_000,
            max_prop_chars: 600,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_parses_and_tolerates_whitespace_and_blanks() {
        assert_eq!(csv(""), Vec::<String>::new());
        assert_eq!(csv(" a , b ,, c "), vec!["a", "b", "c"]);
    }
}
