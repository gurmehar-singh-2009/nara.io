use std::collections::HashMap;

use glam::Vec2;
use nanorand::Rng;

use crate::{
    entities::{
        entity::{Entities, EntityId},
        spatial_hash::{HashEntity, SpatialHash},
    },
    fs::load_config::Config,
};

#[derive(Clone, Copy)]
pub struct Enemy {
    pub entity: EntityId,
    pub pos: Vec2,
    pub dist: f32,
    pub power: f32,
    pub is_bot: bool,
    pub hp_frac: f32,
}

pub fn is_bot_id(conn_id: u32, config: &Config) -> bool {
    conn_id >= config.bots.id_base
}

pub fn tank_power(entities: &Entities, eid: EntityId) -> f32 {
    let level = entities.tanks.get(eid).map(|t| *t.level).unwrap_or(1) as f32;
    let max = entities.tanks.get(eid).map(|t| *t.max_health).unwrap_or(1);
    let hp = entities.get(eid).map(|e| *e.health).unwrap_or(0) as f32;
    let frac = if max > 0 { hp / max as f32 } else { 0.0 };

    level * (0.35 + 0.65 * frac)
}

pub fn scan_enemies(
    entities: &Entities,
    players: &HashMap<u32, EntityId>,
    exclude: EntityId,
    center: Vec2,
    vision: f32,
    jitter: f32,
    rnd: &mut nanorand::WyRand,
    out: &mut Vec<Enemy>,
    config: &Config,
) {
    out.clear();
    for (&conn, &pe) in players.iter() {
        if pe == exclude || !entities.is_alive(pe) {
            continue;
        }

        let ppos = entities.positions[pe.index];
        let dist = center.distance(ppos);

        if dist > vision {
            continue;
        }

        let mut power = tank_power(entities, pe);
        if jitter > 0.0 {
            power *= 1.0 + (rnd.generate::<f32>() * 2.0 - 1.0) * jitter;
        }

        let hp = entities.get(pe).map(|e| *e.health).unwrap_or(0) as f32;
        let max = entities
            .tanks
            .get(pe)
            .map(|t| *t.max_health)
            .unwrap_or(1)
            .max(1) as f32;

        out.push(Enemy {
            entity: pe,
            pos: ppos,
            dist,
            power,
            is_bot: is_bot_id(conn, config),
            hp_frac: hp / max,
        });
    }
    out.sort_by(|a, b| {
        a.dist
            .partial_cmp(&b.dist)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(config.bots.max_tracked_enemies);
}

pub fn scan_positions(
    entities: &Entities,
    players: &HashMap<u32, EntityId>,
    exclude: EntityId,
    center: Vec2,
    vision: f32,
    out: &mut Vec<(EntityId, Vec2, f32)>,
) {
    out.clear();

    for (&_, &pe) in players.iter() {
        if pe == exclude || !entities.is_alive(pe) {
            continue;
        }

        let ppos = entities.positions[pe.index];
        let d = center.distance(ppos);

        if d > vision {
            continue;
        }

        out.push((pe, ppos, d));
    }

    out.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(16);
}

pub fn escape_direction(pos: Vec2, enemy_positions: &[Vec2]) -> Vec2 {
    let mut away = Vec2::ZERO;
    let mut bearings: Vec<f32> = Vec::with_capacity(enemy_positions.len());

    for &e in enemy_positions {
        let dir = (e - pos).normalize_or_zero();

        away -= dir;
        bearings.push(dir.y.atan2(dir.x));
    }

    if bearings.is_empty() {
        return Vec2::ZERO;
    }

    if bearings.len() == 1 {
        return away.normalize_or_zero();
    }

    let away = away.normalize_or_zero();

    bearings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut best_gap = -f32::INFINITY;
    let mut best_mid = 0.0f32;
    for i in 0..bearings.len() {
        let a = bearings[i];

        let b = if i + 1 == bearings.len() {
            bearings[0] + std::f32::consts::TAU
        } else {
            bearings[i + 1]
        };

        let gap = b - a;

        if gap > best_gap {
            best_gap = gap;
            best_mid = a + gap * 0.5;
        }
    }

    let gap_dir = Vec2::from_angle(best_mid);

    (gap_dir + away * 0.6).normalize_or_zero()
}

pub fn fire_line_clear(
    entities: &Entities,
    hash: &SpatialHash,
    buf: &mut Vec<HashEntity>,
    from: Vec2,
    to: Vec2,
    margin: f32,
    exclude: Option<EntityId>,
) -> bool {
    let delta = to - from;
    let dist = delta.length();

    if dist < 1.0 {
        return true;
    }
    let dir = delta / dist;

    let steps = ((dist / 120.0).ceil() as usize).max(1);
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let p = from + delta * t;

        hash.get_nearby_into(buf, p.x, p.y, 140.0);

        for &cand in buf.iter() {
            let HashEntity::Entity(eid) = cand else {
                continue;
            };

            if Some(eid) == exclude || !entities.is_alive(eid) {
                continue;
            }

            let Some(shape) = entities.shapes.get(eid) else {
                continue; // tanks don't block
            };

            let c = entities.positions[eid.index];
            let rel = c - from;
            let u = rel.dot(dir).clamp(0.0, dist);
            let closest = from + dir * u;

            if closest.distance(c) < shape_radius(*shape.kind) + margin {
                return false;
            }
        }
    }

    true
}

fn shape_radius(kind: crate::entities::shape::ShapeKind) -> f32 {
    match kind {
        crate::entities::shape::ShapeKind::Square => 16.0,
        crate::entities::shape::ShapeKind::Triangle => 16.0,
        crate::entities::shape::ShapeKind::Pentagon => 26.0,
    }
}

// brain hurt
pub fn intercept_angle(from: Vec2, to: Vec2, vel: Vec2, bullet_speed: f32) -> f32 {
    let rel = to - from;
    let a = vel.dot(vel) - bullet_speed * bullet_speed;
    let b = 2.0 * rel.dot(vel);
    let c = rel.dot(rel);

    let mut t = 0.0f32;
    if a.abs() < 1e-6 {
        if b.abs() > 1e-6 {
            t = (-c / b).max(0.0);
        }
    } else {
        let disc = b * b - 4.0 * a * c;

        if disc >= 0.0 {
            let sq = disc.sqrt();
            let t1 = (-b - sq) / (2.0 * a);
            let t2 = (-b + sq) / (2.0 * a);

            t = if t1 > 0.0 && t2 > 0.0 {
                t1.min(t2)
            } else {
                t1.max(t2).max(0.0)
            };
        }
    }

    let aim_point = to + vel * t;
    (aim_point - from).y.atan2((aim_point - from).x)
}
