//! A synthetic graph generator, for benchmarking and CI.
//!
//! Exists so performance work can be measured against a workload that is reproducible and needs
//! no database: a benchmark that depends on the live graph's current contents cannot be compared
//! against itself a week later. Selecting it (`GRAPH_FIXTURE_NODES`) bypasses Neo4j entirely, so
//! no endpoint and no credential are required.

use std::collections::BTreeMap;

use serde_json::Value;

use super::model::{GraphData, GraphLink, GraphNode};
use super::options::FetchOptions;
use super::shape::stringify;

/// Deterministic xorshift64* PRNG. Hand-rolled rather than pulling in `rand`, because the only
/// consumer is the benchmark fixture and determinism is the point: the same `seed` and size must
/// always produce the same graph, or a before/after benchmark compares two different workloads.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..n`. `n` must be non-zero.
    fn below(&mut self, n: usize) -> usize {
        // Callers currently satisfy this by construction (the draw pool is seeded non-empty and
        // only grows). Assert it so a future change fails loudly instead of dividing by zero.
        debug_assert!(n != 0, "below(0) would divide by zero");
        (self.next() % n as u64) as usize
    }
}

/// Labels the fixture cycles through, so the legend, the colour hash, and label-based filtering
/// all get something realistic to chew on.
const FIXTURE_LABELS: [&str; 8] = [
    "Service",
    "Container",
    "Host",
    "Package",
    "Certificate",
    "Skill",
    "Volume",
    "DomainName",
];

/// Build a synthetic graph of `n` nodes via preferential attachment (Barabási–Albert).
///
/// Benchmarks need a workload with the *shape* of a real graph, not a uniform random one: real
/// topologies are hub-heavy, and hubs are what stress both the layout (they dominate the force
/// field) and the renderer (they own most of the edges). Uniform random edges would understate
/// both. `edges_per_node` sets the mean degree; the tail emerges from the attachment rule.
///
/// Deterministic given `seed`, and it never touches Neo4j — so CI can exercise the whole
/// pipeline, at any scale, with no database and no credentials.
///
/// **The `GRAPH_MAX_NODES` / `GRAPH_MAX_RELS` caps do not apply here.** Those exist to bound an
/// unbounded scan of a real database; a fixture is sized explicitly by its own parameters, and
/// silently truncating it to a cap would make a benchmark quietly measure a smaller graph than
/// the one it claims to. `n * edges_per_node` is the link count you get.
pub fn fixture(n: usize, edges_per_node: usize, seed: u64, opts: &FetchOptions) -> GraphData {
    let n = n.max(1);
    let m = edges_per_node.clamp(1, n);
    let mut rng = Rng(seed | 1); // xorshift degenerates from a zero state
    let mut nodes: Vec<GraphNode> = Vec::with_capacity(n);

    for i in 0..n {
        let label = FIXTURE_LABELS[i % FIXTURE_LABELS.len()];
        let mut props = BTreeMap::new();
        props.insert("name".to_string(), format!("{label}-{i}"));
        // Padded to roughly the size of a real property bag, so payload measurements are honest
        // rather than flattering.
        props.insert(
            "description".to_string(),
            stringify(
                &Value::String(format!(
                    "Synthetic {label} #{i} generated for benchmarking. \
                     This padding approximates a realistic property payload."
                )),
                opts.max_prop_chars,
            ),
        );
        props.insert("index".to_string(), i.to_string());
        nodes.push(GraphNode {
            id: format!("fixture:{i}"),
            name: format!("{label}-{i}"),
            label: label.to_string(),
            group: String::new(),
            deg: 0,
            props,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
    }

    // Preferential attachment: `targets` holds each node once per incident edge, so drawing from
    // it uniformly is drawing proportional to degree. That is what grows the hubs.
    let mut links: Vec<GraphLink> = Vec::with_capacity(n * m);
    let mut targets: Vec<usize> = vec![0];
    for i in 1..n {
        let mut chosen: Vec<usize> = Vec::with_capacity(m);
        for _ in 0..m.min(i) {
            // Retry a few times to avoid duplicate edges without an O(n) membership structure;
            // a collision just costs one extra draw, and giving up keeps this O(1) per edge.
            let mut pick = targets[rng.below(targets.len())];
            for _ in 0..4 {
                if !chosen.contains(&pick) {
                    break;
                }
                pick = targets[rng.below(targets.len())];
            }
            if chosen.contains(&pick) {
                continue;
            }
            chosen.push(pick);
        }
        for &t in &chosen {
            nodes[i].deg += 1;
            nodes[t].deg += 1;
            links.push(GraphLink {
                source: nodes[i].id.clone(),
                target: nodes[t].id.clone(),
                rel: "LINKS_TO".to_string(),
            });
            targets.push(i);
            targets.push(t);
        }
    }

    tracing::info!(
        nodes = nodes.len(),
        links = links.len(),
        "serving synthetic fixture graph — Neo4j is not being queried"
    );
    GraphData::new(nodes, links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_deterministic_and_hub_heavy() {
        let opts = FetchOptions::default();
        let a = fixture(500, 3, 42, &opts);
        let b = fixture(500, 3, 42, &opts);
        assert_eq!(a.nodes.len(), 500);
        // Compare the *targets*: a link's source is fixed by construction (node `i` emits its
        // own edges), so only the target sequence reflects the RNG at all.
        let targets = |g: &GraphData| g.links.iter().map(|l| l.target.clone()).collect::<Vec<_>>();
        assert_eq!(
            targets(&a),
            targets(&b),
            "same seed must produce the same graph"
        );
        // A different seed must actually diverge, or the seed is not wired through.
        let c = fixture(500, 3, 99, &opts);
        assert_ne!(targets(&a), targets(&c));

        // Preferential attachment must produce a heavy tail — a uniform-random graph would not
        // stress the layout or the renderer the way a real topology does.
        let max_deg = a.nodes.iter().map(|n| n.deg).max().unwrap_or(0);
        let mean_deg = a.nodes.iter().map(|n| n.deg as f64).sum::<f64>() / a.nodes.len() as f64;
        assert!(
            f64::from(max_deg) > mean_deg * 5.0,
            "expected hubs: max degree {max_deg} vs mean {mean_deg:.1}"
        );

        // Degree must match the links actually emitted, or the renderer sizes nodes from a lie.
        let total_deg: u32 = a.nodes.iter().map(|n| n.deg).sum();
        assert_eq!(total_deg as usize, a.links.len() * 2);
    }

    #[test]
    fn fixture_tolerates_degenerate_sizes() {
        let opts = FetchOptions::default();
        // A zero seed must not lock the xorshift state at zero.
        let z = fixture(50, 2, 0, &opts);
        assert!(z.links.iter().any(|l| l.source != z.links[0].source));
        // Sizes below the edge budget must not panic or over-link.
        assert_eq!(fixture(1, 5, 1, &opts).nodes.len(), 1);
        assert!(fixture(1, 5, 1, &opts).links.is_empty());
        assert_eq!(fixture(0, 3, 1, &opts).nodes.len(), 1);
    }
}
