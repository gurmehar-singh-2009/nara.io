use glam::Vec2;
use nanorand::Rng;
use shared::packets::{
    NO_TEAM, PACKET_SEED,
    client_bound::{ArenaPacket, WallDef, ZoneDef},
};

const GRID_CELL: f32 = 128.0;
const MAX_GRID_SCAN: usize = 24;

#[derive(Debug, Clone, Copy)]
pub struct Wall {
    pub id: u32,
    pub min: Vec2,
    pub max: Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct Zone {
    pub id: u32,
    pub min: Vec2,
    pub max: Vec2,
    pub team: u8,
    pub damage: f32,
    pub spawn: bool,
    pub shapes: bool,
}

impl Zone {
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}

pub struct Arena {
    pub bound: f32,
    pub team_count: u8,
    pub max_shapes: Option<usize>,
    pub locked: bool,
    pub version: u64,

    walls: Vec<Wall>,
    zones: Vec<Zone>,
    next_id: u32,

    grid: Vec<Vec<usize>>,
    grid_w: usize,
}

impl Arena {
    pub fn new(bound: f32) -> Self {
        let mut arena = Self {
            bound,
            team_count: 0,
            max_shapes: None,
            locked: false,
            version: 0,
            walls: Vec::new(),
            zones: Vec::new(),
            next_id: 1,
            grid: Vec::new(),
            grid_w: 0,
        };

        arena.rebuild_grid();
        arena
    }

    pub fn set_bound(&mut self, bound: f32) -> Result<(), &'static str> {
        if self.locked {
            return Err("the arena size can only be changed while scripts are loading");
        }

        self.bound = bound.max(200.0);
        self.rebuild_grid();
        self.version += 1;

        Ok(())
    }

    pub fn walls(&self) -> &[Wall] {
        &self.walls
    }

    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    pub fn add_wall(&mut self, center: Vec2, size: Vec2) -> u32 {
        let half = size.abs() * 0.5;
        let id = self.next_id;
        self.next_id += 1;

        self.walls.push(Wall {
            id,
            min: center - half,
            max: center + half,
        });

        self.rebuild_grid();
        self.version += 1;

        id
    }

    pub fn remove_wall(&mut self, id: u32) -> bool {
        let before = self.walls.len();
        self.walls.retain(|w| w.id != id);

        if self.walls.len() == before {
            return false;
        }

        self.rebuild_grid();
        self.version += 1;

        true
    }

    pub fn clear_walls(&mut self) {
        self.walls.clear();
        self.rebuild_grid();
        self.version += 1;
    }

    pub fn add_zone(&mut self, mut zone: Zone) -> u32 {
        zone.id = self.next_id;
        self.next_id += 1;

        let id = zone.id;
        self.zones.push(zone);
        self.version += 1;

        id
    }

    pub fn remove_zone(&mut self, id: u32) -> bool {
        let before = self.zones.len();
        self.zones.retain(|z| z.id != id);
        self.version += 1;

        self.zones.len() != before
    }

    pub fn clear_zones(&mut self) {
        self.zones.clear();
        self.version += 1;
    }

    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let max = self.grid_w.saturating_sub(1) as f32;
        let x = ((p.x + self.bound) / GRID_CELL).floor().clamp(0.0, max) as usize;
        let y = ((p.y + self.bound) / GRID_CELL).floor().clamp(0.0, max) as usize;

