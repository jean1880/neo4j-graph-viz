//! A Barnes–Hut **octree** — the 3D counterpart to [`super::quadtree`].
//!
//! Same idea, one more axis: eight children per cell instead of four, and the opening-angle test
//! compares the cell's half-width against a distance in three dimensions. That is what keeps a 3D
//! layout O(n log n) rather than O(n²), which at 25 000 nodes is the difference between a
//! sub-second pass and one nobody would wait for.
//!
//! Cells live in a flat `Vec` for the same reason as in 2D: the tree is rebuilt every tick and
//! traversed by every body, so contiguity matters more than elegance.

const NO_CHILD: u32 = u32::MAX;

#[derive(Clone, Copy)]
pub(super) struct Cell {
    cx: f32,
    cy: f32,
    cz: f32,
    half: f32,
    mass: f32,
    com_x: f32,
    com_y: f32,
    com_z: f32,
    children: [u32; 8],
    /// Index of the single body in a leaf, or `NO_CHILD` once the cell has subdivided.
    body: u32,
}

pub(super) struct OctTree {
    cells: Vec<Cell>,
}

impl OctTree {
    pub(super) fn build(pos: &[[f32; 3]]) -> Self {
        let (mut min_x, mut min_y, mut min_z) = (f32::MAX, f32::MAX, f32::MAX);
        let (mut max_x, mut max_y, mut max_z) = (f32::MIN, f32::MIN, f32::MIN);
        for p in pos {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            min_z = min_z.min(p[2]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
            max_z = max_z.max(p[2]);
        }
        // Cube bounds, padded — a body exactly on the boundary must still land inside.
        let half = ((max_x - min_x).max(max_y - min_y).max(max_z - min_z) * 0.5).max(1.0) * 1.05;

        let mut tree = Self {
            cells: Vec::with_capacity(pos.len() * 2),
        };
        tree.cells.push(Cell {
            cx: (min_x + max_x) * 0.5,
            cy: (min_y + max_y) * 0.5,
            cz: (min_z + max_z) * 0.5,
            half,
            mass: 0.0,
            com_x: 0.0,
            com_y: 0.0,
            com_z: 0.0,
            children: [NO_CHILD; 8],
            body: NO_CHILD,
        });
        for (i, p) in pos.iter().enumerate() {
            tree.insert(0, i as u32, p[0], p[1], p[2], 0);
        }
        tree.finish(0);
        tree
    }

    #[inline]
    fn octant(cell: &Cell, x: f32, y: f32, z: f32) -> usize {
        usize::from(x >= cell.cx)
            | (usize::from(y >= cell.cy) << 1)
            | (usize::from(z >= cell.cz) << 2)
    }

    fn child_cell(parent: &Cell, q: usize) -> Cell {
        let h = parent.half * 0.5;
        Cell {
            cx: parent.cx + if q & 1 == 0 { -h } else { h },
            cy: parent.cy + if q & 2 == 0 { -h } else { h },
            cz: parent.cz + if q & 4 == 0 { -h } else { h },
            half: h,
            mass: 0.0,
            com_x: 0.0,
            com_y: 0.0,
            com_z: 0.0,
            children: [NO_CHILD; 8],
            body: NO_CHILD,
        }
    }

    fn child_index(&mut self, cell_idx: usize, q: usize) -> u32 {
        let existing = self.cells[cell_idx].children[q];
        if existing != NO_CHILD {
            return existing;
        }
        let c = Self::child_cell(&self.cells[cell_idx], q);
        self.cells.push(c);
        let idx = (self.cells.len() - 1) as u32;
        self.cells[cell_idx].children[q] = idx;
        idx
    }

    fn insert(&mut self, cell_idx: usize, body: u32, x: f32, y: f32, z: f32, depth: u32) {
        // Coincident points would subdivide forever; past this depth the cell is small enough
        // that treating them as one aggregate is correct anyway.
        if depth > 48 {
            self.cells[cell_idx].mass += 1.0;
            self.cells[cell_idx].com_x += x;
            self.cells[cell_idx].com_y += y;
            self.cells[cell_idx].com_z += z;
            return;
        }

        // Empty leaf: park the body here.
        if self.cells[cell_idx].body == NO_CHILD && self.cells[cell_idx].mass == 0.0 {
            self.cells[cell_idx].body = body;
            self.cells[cell_idx].mass = 1.0;
            self.cells[cell_idx].com_x = x;
            self.cells[cell_idx].com_y = y;
            self.cells[cell_idx].com_z = z;
            return;
        }

        // Occupied leaf: push the resident down before inserting the newcomer.
        if self.cells[cell_idx].body != NO_CHILD {
            let resident = self.cells[cell_idx].body;
            let (rx, ry, rz) = (
                self.cells[cell_idx].com_x,
                self.cells[cell_idx].com_y,
                self.cells[cell_idx].com_z,
            );
            self.cells[cell_idx].body = NO_CHILD;
            self.cells[cell_idx].mass = 0.0;
            self.cells[cell_idx].com_x = 0.0;
            self.cells[cell_idx].com_y = 0.0;
            self.cells[cell_idx].com_z = 0.0;
            let q = Self::octant(&self.cells[cell_idx], rx, ry, rz);
            let child = self.child_index(cell_idx, q) as usize;
            self.insert(child, resident, rx, ry, rz, depth + 1);
        }

        self.cells[cell_idx].mass += 1.0;
        self.cells[cell_idx].com_x += x;
        self.cells[cell_idx].com_y += y;
        self.cells[cell_idx].com_z += z;

        let q = Self::octant(&self.cells[cell_idx], x, y, z);
        let child = self.child_index(cell_idx, q) as usize;
        self.insert(child, body, x, y, z, depth + 1);
    }

    /// Turn the accumulated position sums into centres of mass.
    fn finish(&mut self, idx: usize) {
        let m = self.cells[idx].mass;
        if m > 0.0 {
            self.cells[idx].com_x /= m;
            self.cells[idx].com_y /= m;
            self.cells[idx].com_z /= m;
        }
        let children = self.cells[idx].children;
        for c in children {
            if c != NO_CHILD {
                self.finish(c as usize);
            }
        }
    }

    /// Accumulated repulsive force on a body at `(x, y, z)`.
    ///
    /// Iterative rather than recursive: this is the hottest loop in the program, and an explicit
    /// stack avoids both call overhead and any risk of blowing the real one on a degenerate tree.
    pub(super) fn repulsion(
        &self,
        x: f32,
        y: f32,
        z: f32,
        k2: f32,
        theta: f32,
        stack: &mut Vec<u32>,
    ) -> [f32; 3] {
        let mut f = [0.0f32; 3];
        stack.clear();
        stack.push(0);
        while let Some(idx) = stack.pop() {
            let cell = &self.cells[idx as usize];
            if cell.mass == 0.0 {
                continue;
            }
            let dx = x - cell.com_x;
            let dy = y - cell.com_y;
            let dz = z - cell.com_z;
            let dist2 = dx * dx + dy * dy + dz * dz;
            let is_leaf = cell.body != NO_CHILD;

            // Far enough away (or a leaf) to treat as a single aggregate mass.
            if is_leaf || (cell.half * cell.half) < theta * theta * dist2 {
                if dist2 > 1e-9 {
                    // Fruchterman–Reingold repulsion: magnitude k²/d along (dx,dy,dz)/d, so the
                    // vector is (dx,dy,dz) · k²/d² — no square root needed.
                    let s = cell.mass * k2 / dist2;
                    f[0] += dx * s;
                    f[1] += dy * s;
                    f[2] += dz * s;
                }
                continue;
            }
            for c in cell.children {
                if c != NO_CHILD {
                    stack.push(c);
                }
            }
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Coincident points are the classic infinite-subdivision bug, in 3D as in 2D.
    #[test]
    fn survives_coincident_points() {
        let tree = OctTree::build(&[[0.0; 3], [0.0; 3], [0.0; 3], [0.0; 3]]);
        let mut stack = Vec::new();
        let f = tree.repulsion(0.0, 0.0, 0.0, 1.0, 0.9, &mut stack);
        assert!(f.iter().all(|v| v.is_finite()));
    }

    /// The approximation must agree with brute force on the direction of the force, or the
    /// opening-angle test is wrong and the layout will be subtly, unfalsifiably bad.
    #[test]
    fn approximation_agrees_with_brute_force_direction() {
        let bodies: Vec<[f32; 3]> = (0..200)
            .map(|i| {
                let f = i as f32;
                [f.sin() * 100.0, f.cos() * 100.0, (f * 0.7).sin() * 100.0]
            })
            .collect();
        let tree = OctTree::build(&bodies);
        let mut stack = Vec::new();
        let probe = [250.0f32, 40.0, -30.0];
        let approx = tree.repulsion(probe[0], probe[1], probe[2], 1.0, 0.5, &mut stack);

        let mut exact = [0.0f32; 3];
        for b in &bodies {
            let d = [probe[0] - b[0], probe[1] - b[1], probe[2] - b[2]];
            let dist2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if dist2 > 1e-9 {
                for a in 0..3 {
                    exact[a] += d[a] / dist2;
                }
            }
        }

        let dot: f32 = (0..3).map(|a| approx[a] * exact[a]).sum();
        let na = approx.iter().map(|v| v * v).sum::<f32>().sqrt();
        let ne = exact.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(na > 0.0 && ne > 0.0);
        assert!(
            dot / (na * ne) > 0.99,
            "Barnes-Hut direction diverged from brute force (cos = {})",
            dot / (na * ne)
        );
    }
}
