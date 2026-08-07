//! Server-side "smart" search that **removes** unrelated nodes rather than dimming them.
//!
//! It lives here rather than in the browser for two reasons. Node properties no longer travel
//! with the graph payload (they dominated it at scale), so only the server can match against
//! them; and the interesting part — deciding what counts as *related* — is a graph traversal,
//! which wants the adjacency structure the server already holds.
//!
//! # How relatedness is decided
//!
//! Not "matches, plus everything within N hops". A fixed hop count treats a barely-relevant
//! match the same as a bullseye, so one loose match drags half the graph back in. Instead each
//! match's score propagates outward, decaying with distance:
//!
//! ```text
//! visible(n)  ⟺  max over matches m of ( score(m) × decay^dist(m,n) )  ≥  threshold
//! ```
//!
//! A strong match keeps its neighbourhood; a weak one keeps only itself. Match strength and
//! graph distance collapse into one number, so the UI needs a single "breadth" control rather
//! than separate hop-count and score knobs.

use std::collections::BinaryHeap;

use serde::Serialize;

use crate::graph::GraphData;

/// Relative weight of each field a query is matched against. A query is far more likely to be
/// reaching for a node's name than for a word buried in one of its properties.
const WEIGHT_NAME: f32 = 1.0;
const WEIGHT_LABEL: f32 = 0.55;
const WEIGHT_PROPS: f32 = 0.40;

/// Gentle preference for shorter names among otherwise equal matches, so "Host-426" outranks
/// "Host-42109". Deliberately mild: it breaks ties, it does not reorder real differences.
const LENGTH_TIEBREAK: f32 = 0.02;

#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    /// Score multiplier per hop away from a match. Lower confines results more tightly.
    pub decay: f32,
    /// Ranked matches returned to the client for display.
    pub max_matches: usize,
    /// Ceiling on the visible set, so a pathological query cannot ask the client to render
    /// (and re-lay-out) the whole graph.
    pub max_visible: usize,
    /// How strongly a semantic score counts relative to a lexical one, when embeddings are
    /// available. Lexical still wins outright on an exact name match, which is what a user
    /// typing a name they already know expects.
    pub semantic_weight: f32,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            decay: 0.5,
            max_matches: 50,
            max_visible: 4000,
            semantic_weight: 0.75,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SearchMatch {
    pub id: String,
    pub name: String,
    pub label: String,
    pub score: f32,
}

/// A node that survived the search, with the position it should be drawn at. Coordinates are
/// recomputed over the surviving subgraph, so results are laid out compactly instead of being
/// scattered across the holes left by everything that was removed.
#[derive(Debug, Serialize)]
pub struct VisibleNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub visible: Vec<VisibleNode>,
    /// True when the visible set hit `max_visible` and was cut short — the client says so
    /// rather than silently showing a partial answer.
    pub truncated: bool,
    /// Whether semantic scoring contributed, so the UI can be honest about what it did.
    pub semantic: bool,
}

mod score;

use score::{lexical_scores, semantic_scores};

