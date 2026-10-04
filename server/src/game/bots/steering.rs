use glam::Vec2;

use crate::entities::{
    entity::{Entities, EntityId},
    shape::ShapeKind,
    spatial_hash::{HashEntity, SpatialHash},
};

const STEER_DIRS: usize = 16;
const STEER_RADIUS: f32 = 240.0;

pub fn context_steer(
    entities: &Entities,
    hash: &SpatialHash,
    nearby: &mut Vec<HashEntity>,
    pos: Vec2,
    desired: Vec2,
    ignore: Option<EntityId>,
) -> Option<f32> {
    let desired = desired.normalize_or_zero();

    let mut obstacles: [(Vec2, f32); 64] = [(Vec2::ZERO, 0.0); 64];
    let mut obstacle_count = 0usize;
    let mut bullets: [(Vec2, Vec2); 16] = [(Vec2::ZERO, Vec2::ZERO); 16];
    let mut bullet_count = 0usize;

    hash.get_nearby_into(nearby, pos.x, pos.y, STEER_RADIUS + 80.0);
    for &candidate in nearby.iter() {
        match candidate {
            HashEntity::Entity(eid) => {
                if Some(eid) == ignore || !entities.is_alive(eid) {
                    continue;
                }
                let opos = entities.positions[eid.index];
                let off = opos - pos;
                if off.length() > STEER_RADIUS {
                    continue;
                }
                let weight = if let Some(shape) = entities.shapes.get(eid) {
                    match *shape.kind {
                        ShapeKind::Pentagon => 3.0,
                        ShapeKind::Triangle => 1.0,
                        ShapeKind::Square => 0.7,
                    }
                } else {
                    1.6 // tank body
                };
                if obstacle_count < obstacles.len() {
                    obstacles[obstacle_count] = (off, weight);
                    obstacle_count += 1;
                }
            }
            HashEntity::Bullet(bi) => {
                if bi >= entities.bullets.len() || bullet_count >= bullets.len() {
                    continue;
                }
                let bpos = entities.bullets.position(bi);
                let vel = entities.bullets.velocity_at(bi);
                if vel.length_squared() < 2500.0 {
                    continue; // < 50 u/s
                }
                if (pos - bpos).dot(vel) <= 0.0 {
                    continue; // receding
                }
                bullets[bullet_count] = (bpos, vel);
                bullet_count += 1;
            }
        }
    }

    let mut best_score = f32::MIN;
    let mut best_angle = 0.0f32;
    let mut any = false;

    for k in 0..STEER_DIRS {
        let angle = k as f32 * (std::f32::consts::TAU / STEER_DIRS as f32);
        let dir = Vec2::from_angle(angle);
        let mut score = dir.dot(desired);

        // avoid steering into obstacles (scaled by proximity)
        for &(off, weight) in &obstacles[..obstacle_count] {
            let dist = off.length().max(1e-3);
            let align = dir.dot(off / dist);
            if align > 0.0 {
                let prox = (1.0 - dist / STEER_RADIUS).max(0.0);
                score -= weight * align * prox * prox * 3.0;
            }
        }

        for &(off, weight) in &obstacles[..obstacle_count] {
            let dist = off.length().max(1e-3);
            if dist < 90.0 {
                let align = (dir.dot(off / dist)).max(0.0);
                score -= weight * 0.8 * (1.0 - dist / 90.0) * (0.35 + 0.65 * align);
            }
        }

        // dodge predicted bullet paths
        for &(bpos, vel) in &bullets[..bullet_count] {
            for ti in 1..=4usize {
                let t = ti as f32 * 0.1;
                let bfuture = bpos + vel * t;
                let myfuture = pos + dir * 170.0 * t;
                let d = bfuture.distance(myfuture);
                if d < 80.0 {
                    score -= (1.0 - d / 80.0) * 3.0;
                }
            }
        }

        if !any || score > best_score {
            any = true;
            best_score = score;
            best_angle = angle;
        }
    }

    if any { Some(best_angle) } else { None }
}
