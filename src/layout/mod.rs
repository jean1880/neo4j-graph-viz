//! Force-directed layout, computed server-side.
//!
//! The browser used to run this simulation itself, on the main thread, on every page load: at
//! 25k nodes that meant ~8 fps for the fifteen seconds it took to cool, in every tab, every
//! time. Here it runs once per cache fill, natively, across every core — and the client renders
//! an already-settled graph.
//!
//! The algorithm is Fruchterman–Reingold with a Barnes–Hut quadtree for the repulsive term,
//! which is what takes it from O(n²) to O(n log n) — the difference between tractable and not at
//! this size.
//!
//! **Determinism is a requirement, not a nicety.** The same graph must always lay out the same
//! way, or a refresh reshuffles the map and destroys the spatial memory that makes it useful.
//! That constrains the parallelism: every tick is a *gather* (each node computes its own force
//! by reading shared state) rather than a *scatter* (nodes accumulating into each other), so no
//! result depends on thread scheduling or float-accumulation order.

use rayon::prelude::*;

/// Layout tuning. All of it is env-driven at the call site so a different graph shape can be
/// dialled in without a rebuild.
#[derive(Debug, Clone, Copy)]
pub struct LayoutOptions {
    /// Simulation ticks. More is tighter and slower; quality plateaus well before 1000.
    pub iterations: usize,
    /// Barnes–Hut opening angle. 0 is exact O(n²); higher is faster and coarser. 0.9 is a
    /// standard quality/speed compromise.
    pub theta: f32,
    /// Edge length scale. Larger spreads the graph out.
    pub scale: f32,
    /// Pull toward the origin, which is the only thing keeping disconnected components from
    /// drifting apart forever — a real graph is rarely one connected component.
    pub gravity: f32,
    /// Seeds the initial positions. Fixed by default so layouts reproduce exactly.
    pub seed: u64,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            iterations: 300,
            theta: 0.9,
            scale: 1.0,
            gravity: 0.02,
            seed: 1,
        }
    }
}

/// Same xorshift64* as the fixture generator: deterministic, and no dependency for what is
/// only ever used to jitter starting positions.
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

    /// Uniform in `-1.0..1.0`.
    fn signed_unit(&mut self) -> f32 {
        (self.next() >> 11) as f32 / (1u64 << 52) as f32 * 2.0 - 1.0
    }
}

/// Compressed sparse row adjacency. Built once and reused across every tick, so the attractive
/// term is a contiguous read per node rather than a scan of the whole edge list.
struct Csr {
    offsets: Vec<u32>,
    neighbours: Vec<u32>,
}

impl Csr {
    fn build(n: usize, edges: &[(u32, u32)]) -> Self {
        let mut degree = vec![0u32; n];
        for &(s, t) in edges {
            if (s as usize) < n && (t as usize) < n && s != t {
                degree[s as usize] += 1;
                degree[t as usize] += 1;
            }
        }
        let mut offsets = Vec::with_capacity(n + 1);
        let mut running = 0u32;
        for d in &degree {
            offsets.push(running);
            running += d;
        }
        offsets.push(running);

        let mut cursor = offsets.clone();
        let mut neighbours = vec![0u32; running as usize];
        for &(s, t) in edges {
            if (s as usize) < n && (t as usize) < n && s != t {
                neighbours[cursor[s as usize] as usize] = t;
                cursor[s as usize] += 1;
                neighbours[cursor[t as usize] as usize] = s;
                cursor[t as usize] += 1;
            }
        }
        Self {
            offsets,
            neighbours,
        }
    }

    #[inline]
    fn of(&self, i: usize) -> &[u32] {
        let lo = self.offsets[i] as usize;
        let hi = self.offsets[i + 1] as usize;
        &self.neighbours[lo..hi]
    }
}

mod octree;
mod quadtree;

use octree::OctTree;
use quadtree::QuadTree;