/// Run a search. `query_embedding` and `corpus_embeddings` are optional; without them this is
/// pure lexical matching and no semantic work happens at all.
pub fn search(
    data: &GraphData,
    query: &str,
    breadth: f32,
    opts: &SearchOptions,
    query_embedding: Option<&[f32]>,
    corpus_embeddings: Option<&[Option<Vec<f32>>]>,
) -> SearchResponse {
    let query = query.trim();
    let n = data.nodes.len();
    if query.is_empty() || n == 0 {
        return SearchResponse {
            query: query.to_string(),
            matches: Vec::new(),
            visible: Vec::new(),
            truncated: false,
            semantic: false,
        };
    }

    // --- score ------------------------------------------------------------------------------
    let lexical = lexical_scores(data, query);
    let lex_max = lexical.iter().copied().fold(0.0f32, f32::max);
    // Normalize against the best hit rather than a theoretical maximum: fuzzy scores are only
    // meaningful relative to each other, and the top result should anchor at 1.0.
    let mut score: Vec<f32> = if lex_max > 0.0 {
        lexical.iter().map(|s| s / lex_max).collect()
    } else {
        vec![0.0; n]
    };

    let semantic_used = match (query_embedding, corpus_embeddings) {
        (Some(q), Some(corpus)) if !q.is_empty() => {
            let sem = semantic_scores(q, corpus, n);
            for (s, sv) in score.iter_mut().zip(&sem) {
                *s = s.max(sv * opts.semantic_weight);
            }
            true
        }
        _ => false,
    };

    // --- seed the frontier ------------------------------------------------------------------
    // Anything scoring this low is noise; fuzzy matchers will find *some* alignment in almost
    // any string, and admitting all of it would make every query return the whole graph.
    const SEED_FLOOR: f32 = 0.18;
    let breadth = breadth.clamp(0.0, 1.0);
    // breadth 0 → only the matches themselves; breadth 1 → propagate as far as decay allows.
    let threshold = (1.0 - breadth).clamp(0.02, 1.0);

    let mut best = vec![0.0f32; n];
    let mut heap: BinaryHeap<(ordered::F32, u32)> = BinaryHeap::new();
    for (i, &s) in score.iter().enumerate() {
        if s >= SEED_FLOOR && s >= threshold * SEED_FLOOR {
            best[i] = s;
            heap.push((ordered::F32(s), i as u32));
        }
    }

    // --- propagate outward, best-first ------------------------------------------------------
    // Best-first (rather than plain BFS) means a node is finalized the first time it is popped,
    // because no later path can reach it with a higher score — decay only ever reduces.
    let mut visible_idx: Vec<u32> = Vec::new();
    let mut truncated = false;
    let mut settled = vec![false; n];
    while let Some((ordered::F32(s), i)) = heap.pop() {
        let i = i as usize;
        if settled[i] {
            continue;
        }
        settled[i] = true;
        if s < threshold {
            continue;
        }
        visible_idx.push(i as u32);
        if visible_idx.len() >= opts.max_visible {
            truncated = true;
            break;
        }
        let next = s * opts.decay;
        if next < threshold {
            continue;
        }
        for &nb in data.neighbours(i) {
            let nb = nb as usize;
            if !settled[nb] && next > best[nb] {
                best[nb] = next;
                heap.push((ordered::F32(next), nb as u32));
            }
        }
    }

    // --- rank the matches for display -------------------------------------------------------
    let mut ranked: Vec<(usize, f32)> = score
        .iter()
        .enumerate()
        .filter(|(_, &s)| s >= SEED_FLOOR)
        .map(|(i, &s)| (i, s))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(opts.max_matches);

    let matches = ranked
        .into_iter()
        .map(|(i, s)| SearchMatch {
            id: data.nodes[i].id.clone(),
            name: data.nodes[i].name.clone(),
            label: data.nodes[i].label.clone(),
            score: s,
        })
        .collect();

    SearchResponse {
        query: query.to_string(),
        matches,
        // Positions are filled in by the caller, which owns the layout options.
        visible: visible_idx
            .into_iter()
            .map(|i| VisibleNode {
                id: data.nodes[i as usize].id.clone(),
                x: data.nodes[i as usize].x,
                y: data.nodes[i as usize].y,
            })
            .collect(),
        truncated,
        semantic: semantic_used,
    }
}

