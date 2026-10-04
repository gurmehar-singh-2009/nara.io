use std::{cmp::Reverse, collections::BinaryHeap};

use glam::Vec2;

use crate::{
    entities::{entity::EntityId, shape::ShapeKind},
    fs::load_config::Config,
    scripting::scripting::Scripting,
};

pub(crate) const NAV_CELLS: usize = 64;
pub const NAV_LEN: usize = NAV_CELLS * NAV_CELLS;
pub(crate) const NAV_BLOCK_DANGER: f32 = 40.0;
pub const NAV_REBUILD_INTERVAL: u64 = 10;
const DSTAR_EXPANSION_BUDGET: usize = 2048;

#[derive(Clone, Copy, PartialEq)]
struct DStarNode {
    k1: f32,
    k2: f32,
    idx: usize,
}
impl Eq for DStarNode {}
impl Ord for DStarNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.k1
            .partial_cmp(&other.k1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                self.k2
                    .partial_cmp(&other.k2)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| self.idx.cmp(&other.idx))
    }
}
impl PartialOrd for DStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn nav_neighbors(cell: usize) -> ([(usize, bool); 8], usize) {
    let mut out: [(usize, bool); 8] = [(0, false); 8];
    let mut n = 0usize;
    let cx = (cell % NAV_CELLS) as i32;
    let cy = (cell / NAV_CELLS) as i32;

    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            if dx == 0 && dy == 0 {
                continue;
            }

            let nx = cx + dx;
            let ny = cy + dy;

            if nx < 0 || ny < 0 || nx >= NAV_CELLS as i32 || ny >= NAV_CELLS as i32 {
                continue;
            }

            out[n] = ((ny as usize) * NAV_CELLS + nx as usize, dx != 0 && dy != 0);

            n += 1;
        }
    }

    (out, n)
}

pub struct DStarLite {
    known: Vec<f32>,
    g: Vec<f32>,
    rhs: Vec<f32>,

    in_queue: Vec<Option<(f32, f32)>>,
    heap: BinaryHeap<Reverse<DStarNode>>,
    k_m: f32,
    start: usize,
    s_last: usize,
    goal: Option<usize>,
}

impl DStarLite {
    pub fn new(grid: &[f32]) -> Self {
        Self {
            known: grid.to_vec(),
            g: vec![f32::INFINITY; NAV_LEN],
            rhs: vec![f32::INFINITY; NAV_LEN],
            in_queue: vec![None; NAV_LEN],
            heap: BinaryHeap::new(),
            k_m: 0.0,
            start: 0,
            s_last: 0,
            goal: None,
        }
    }

    pub fn goal(&self) -> Option<usize> {
        self.goal
    }

    fn cell_cost(&self, cell: usize) -> f32 {
        if self.known[cell] >= NAV_BLOCK_DANGER {
            f32::INFINITY
        } else {
            1.0 + self.known[cell]
        }
    }

    fn edge_cost(&self, from: usize, to: usize, diag: bool) -> f32 {
        let c = self.cell_cost(to);

        if !c.is_finite() {
            return f32::INFINITY;
        }

        if diag {
            let fx = from % NAV_CELLS;
            let fy = from / NAV_CELLS;
            let tx = to % NAV_CELLS;
            let ty = to / NAV_CELLS;
            let a = fy * NAV_CELLS + tx;
            let b = ty * NAV_CELLS + fx;

            if self.cell_cost(a).is_infinite() || self.cell_cost(b).is_infinite() {
                return f32::INFINITY;
            }

            std::f32::consts::SQRT_2 * c
        } else {
            c
        }
    }

    fn heuristic(&self, a: usize, b: usize) -> f32 {
        let ax = (a % NAV_CELLS) as i32;
        let ay = (a / NAV_CELLS) as i32;
        let bx = (b % NAV_CELLS) as i32;
        let by = (b / NAV_CELLS) as i32;
        let dx = (ax - bx).abs() as f32;
        let dy = (ay - by).abs() as f32;

        (dx + dy) + (std::f32::consts::SQRT_2 - 2.0) * dx.min(dy)
    }

    fn calc_key(&self, cell: usize) -> (f32, f32) {
        let m = self.g[cell].min(self.rhs[cell]);

        (m + self.heuristic(self.start, cell) + self.k_m, m)
    }

    fn queue_update(&mut self, cell: usize) {
        if self.g[cell] != self.rhs[cell] {
            let key = self.calc_key(cell);

            match self.in_queue[cell] {
                Some(k) if k == key => {}
                Some(_) | None => {
                    self.in_queue[cell] = Some(key);
                    self.heap.push(Reverse(DStarNode {
                        k1: key.0,
                        k2: key.1,
                        idx: cell,
                    }));
                }
            }
        } else {
            self.in_queue[cell] = None;
        }
    }