/// Lay the graph out and return one `[x, y]` per node, in node-index order.
///
/// `edges` are index pairs into the node array; out-of-range pairs and self-loops are ignored
/// rather than treated as an error, so a truncated fetch still lays out.
pub fn compute(n: usize, edges: &[(u32, u32)], opts: &LayoutOptions) -> Vec<[f32; 2]> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![[0.0, 0.0]];
    }

    let csr = Csr::build(n, edges);

    // Ideal edge length. The area is nominal — only the ratio matters, since the client
    // zoom-to-fits whatever comes back.
    let area = 1_000_000.0f32 * opts.scale * opts.scale;
    let k = (area / n as f32).sqrt();
    let k2 = k * k;

    // Seeded starting positions on a disc. A deterministic start is what makes the whole layout
    // reproducible; the radius keeps early repulsion from exploding.
    let mut rng = Rng(opts.seed | 1);
    let radius = k * (n as f32).sqrt() * 0.5;
    let mut pos: Vec<[f32; 2]> = (0..n)
        .map(|_| [rng.signed_unit() * radius, rng.signed_unit() * radius])
        .collect();

    let mut temp = radius * 0.1;
    let cooling = temp / (opts.iterations.max(1) as f32);

    for _ in 0..opts.iterations {
        let tree = QuadTree::build(&pos);

        // Each node computes its own displacement by *reading* shared state. Nothing is written
        // across nodes, so the result does not depend on how rayon schedules the work.
        let next: Vec<[f32; 2]> = (0..n)
            .into_par_iter()
            .map_init(
                || Vec::with_capacity(64),
                |stack, i| {
                    let [x, y] = pos[i];
                    let [mut dx, mut dy] = tree.repulsion(x, y, k2, opts.theta, stack);

                    // Attraction along incident edges: d²/k toward each neighbour.
                    for &j in csr.of(i) {
                        let [nx, ny] = pos[j as usize];
                        let ex = x - nx;
                        let ey = y - ny;
                        let d2 = ex * ex + ey * ey;
                        if d2 > 1e-9 {
                            let d = d2.sqrt();
                            let f = d2 / k;
                            dx -= ex / d * f;
                            dy -= ey / d * f;
                        }
                    }

                    // Gravity toward the origin keeps disconnected components from escaping.
                    dx -= x * opts.gravity * k;
                    dy -= y * opts.gravity * k;

                    // Cap the step at the current temperature — this is what makes the system
                    // settle rather than oscillate.
                    let disp = (dx * dx + dy * dy).sqrt();
                    if disp > 1e-9 {
                        let limited = disp.min(temp) / disp;
                        [x + dx * limited, y + dy * limited]
                    } else {
                        [x, y]
                    }
                },
            )
            .collect();

        pos = next;
        temp = (temp - cooling).max(0.0);
    }

    pos
}

