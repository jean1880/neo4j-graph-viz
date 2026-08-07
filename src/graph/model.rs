//! The `{nodes, links}` payload the API serves, and the indices built alongside it.
//!
//! `GraphData` owns two structures the rest of the service depends on and neither endpoint
//! should rebuild: an id index (so a detail lookup is O(1)) and undirected CSR adjacency (so
//! search can expand outward without scanning every link).

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::layout::{self, LayoutOptions};

#[derive(Debug, Serialize, Clone)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub label: String,
    pub group: String,
    pub deg: u32,
    /// Held server-side only. Property bags dominate the payload at scale — 25k nodes of them
    /// is tens of megabytes the browser must `JSON.parse` on the main thread before it can draw
    /// anything, to populate a detail panel that shows one node at a time. They stay in the
    /// cache (search reads them, and `GET /api/node/:id` serves them) but never ship with the
    /// graph. See [`NodeDetail`].
    #[serde(skip_serializing)]
    pub props: BTreeMap<String, String>,
    /// Layout coordinates, computed server-side (see [`crate::layout`]) and used by the client
    /// as the warm start for its own settling pass. `z` is 0 unless the graph was requested in
    /// three dimensions (`?dims=3`).
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Serialize, Clone)]
pub struct GraphLink {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub rel: String,
}

/// One node *with* its properties — the `GET /api/node/:id` payload. Borrows from the cached
/// [`GraphData`] so serving a detail request never clones a property bag.
#[derive(Debug, Serialize)]
pub struct NodeDetail<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub label: &'a str,
    pub group: &'a str,
    pub deg: u32,
    pub props: &'a BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
    /// `id` → index into `nodes`. Built once per fetch so a detail lookup is O(1) instead of a
    /// scan of every node. Server-side accessor, not part of the API — hence `skip`.
    #[serde(skip)]
    index: HashMap<String, usize>,
    /// Undirected adjacency in CSR form (`offsets` into `neighbours`). Search expands outward
    /// from its matches, so it needs neighbours-of-a-node in constant time rather than a scan
    /// of 75 000 links per step.
    #[serde(skip)]
    adj_offsets: Vec<u32>,
    #[serde(skip)]
    adj_neighbours: Vec<u32>,
}

impl GraphData {
    /// Build the payload and its id index together, so the two can never drift apart.
    pub fn new(nodes: Vec<GraphNode>, links: Vec<GraphLink>) -> Self {
        let index: HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();

        // CSR build: count degrees, prefix-sum into offsets, then fill.
        let n = nodes.len();
        let mut degree = vec![0u32; n];
        let ends: Vec<(usize, usize)> = links
            .iter()
            .filter_map(|l| Some((*index.get(&l.source)?, *index.get(&l.target)?)))
            .filter(|(s, t)| s != t)
            .collect();
        for &(s, t) in &ends {
            degree[s] += 1;
            degree[t] += 1;
        }
        let mut adj_offsets = Vec::with_capacity(n + 1);
        let mut running = 0u32;
        for d in &degree {
            adj_offsets.push(running);
            running += d;
        }
        adj_offsets.push(running);
        let mut cursor = adj_offsets.clone();
        let mut adj_neighbours = vec![0u32; running as usize];
        for &(s, t) in &ends {
            adj_neighbours[cursor[s] as usize] = t as u32;
            cursor[s] += 1;
            adj_neighbours[cursor[t] as usize] = s as u32;
            cursor[t] += 1;
        }

        Self {
            nodes,
            links,
            index,
            adj_offsets,
            adj_neighbours,
        }
    }

    /// Index of `id`, if the graph holds it.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// Neighbouring node indices of `i`, in constant time.
    pub fn neighbours(&self, i: usize) -> &[u32] {
        let lo = self.adj_offsets[i] as usize;
        let hi = self.adj_offsets[i + 1] as usize;
        &self.adj_neighbours[lo..hi]
    }

    /// Edge index pairs, for laying out a subgraph.
    pub fn edge_pairs(&self) -> Vec<(u32, u32)> {
        self.links
            .iter()
            .filter_map(|l| {
                Some((
                    *self.index.get(&l.source)? as u32,
                    *self.index.get(&l.target)? as u32,
                ))
            })
            .collect()
    }

    /// Run the force layout and stamp the resulting coordinates onto every node.
    ///
    /// Separate from [`GraphData::new`] because it is the expensive step — seconds, not
    /// milliseconds, at 25k nodes — and callers need to control when they pay for it.
    pub fn apply_layout(&mut self, opts: &LayoutOptions, dimensions: u8) {
        let edges: Vec<(u32, u32)> = self
            .links
            .iter()
            .filter_map(|l| {
                Some((
                    *self.index.get(&l.source)? as u32,
                    *self.index.get(&l.target)? as u32,
                ))
            })
            .collect();

        let started = std::time::Instant::now();
        let n = self.nodes.len();
        let positions: Vec<[f32; 3]> = if dimensions == 3 {
            layout::compute_3d(n, &edges, opts)
        } else {
            layout::compute(n, &edges, opts)
                .into_iter()
                .map(|[x, y]| [x, y, 0.0])
                .collect()
        };
        for (node, p) in self.nodes.iter_mut().zip(positions) {
            node.x = p[0];
            node.y = p[1];
            node.z = p[2];
        }
        tracing::info!(
            nodes = self.nodes.len(),
            edges = edges.len(),
            dimensions,
            iterations = opts.iterations,
            elapsed_ms = started.elapsed().as_millis(),
            "layout computed"
        );
    }

    /// The node with `id`, plus its properties, or `None` when the graph does not hold it.
    pub fn detail(&self, id: &str) -> Option<NodeDetail<'_>> {
        let n = self.index.get(id).and_then(|&i| self.nodes.get(i))?;
        Some(NodeDetail {
            id: &n.id,
            name: &n.name,
            label: &n.label,
            group: &n.group,
            deg: n.deg,
            props: &n.props,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixture;
    use super::super::options::FetchOptions;

    #[test]
    fn props_are_withheld_from_the_graph_payload_but_served_by_detail() {
        let data = fixture(8, 2, 7, &FetchOptions::default());

        let graph_json = serde_json::to_value(&data).expect("graph serializes");
        let first = &graph_json["nodes"][0];
        assert!(
            first.get("props").is_none(),
            "props leaked into the /api/graph payload: {first}"
        );
        // The fields the renderer actually needs are still there.
        for key in ["id", "name", "label", "group", "deg"] {
            assert!(first.get(key).is_some(), "{key} missing from graph payload");
        }
        // The index is a server-side accessor, not part of the API surface.
        assert!(graph_json.get("index").is_none());

        let id = data.nodes[0].id.clone();
        let detail = data.detail(&id).expect("node is in the graph");
        let detail_json = serde_json::to_value(&detail).expect("detail serializes");
        assert_eq!(detail_json["id"], serde_json::json!(id));
        assert!(
            detail_json["props"].get("description").is_some(),
            "detail must carry props: {detail_json}"
        );

        assert!(data.detail("fixture:nope").is_none());
    }
}
