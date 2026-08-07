//! Scoring a query against every node.
//!
//! Two independent opinions, combined by the caller: fuzzy text matching across the fields a
//! node exposes, and — when embeddings are configured — cosine similarity in vector space.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::graph::GraphData;

use super::{LENGTH_TIEBREAK, WEIGHT_LABEL, WEIGHT_NAME, WEIGHT_PROPS};

/// Multiplier for a name that *is* the query. A fuzzy matcher scores "Host-42" and "Host-426"
/// identically — the query embeds cleanly in both — so without this, typing a name you already
/// know puts it in an arbitrary tie with every node that merely starts the same way.
const EXACT_NAME_BONUS: f32 = 1.6;

/// Score every node against `query`, lexically.
///
/// Returns a raw score per node, already field-weighted but not yet normalized.
pub(super) fn lexical_scores(data: &GraphData, query: &str) -> Vec<f32> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    // The `Pattern` API rather than `Matcher::fuzzy_match` directly: the latter requires a needle
    // that is already case-folded and normalized, and handing it a raw user query makes nucleo
    // panic in its own prefilter. `Pattern::parse` owns that normalization.
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut hay_buf = Vec::new();
    let mut out = vec![0.0f32; data.nodes.len()];

    let score_of = |text: &str, matcher: &mut Matcher, buf: &mut Vec<char>| -> f32 {
        pattern
            .score(Utf32Str::new(text, buf), matcher)
            .map_or(0.0, |s| s as f32)
    };

    let query_lower = query.to_lowercase();
    let query_chars = query.chars().count();

    for (i, node) in data.nodes.iter().enumerate() {
        let mut name_score = score_of(&node.name, &mut matcher, &mut hay_buf) * WEIGHT_NAME;
        if name_score > 0.0 {
            let extra = node.name.chars().count().saturating_sub(query_chars) as f32;
            name_score /= 1.0 + LENGTH_TIEBREAK * extra;
            if node.name.to_lowercase() == query_lower {
                name_score *= EXACT_NAME_BONUS;
            }
        }
        let mut best = name_score;
        best = best.max(score_of(&node.label, &mut matcher, &mut hay_buf) * WEIGHT_LABEL);
        for value in node.props.values() {
            best = best.max(score_of(value, &mut matcher, &mut hay_buf) * WEIGHT_PROPS);
        }
        out[i] = best;
    }
    out
}

/// Cosine similarity of the query embedding against each node's, mapped to 0..1.
pub(super) fn semantic_scores(
    query_vec: &[f32],
    corpus: &[Option<Vec<f32>>],
    n: usize,
) -> Vec<f32> {
    let mut out = vec![0.0f32; n];
    let qn = query_vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if qn <= f32::EPSILON {
        return out;
    }
    for (i, slot) in corpus.iter().enumerate().take(n) {
        let Some(v) = slot else { continue };
        if v.len() != query_vec.len() {
            continue;
        }
        let mut dot = 0.0f32;
        let mut vn = 0.0f32;
        for (a, b) in v.iter().zip(query_vec) {
            dot += a * b;
            vn += a * a;
        }
        let vn = vn.sqrt();
        if vn <= f32::EPSILON {
            continue;
        }
        // Cosine is -1..1; the negative half is noise for this purpose, so clamp rather than
        // rescale — an unrelated node should score zero, not "half relevant".
        out[i] = (dot / (qn * vn)).clamp(0.0, 1.0);
    }
    out
}