        (x, y)
    }

    fn rebuild_grid(&mut self) {
        self.grid_w = ((self.bound * 2.0) / GRID_CELL).ceil().max(1.0) as usize;
        self.grid = vec![Vec::new(); self.grid_w * self.grid_w];

        for (i, wall) in self.walls.iter().enumerate() {
            let (x0, y0) = self.cell_of(wall.min);
            let (x1, y1) = self.cell_of(wall.max);

            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.grid[y * self.grid_w + x].push(i);
                }
            }
        }
    }

    fn for_walls_in(&self, min: Vec2, max: Vec2, mut f: impl FnMut(&Wall) -> bool) -> bool {
        if self.walls.is_empty() {
            return false;
        }

        let (x0, y0) = self.cell_of(min);
        let (x1, y1) = self.cell_of(max);

        if (x1 - x0 + 1) * (y1 - y0 + 1) > MAX_GRID_SCAN {
            return self
                .walls
                .iter()
                .filter(|w| w.max.x >= min.x && w.min.x <= max.x)
                .filter(|w| w.max.y >= min.y && w.min.y <= max.y)
                .any(f);
        }

        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in &self.grid[y * self.grid_w + x] {
                    if f(&self.walls[i]) {
                        return true;
                    }
                }
            }
        }

        false
    }

    pub fn circle_hits_wall(&self, p: Vec2, radius: f32) -> bool {
        let r = Vec2::splat(radius);

        self.for_walls_in(p - r, p + r, |w| {
            p.clamp(w.min, w.max).distance_squared(p) < radius * radius
        })
    }

    pub fn push_out(&self, p: Vec2, radius: f32) -> Option<(Vec2, Vec2)> {
        let r = Vec2::splat(radius);
        let mut pos = p;
        let mut normal = Vec2::ZERO;

        self.for_walls_in(p - r, p + r, |w| {
            let closest = pos.clamp(w.min, w.max);
            let delta = pos - closest;
            let dist_sq = delta.length_squared();

            if dist_sq >= radius * radius {
                return false;
            }

            let (n, depth) = if dist_sq > 1e-6 {
                let dist = dist_sq.sqrt();
                (delta / dist, radius - dist)
            } else {
                let left = pos.x - w.min.x;
                let right = w.max.x - pos.x;
                let down = pos.y - w.min.y;
                let up = w.max.y - pos.y;
                let nearest = left.min(right).min(down).min(up);

                if nearest == left {
                    (Vec2::NEG_X, left + radius)
                } else if nearest == right {
                    (Vec2::X, right + radius)
                } else if nearest == down {
                    (Vec2::NEG_Y, down + radius)
                } else {
                    (Vec2::Y, up + radius)
                }
            };

            pos += n * depth;
            normal += n;

            false
        });

        if pos == p {
            None
        } else {
            Some((pos, normal.normalize_or_zero()))
        }
    }

    pub fn segment_hits_wall(&self, a: Vec2, b: Vec2, radius: f32) -> bool {
        let pad = Vec2::splat(radius.max(0.0));

        self.for_walls_in(a.min(b) - pad, a.max(b) + pad, |w| {
            segment_hits_box(a, b, w.min - pad, w.max + pad)
        })
    }

    pub fn zone_at(&self, p: Vec2) -> Option<&Zone> {
        self.zones.iter().find(|z| z.contains(p))
    }

    pub fn shapes_allowed(&self, p: Vec2) -> bool {
        self.zones.iter().all(|z| z.shapes || !z.contains(p))
    }

    pub fn random_open_point(
        &self,
        rnd: &mut nanorand::WyRand,
        min: Vec2,
        max: Vec2,
        radius: f32,
        accept: impl Fn(Vec2) -> bool,
    ) -> Vec2 {
        let min = min.max(Vec2::splat(-self.bound + radius));
        let max = max.min(Vec2::splat(self.bound - radius)).max(min);

        let mut point = (min + max) * 0.5;

        for _ in 0..64 {
            point = Vec2::new(
                min.x + rnd.generate::<f32>() * (max.x - min.x),
                min.y + rnd.generate::<f32>() * (max.y - min.y),
            );

            if !self.circle_hits_wall(point, radius) && accept(point) {
                return point;
            }
        }

        point
    }

    pub fn spawn_point(&self, rnd: &mut nanorand::WyRand, team: u8, radius: f32) -> Vec2 {
        let bases: Vec<&Zone> = self
            .zones
            .iter()
            .filter(|z| z.spawn && z.team == team && team != NO_TEAM)
            .collect();

        if !bases.is_empty() {
            let zone = bases[rnd.generate::<u32>() as usize % bases.len()];
            return self.random_open_point(rnd, zone.min, zone.max, radius, |_| true);
        }

        let bound = Vec2::splat(self.bound);
        self.random_open_point(rnd, -bound, bound, radius, |p| {
            self.zone_at(p)
                .is_none_or(|z| z.team == NO_TEAM || z.team == team)
        })
    }

    pub fn packet(&self) -> ArenaPacket {
        ArenaPacket::new(
            self.bound,
            self.team_count,
            self.walls
                .iter()
                .map(|w| {
                    let c = (w.min + w.max) * 0.5;
                    let s = w.max - w.min;
                    WallDef {
                        x: c.x,
                        y: c.y,
                        w: s.x,
                        h: s.y,
                    }
                })
                .collect(),
            self.zones
                .iter()
                .map(|z| {
                    let c = (z.min + z.max) * 0.5;
                    let s = z.max - z.min;
                    ZoneDef {
                        x: c.x,
                        y: c.y,
                        w: s.x,
                        h: s.y,
                        team: z.team,
                    }
                })
                .collect(),
            PACKET_SEED as u64,
        )
    }
}

fn segment_hits_box(a: Vec2, b: Vec2, min: Vec2, max: Vec2) -> bool {
    let d = b - a;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;

    for axis in 0..2 {
        let (start, dir, lo, hi) = if axis == 0 {
            (a.x, d.x, min.x, max.x)
        } else {
            (a.y, d.y, min.y, max.y)
        };

        if dir.abs() < 1e-6 {
            if start < lo || start > hi {
                return false;
            }
            continue;
        }

        let inv = 1.0 / dir;
        let mut near = (lo - start) * inv;
        let mut far = (hi - start) * inv;

        if near > far {
            std::mem::swap(&mut near, &mut far);
        }

        t0 = t0.max(near);
        t1 = t1.min(far);

        if t0 > t1 {
            return false;
        }
    }

    true
}