    pub fn initialize(&mut self, grid: &[f32], start_cell: usize, goal_cell: usize) {
        self.known.copy_from_slice(grid);

        self.g.fill(f32::INFINITY);

        self.rhs.fill(f32::INFINITY);

        self.in_queue.fill(None);

        self.heap.clear();

        self.k_m = 0.0;
        self.start = start_cell;
        self.s_last = start_cell;
        self.goal = Some(goal_cell);
        self.rhs[goal_cell] = 0.0;

        let key = self.calc_key(goal_cell);

        self.in_queue[goal_cell] = Some(key);
        self.heap.push(Reverse(DStarNode {
            k1: key.0,
            k2: key.1,
            idx: goal_cell,
        }));
    }

    pub fn update_start(&mut self, start_cell: usize) {
        self.start = start_cell;
    }

    fn fold_movement(&mut self) {
        self.k_m += self.heuristic(self.s_last, self.start);
        self.s_last = self.start;
    }

    fn recompute_rhs(&mut self, cell: usize) {
        if Some(cell) == self.goal {
            return;
        }

        let (ns, n) = nav_neighbors(cell);
        let mut best = f32::INFINITY;

        for i in 0..n {
            let (nb, diag) = ns[i];
            let c = self.edge_cost(cell, nb, diag);

            if c.is_finite() {
                let f = c + self.g[nb];

                if f < best {
                    best = f;
                }
            }
        }

        self.rhs[cell] = best;
    }

    pub fn update_costs(&mut self, grid: &[f32]) {
        let mut changed: Vec<usize> = Vec::new();

        for i in 0..NAV_LEN {
            if grid[i] != self.known[i] {
                self.known[i] = grid[i];
                changed.push(i);
            }
        }

        if changed.is_empty() {
            return;
        }

        self.fold_movement();

        let mut touched: Vec<usize> = Vec::new();

        for &c in &changed {
            if !touched.contains(&c) {
                touched.push(c);
            }

            let (ns, n) = nav_neighbors(c);

            for i in 0..n {
                if !touched.contains(&ns[i].0) {
                    touched.push(ns[i].0);
                }
            }
        }

        for &cell in &touched {
            self.recompute_rhs(cell);

            self.queue_update(cell);
        }
    }

    pub fn plan(&mut self) -> bool {
        if self.goal.is_none() {
            return false;
        }

        self.fold_movement();

        let start = self.start;
        let mut expansions = 0usize;

        while expansions < DSTAR_EXPANSION_BUDGET {
            let Some(&Reverse(node)) = self.heap.peek() else {
                break;
            };

            if self.in_queue[node.idx] != Some((node.k1, node.k2)) {
                self.heap.pop();

                continue;
            }

            let start_key = self.calc_key(start);
            let start_consistent = self.rhs[start] == self.g[start];

            if (node.k1, node.k2) >= (start_key.0, start_key.1) && start_consistent {
                break;
            }

            self.heap.pop();

            self.in_queue[node.idx] = None;

            let u = node.idx;

            expansions += 1;

            let k_new = self.calc_key(u);

            if (node.k1, node.k2) < k_new {
                self.in_queue[u] = Some(k_new);
                self.heap.push(Reverse(DStarNode {
                    k1: k_new.0,
                    k2: k_new.1,
                    idx: u,
                }));
                continue;
            }

            if self.g[u] > self.rhs[u] {
                self.g[u] = self.rhs[u];

                let (ns, n) = nav_neighbors(u);

                for i in 0..n {
                    let (s, diag) = ns[i];

                    if Some(s) != self.goal {
                        let c = self.edge_cost(s, u, diag);

                        if c.is_finite() {
                            let v = c + self.g[u];

                            if v < self.rhs[s] {
                                self.rhs[s] = v;
                            }
                        }
                    }
                    self.queue_update(s);
                }
            } else {
                let g_old = self.g[u];

                self.g[u] = f32::INFINITY;

                let (ns, n) = nav_neighbors(u);

                for i in 0..n {
                    let (s, diag) = ns[i];
                    let c = self.edge_cost(s, u, diag);

                    if c.is_finite() && self.rhs[s] == c + g_old {
                        self.recompute_rhs(s);
                    }

                    self.queue_update(s);
                }
                self.queue_update(u);
            }
        }

        self.rhs[start].is_finite()
    }

    pub fn extract_path(&self, max_cells: usize) -> Vec<usize> {
        let Some(goal) = self.goal else {
            return Vec::new();
        };

        let mut cells: Vec<usize> = Vec::new();
        let mut cur = self.start;
        let mut visited = vec![false; NAV_LEN];

        loop {
            if cur == goal {
                return cells;
            }

            if cells.len() >= max_cells || visited[cur] {
                return Vec::new();
            }

            visited[cur] = true;

            let (ns, n) = nav_neighbors(cur);
            let mut best = f32::INFINITY;
            let mut next = usize::MAX;

            for i in 0..n {
                let (nb, diag) = ns[i];
                let c = self.edge_cost(cur, nb, diag);

                if c.is_finite() {
                    let f = c + self.g[nb];

                    if f < best {
                        best = f;
                        next = nb;
                    }
                }
            }

            if next == usize::MAX || !best.is_finite() {
                return Vec::new();
            }

            cur = next;
            cells.push(cur);
        }
    }
}

