//! A Barnes–Hut quadtree: the structure that takes the repulsive term from O(n²) to O(n log n),
//! which is the difference between a tractable layout at 25 000 nodes and an impossible one.
//!
//! Cells live in a flat `Vec` rather than as boxed nodes. The tree is rebuilt every tick and
//! traversed by every body, so contiguity matters more than elegance here.

const NO_CHILD: u32 = u32::MAX;

/// One quadtree cell. Stored in a flat `Vec` rather than as boxed nodes: the tree is rebuilt
/// every tick and traversed by every node, so contiguity matters more than elegance here.
#[derive(Clone, Copy)]
pub(super) struct Cell {
    cx: f32,
    cy: f32,
    half: f32,
    mass: f32,
    com_x: f32,
    com_y: f32,
    children: [u32; 4],
    /// Index of the single body in a leaf, or `NO_CHILD` once the cell has subdivided.
    body: u32,
}

pub(super) struct QuadTree {
    cells: Vec<Cell>,
}

impl QuadTree {
    pub(super) fn build(pos: &[[f32; 2]]) -> Self {
        // Square root bounds, padded — a body exactly on the boundary must still land inside.
        let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
        let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
        for p in pos {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
        }
        let half = ((max_x - min_x).max(max_y - min_y) * 0.5).max(1.0) * 1.05;
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;

        let mut tree = Self {
            cells: Vec::with_capacity(pos.len() * 2),
        };
        tree.cells.push(Cell {
            cx,
            cy,
            half,
            mass: 0.0,
            com_x: 0.0,
            com_y: 0.0,
            children: [NO_CHILD; 4],
            body: NO_CHILD,
        });
        for (i, p) in pos.iter().enumerate() {
            tree.insert(0, i as u32, p[0], p[1], 0);
        }
        tree.finish(0);
        tree
    }

    #[inline]
    fn quadrant(cell: &Cell, x: f32, y: f32) -> usize {
        usize::from(x >= cell.cx) | (usize::from(y >= cell.cy) << 1)
    }

    fn child_cell(parent: &Cell, q: usize) -> Cell {
        let h = parent.half * 0.5;
        let cx = parent.cx + if q & 1 == 0 { -h } else { h };
        let cy = parent.cy + if q & 2 == 0 { -h } else { h };
        Cell {
            cx,
            cy,
            half: h,
            mass: 0.0,
            com_x: 0.0,
            com_y: 0.0,
            children: [NO_CHILD; 4],
            body: NO_CHILD,
        }
    }

    fn insert(&mut self, mut cell_idx: usize, body: u32, x: f32, y: f32, depth: u32) {
        // Coincident (or near-coincident) points would subdivide forever; past this depth the
        // cell is small enough that treating them as one aggregate is correct anyway.
        if depth > 48 {
            self.cells[cell_idx].mass += 1.0;
            self.cells[cell_idx].com_x += x;
            self.cells[cell_idx].com_y += y;
            return;
        }

        // Empty leaf: park the body here.
        if self.cells[cell_idx].body == NO_CHILD && self.cells[cell_idx].mass == 0.0 {
            self.cells[cell_idx].body = body;
            self.cells[cell_idx].mass = 1.0;
            self.cells[cell_idx].com_x = x;
            self.cells[cell_idx].com_y = y;
            return;
        }

        // Occupied leaf: push the resident down before inserting the newcomer.
        if self.cells[cell_idx].body != NO_CHILD {
            let resident = self.cells[cell_idx].body;
            let rx = self.cells[cell_idx].com_x;
            let ry = self.cells[cell_idx].com_y;
            self.cells[cell_idx].body = NO_CHILD;
            self.cells[cell_idx].mass = 0.0;
            self.cells[cell_idx].com_x = 0.0;
            self.cells[cell_idx].com_y = 0.0;
            self.push_down(cell_idx, resident, rx, ry, depth);
        }

        self.cells[cell_idx].mass += 1.0;
        self.cells[cell_idx].com_x += x;
        self.cells[cell_idx].com_y += y;

        let q = Self::quadrant(&self.cells[cell_idx], x, y);
        let child = self.cells[cell_idx].children[q];
        let child_idx = if child == NO_CHILD {
            let c = Self::child_cell(&self.cells[cell_idx], q);
            self.cells.push(c);
            let idx = (self.cells.len() - 1) as u32;
            self.cells[cell_idx].children[q] = idx;
            idx
        } else {
            child
        };
        cell_idx = child_idx as usize;
        self.insert(cell_idx, body, x, y, depth + 1);
    }

    fn push_down(&mut self, cell_idx: usize, body: u32, x: f32, y: f32, depth: u32) {
        let q = Self::quadrant(&self.cells[cell_idx], x, y);
        let child = self.cells[cell_idx].children[q];
        let child_idx = if child == NO_CHILD {
            let c = Self::child_cell(&self.cells[cell_idx], q);
            self.cells.push(c);
            let idx = (self.cells.len() - 1) as u32;
            self.cells[cell_idx].children[q] = idx;
            idx
        } else {
            child
        };
        self.insert(child_idx as usize, body, x, y, depth + 1);
    }

    /// Turn the accumulated position sums into centres of mass.
    fn finish(&mut self, idx: usize) {
        let m = self.cells[idx].mass;
        if m > 0.0 {
            self.cells[idx].com_x /= m;
            self.cells[idx].com_y /= m;
        }
        let children = self.cells[idx].children;
        for c in children {
            if c != NO_CHILD {
                self.finish(c as usize);
            }
        }
    }

    /// Accumulated repulsive force on a body at `(x, y)`.
    ///
    /// Iterative rather than recursive: at 25k bodies this is the hottest loop in the program,
    /// and an explicit stack avoids both call overhead and any risk of blowing the real one on a
    /// degenerate tree.
    pub(super) fn repulsion(
        &self,
        x: f32,
        y: f32,
        k2: f32,
        theta: f32,
        stack: &mut Vec<u32>,
    ) -> [f32; 2] {
        let mut fx = 0.0;
        let mut fy = 0.0;
        stack.clear();
        stack.push(0);
        while let Some(idx) = stack.pop() {
            let cell = &self.cells[idx as usize];
            if cell.mass == 0.0 {
                continue;
            }
            let dx = x - cell.com_x;
            let dy = y - cell.com_y;
            let dist2 = dx * dx + dy * dy;
            let is_leaf = cell.body != NO_CHILD;

            // Far enough away (or a leaf) to treat as a single aggregate mass.
            if is_leaf || (cell.half * cell.half) < theta * theta * dist2 {
                if dist2 > 1e-9 {
                    // Fruchterman–Reingold repulsion has magnitude k²/d along (dx,dy)/d, so the
                    // vector is (dx,dy) · k²/d² — no square root needed at all.
                    let f = cell.mass * k2 / dist2;
                    fx += dx * f;
                    fy += dy * f;
                }
                continue;
            }
            for c in cell.children {
                if c != NO_CHILD {
                    stack.push(c);
                }
            }
        }
        [fx, fy]
    }
}