/// A total-ordering wrapper so `f32` scores can drive a `BinaryHeap`.
mod ordered {
    #[derive(PartialEq)]
    pub struct F32(pub f32);
    impl Eq for F32 {}
    impl PartialOrd for F32 {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for F32 {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.0.total_cmp(&other.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{FetchOptions, GraphData};

    fn fixture() -> GraphData {
        crate::graph::fixture(200, 2, 7, &FetchOptions::default())
    }

    fn ids(r: &SearchResponse) -> Vec<&str> {
        r.visible.iter().map(|v| v.id.as_str()).collect()
    }

    #[test]
    fn empty_query_returns_nothing() {
        let g = fixture();
        let r = search(&g, "   ", 0.6, &SearchOptions::default(), None, None);
        assert!(r.matches.is_empty());
        assert!(r.visible.is_empty());
        assert!(!r.semantic);
    }

    /// An exact name must win outright, not tie with every node that merely starts the same way
    /// — a fuzzy matcher scores "Host-42" and "Host-426" identically on its own.
    #[test]
    fn exact_name_ranks_first_and_beats_prefix_matches() {
        // Big enough to actually contain the competitors ("Host-426", "Host-4210", …); the
        // 200-node fixture has only one node whose name contains "Host-42", so it cannot
        // exercise the tie at all.
        let g = crate::graph::fixture(600, 2, 7, &FetchOptions::default());
        let r = search(&g, "Host-42", 0.6, &SearchOptions::default(), None, None);
        assert_eq!(r.matches[0].name, "Host-42");
        assert!((r.matches[0].score - 1.0).abs() < 1e-6);
        assert!(ids(&r).contains(&"fixture:42"));

        // The runner-up is a longer name that merely contains the query, and must score lower.
        let second = &r.matches[1];
        assert!(
            second.score < 0.95,
            "prefix match {:?} tied with the exact match at {}",
            second.name,
            second.score
        );
    }

    /// Among equally-fuzzy matches, the shorter name is the better answer.
    #[test]
    fn shorter_names_break_ties() {
        let g = crate::graph::fixture(600, 2, 7, &FetchOptions::default());
        let r = search(&g, "Host-4", 0.6, &SearchOptions::default(), None, None);
        let pos = |name: &str| r.matches.iter().position(|m| m.name == name);
        if let (Some(short), Some(long)) = (pos("Host-42"), pos("Host-4210")) {
            assert!(short < long, "longer name outranked the shorter one");
        }
    }

    /// The whole point of the feature: unrelated nodes are absent, not dimmed.
    #[test]
    fn unrelated_nodes_are_removed() {
        let g = fixture();
        let r = search(&g, "Host-42", 0.6, &SearchOptions::default(), None, None);
        assert!(
            r.visible.len() < g.nodes.len(),
            "search returned the whole graph ({} of {})",
            r.visible.len(),
            g.nodes.len()
        );
        assert!(!r.visible.is_empty());
    }

    /// Breadth is the single knob: turning it up admits more of the neighbourhood.
    #[test]
    fn breadth_widens_the_result() {
        let g = fixture();
        let opts = SearchOptions::default();
        let narrow = search(&g, "Host-42", 0.0, &opts, None, None);
        let wide = search(&g, "Host-42", 0.95, &opts, None, None);
        assert!(
            wide.visible.len() > narrow.visible.len(),
            "breadth did not widen the result: {} vs {}",
            narrow.visible.len(),
            wide.visible.len()
        );
    }

    #[test]
    fn fuzzy_tolerates_a_typo_and_missing_separator() {
        let g = fixture();
        // "Certificate-4" with a character dropped should still find it.
        let r = search(
            &g,
            "Certifcate-4",
            0.6,
            &SearchOptions::default(),
            None,
            None,
        );
        assert!(
            r.matches
                .iter()
                .any(|m| m.name.starts_with("Certificate-4")),
            "fuzzy match failed: {:?}",
            r.matches.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
    }

    /// Properties are searchable even though they never reach the browser — that is precisely
    /// why the search had to move server-side.
    #[test]
    fn matches_against_property_values() {
        let g = fixture();
        // "Synthetic Volume #7 ..." appears only in the description property.
        let r = search(
            &g,
            "Synthetic Volume",
            0.3,
            &SearchOptions::default(),
            None,
            None,
        );
        assert!(!r.matches.is_empty());
        assert!(r.matches.iter().any(|m| m.label == "Volume"));
    }

    #[test]
    fn max_visible_truncates_and_says_so() {
        let g = fixture();
        let opts = SearchOptions {
            max_visible: 5,
            ..SearchOptions::default()
        };
        let r = search(&g, "Host", 1.0, &opts, None, None);
        assert!(r.truncated);
        assert_eq!(r.visible.len(), 5);
    }

    #[test]
    fn semantic_scores_blend_when_embeddings_are_supplied() {
        let g = fixture();
        // Give one node an embedding identical to the query's; it must surface even though its
        // name shares nothing with the query.
        let mut corpus: Vec<Option<Vec<f32>>> = vec![None; g.nodes.len()];
        let target = 100usize;
        corpus[target] = Some(vec![1.0, 0.0, 0.0]);
        let q = [1.0f32, 0.0, 0.0];
        let r = search(
            &g,
            "zzzzzz-no-such-name",
            0.6,
            &SearchOptions::default(),
            Some(&q),
            Some(&corpus),
        );
        assert!(r.semantic);
        assert!(
            r.matches.iter().any(|m| m.id == g.nodes[target].id),
            "semantically-matched node did not surface"
        );
    }
}