/// Lay the graph out in **three** dimensions, returning one `[x, y, z]` per node.
///
/// Structurally identical to [`compute`] — same Fruchterman–Reingold forces, same cooling, same
/// gather-shaped parallelism so the result stays deterministic — with an octree in place of the
/// quadtree. Kept as a sibling rather than folded into one generic routine: the inner loops are
/// the hot path, and a dimension-generic version costs more in indirection than it saves in
/// duplication.
pub fn compute_3d(n: usize, edges: &[(u32, u32)], opts: &LayoutOptions) -> Vec<[f32; 3]> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![[0.0, 0.0, 0.0]];
    }

    let csr = Csr::build(n, edges);

    // Volume rather than area: spreading the same node count through three dimensions needs a
    // cube-root relationship for the ideal edge length, or the graph comes out far too dense.
    let volume = 1_000_000_000.0f32 * opts.scale.powi(3);
    let k = (volume / n as f32).cbrt();
    let k2 = k * k;

    let mut rng = Rng(opts.seed | 1);
    let radius = k * (n as f32).cbrt() * 0.5;
    let mut pos: Vec<[f32; 3]> = (0..n)
        .map(|_| {
            [
                rng.signed_unit() * radius,
                rng.signed_unit() * radius,
                rng.signed_unit() * radius,
            ]
        })
        .collect();

    let mut temp = radius * 0.1;
    let cooling = temp / (opts.iterations.max(1) as f32);

    for _ in 0..opts.iterations {
        let tree = OctTree::build(&pos);

        let next: Vec<[f32; 3]> = (0..n)
            .into_par_iter()
            .map_init(
                || Vec::with_capacity(64),
                |stack, i| {
                    let [x, y, z] = pos[i];
                    let mut d = tree.repulsion(x, y, z, k2, opts.theta, stack);

                    for &j in csr.of(i) {
                        let [nx, ny, nz] = pos[j as usize];
                        let e = [x - nx, y - ny, z - nz];
                        let d2 = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
                        if d2 > 1e-9 {
                            let dist = d2.sqrt();
                            let f = d2 / k;
                            for a in 0..3 {
                                d[a] -= e[a] / dist * f;
                            }
                        }
                    }

                    d[0] -= x * opts.gravity * k;
                    d[1] -= y * opts.gravity * k;
                    d[2] -= z * opts.gravity * k;

                    let disp = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if disp > 1e-9 {
                        let limited = disp.min(temp) / disp;
                        [x + d[0] * limited, y + d[1] * limited, z + d[2] * limited]
                    } else {
                        [x, y, z]
                    }
                },
            )
            .collect();

        pos = next;
        temp = (temp - cooling).max(0.0);
    }

    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(iterations: usize) -> LayoutOptions {
        LayoutOptions {
            iterations,
            ..LayoutOptions::default()
        }
    }

    /// A refresh must not reshuffle the map — the user's spatial memory of where things are is
    /// most of what makes a persistent graph view useful.
    #[test]
    fn layout_is_deterministic() {
        let edges: Vec<(u32, u32)> = (1..60).map(|i| (i, i / 2)).collect();
        let a = compute(60, &edges, &opts(40));
        let b = compute(60, &edges, &opts(40));
        assert_eq!(a, b);

        // ...but the seed must actually do something, or "deterministic" just means "constant".
        let c = compute(
            60,
            &edges,
            &LayoutOptions {
                iterations: 40,
                seed: 12345,
                ..LayoutOptions::default()
            },
        );
        assert_ne!(a, c);
    }

    #[test]
    fn layout_separates_connected_nodes_and_stays_finite() {
        let edges: Vec<(u32, u32)> = (1..200).map(|i| (i, i / 2)).collect();
        let pos = compute(200, &edges, &opts(120));
        assert_eq!(pos.len(), 200);
        for p in &pos {
            assert!(
                p[0].is_finite() && p[1].is_finite(),
                "non-finite position {p:?}"
            );
        }
        // No two connected nodes should have collapsed onto each other.
        for &(s, t) in &edges {
            let a = pos[s as usize];
            let b = pos[t as usize];
            let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
            assert!(d > 1.0, "nodes {s} and {t} collapsed (distance {d})");
        }
        // The graph must actually occupy space rather than converging to a point.
        let span_x = pos.iter().map(|p| p[0]).fold(f32::MIN, f32::max)
            - pos.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        assert!(span_x > 10.0, "layout collapsed: x span {span_x}");
    }

    #[test]
    fn layout_tolerates_degenerate_input() {
        assert!(compute(0, &[], &opts(10)).is_empty());
        assert_eq!(compute(1, &[], &opts(10)), vec![[0.0, 0.0]]);
        // No edges at all — pure repulsion plus gravity must still terminate and stay finite.
        let pos = compute(50, &[], &opts(20));
        assert!(pos.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
        // Self-loops and out-of-range endpoints are ignored, not fatal.
        let pos = compute(10, &[(0, 0), (99, 1), (1, 2)], &opts(20));
        assert_eq!(pos.len(), 10);
        assert!(pos.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
    }

    #[test]
    fn layout_3d_is_deterministic_and_uses_all_three_axes() {
        let edges: Vec<(u32, u32)> = (1..200).map(|i| (i, i / 2)).collect();
        let a = compute_3d(200, &edges, &opts(60));
        let b = compute_3d(200, &edges, &opts(60));
        assert_eq!(a, b, "same seed must produce the same 3D layout");
        assert_eq!(a.len(), 200);
        assert!(a.iter().all(|p| p.iter().all(|v| v.is_finite())));

        // Every axis must actually be used — a 3D layout that collapses onto a plane is just a
        // slower 2D one, and the failure would be invisible from the numbers alone.
        for axis in 0..3 {
            let lo = a.iter().map(|p| p[axis]).fold(f32::MAX, f32::min);
            let hi = a.iter().map(|p| p[axis]).fold(f32::MIN, f32::max);
            assert!(hi - lo > 10.0, "axis {axis} collapsed (span {})", hi - lo);
        }
    }

    #[test]
    fn layout_3d_tolerates_degenerate_input() {
        assert!(compute_3d(0, &[], &opts(10)).is_empty());
        assert_eq!(compute_3d(1, &[], &opts(10)), vec![[0.0, 0.0, 0.0]]);
        let pos = compute_3d(40, &[(0, 0), (99, 1), (1, 2)], &opts(20));
        assert_eq!(pos.len(), 40);
        assert!(pos.iter().all(|p| p.iter().all(|v| v.is_finite())));
    }

    /// Coincident points are the classic quadtree infinite-subdivision bug.
    #[test]
    fn layout_survives_coincident_starting_points() {
        let tree = QuadTree::build(&[[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]);
        let mut stack = Vec::new();
        let f = tree.repulsion(0.0, 0.0, 1.0, 0.9, &mut stack);
        assert!(f[0].is_finite() && f[1].is_finite());
    }
}
