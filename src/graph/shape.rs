//! Turning raw Neo4j rows into display values: which label to show, which property is the
//! node's name, and which properties to send at all.
//!
//! Kept separate from the fetch because these are pure functions over a row — the part of the
//! transform that is worth testing exhaustively without a database anywhere near it.

use std::collections::BTreeMap;

use serde_json::Value;

use super::options::FetchOptions;

/// Stringify + truncate a property value for display. Char-safe truncation.
pub(super) fn stringify(val: &Value, max_chars: usize) -> String {
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

pub(super) fn clean_props(props: &Value, opts: &FetchOptions) -> BTreeMap<String, String> {
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
pub(super) fn pick_label(labels: &[String], wrappers: &[String]) -> (String, String) {
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

/// Pick a display name: the first non-empty key from `name_keys`, else the first non-empty
/// string property. `serde_json`'s object is a `BTreeMap`, so the no-priority-key fallback is
/// **deterministic (alphabetical by key)** rather than driver-return order.
pub(super) fn pick_name(props: &Value, name_keys: &[String]) -> String {
    let Value::Object(obj) = props else {
        return "?".to_string();
    };
    for key in name_keys {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use super::super::options::csv;

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
    fn pick_name_prefers_priority_keys_then_falls_back() {
        let keys = FetchOptions::default().name_keys;
        // Earlier key in the list wins over a later one.
        assert_eq!(
            pick_name(&json!({ "hostname": "n", "name": "N" }), &keys),
            "N"
        );
        assert_eq!(pick_name(&json!({ "hostname": "n" }), &keys), "n");
        // No priority key → first stringy value.
        assert_eq!(pick_name(&json!({ "foo": "bar" }), &keys), "bar");
        // No priority key, multiple props → deterministic alphabetical-by-key fallback
        // ("alpha" < "zeta"), regardless of JSON declaration order.
        assert_eq!(pick_name(&json!({ "zeta": "z", "alpha": "a" }), &keys), "a");
        // Blank strings are skipped.
        assert_eq!(
            pick_name(&json!({ "name": "  ", "title": "R" }), &keys),
            "R"
        );
        assert_eq!(pick_name(&json!({}), &keys), "?");
        // GRAPH_NAME_KEYS order is authoritative — a custom list overrides the defaults.
        let custom = csv("email,name");
        assert_eq!(
            pick_name(&json!({ "name": "N", "email": "e@x" }), &custom),
            "e@x"
        );
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

    /// The whole point of Phase 1: `/api/graph` must not carry property bags, and
    /// `/api/node/{id}` must. A regression here silently reinstates a multi-megabyte payload,
    /// so assert on the serialized JSON rather than on the struct.
    #[test]
    fn stringify_truncates_on_char_boundary() {
        let max = 600;
        let long = "é".repeat(max + 50); // multi-byte; byte-slicing would panic
        let out = stringify(&Value::String(long), max);
        assert_eq!(out.chars().count(), max + 1); // max chars + the ellipsis
        assert!(out.ends_with('…'));
    }
}