pub(crate) fn nav_cell_size(config: &Config) -> f32 {
    (config.world.map_bound * 2.0) / NAV_CELLS as f32
}

pub(crate) fn nav_world_to_cell(pos: Vec2, config: &Config) -> (usize, usize) {
    let cs = nav_cell_size(config);
    let fx = ((pos.x + config.world.map_bound) / cs)
        .floor()
        .clamp(0.0, (NAV_CELLS - 1) as f32);
    let fy = ((pos.y + config.world.map_bound) / cs)
        .floor()
        .clamp(0.0, (NAV_CELLS - 1) as f32);

    (fx as usize, fy as usize)
}

pub(crate) fn nav_cell_center(cx: usize, cy: usize, config: &Config) -> Vec2 {
    let cs = nav_cell_size(config);

    Vec2::new(
        -config.world.map_bound + (cx as f32 + 0.5) * cs,
        -config.world.map_bound + (cy as f32 + 0.5) * cs,
    )
}

pub(crate) fn nearest_passable_cell(danger: &[f32], cx: usize, cy: usize) -> (usize, usize) {
    if danger[cy * NAV_CELLS + cx] < NAV_BLOCK_DANGER {
        return (cx, cy);
    }

    for r in 1..6usize {
        for dy in -(r as i32)..=(r as i32) {
            for dx in -(r as i32)..=(r as i32) {
                if dx.abs() != r as i32 && dy.abs() != r as i32 {
                    continue;
                }

                let ax = cx as i32 + dx;
                let ay = cy as i32 + dy;

                if ax < 0 || ay < 0 || ax >= NAV_CELLS as i32 || ay >= NAV_CELLS as i32 {
                    continue;
                }

                let idx = (ay as usize) * NAV_CELLS + ax as usize;

                if danger[idx] < NAV_BLOCK_DANGER {
                    return (ax as usize, ay as usize);
                }
            }
        }
    }
    (cx, cy)
}

pub(crate) fn nav_line_walkable(danger: &[f32], a: Vec2, b: Vec2, config: &Config) -> bool {
    let steps = (a.distance(b) / (nav_cell_size(config) * 0.5)).ceil() as usize + 1;

    for i in 0..=steps {
        let t = (i as f32 / steps as f32).min(1.0);
        let p = a.lerp(b, t);
        let (cx, cy) = nav_world_to_cell(p, config);

        if danger[cy * NAV_CELLS + cx] >= NAV_BLOCK_DANGER {
            return false;
        }
    }
    true
}

pub(crate) fn string_pull(
    danger: &[f32],
    start: Vec2,
    cells: &[usize],
    goal: Vec2,
    config: &Config,
) -> Vec<Vec2> {
    let centers: Vec<Vec2> = cells
        .iter()
        .map(|&c| nav_cell_center(c % NAV_CELLS, c / NAV_CELLS, config))
        .collect();

    let mut path: Vec<Vec2> = Vec::new();
    let mut anchor = start;
    let mut i = 0;

    while i < centers.len() {
        let mut j = centers.len() - 1;

        while j > i && !nav_line_walkable(danger, anchor, centers[j], config) {
            j -= 1;
        }
        path.push(centers[j]);

        anchor = centers[j];
        i = j + 1;
    }

    path.push(goal);

    path
}

pub fn rebuild_nav_grid(
    scripting: &mut Scripting,
    nav_danger: &mut [f32],
    nav_version: &mut u64,
    config: &Config,
) {
    *nav_version += 1;
    nav_danger.fill(0.0);

    let entities = scripting.entities_mut();

    for i in 0..entities.alive.len() {
        if !entities.alive[i] {
            continue;
        }

        let id = EntityId {
            index: i,
            generation: entities.generations[i],
        };
        let Some(shape) = entities.shapes.get(id) else {
            continue;
        };

        let weight = match *shape.kind {
            ShapeKind::Pentagon => 10.0,
            ShapeKind::Triangle => 2.5,
            ShapeKind::Square => 1.0,
        };
        let (cx, cy) = nav_world_to_cell(entities.positions[i], config);
        let idx = cy * NAV_CELLS + cx;

        nav_danger[idx] += weight;
        if cx > 0 {
            nav_danger[idx - 1] += weight * 0.5;
        }
        if cx + 1 < NAV_CELLS {
            nav_danger[idx + 1] += weight * 0.5;
        }
        if cy > 0 {
            nav_danger[idx - NAV_CELLS] += weight * 0.5;
        }
        if cy + 1 < NAV_CELLS {
            nav_danger[idx + NAV_CELLS] += weight * 0.5;
        }
    }
}
