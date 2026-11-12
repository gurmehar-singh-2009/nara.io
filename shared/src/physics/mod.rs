//! Tank movement, shared by the server simulation and the client's prediction so both move a
//! tank the same way.

use bitcode::{Decode, Encode};
use glam::Vec2;

/// What a client needs to predict its own tank. The server sends it with the player stats.
#[derive(Clone, Copy, Debug, PartialEq, Encode, Decode)]
pub struct MoveParams {
    /// Top speed for the tank's current level, class and movement points.
    pub max_speed: f32,
    /// How fast velocity eases toward the input direction (1/s).
    pub move_friction: f32,
    /// How fast knockback fades (1/s).
    pub recoil_decay: f32,
    /// The tank's collision radius at its current level.
    pub radius: f32,
    /// Tanks are kept inside `[-map_bound, map_bound]` on both axes.
    pub map_bound: f32,
    /// How bodies push apart on contact.
    pub contact: ContactParams,
    /// How fast a shoved shape's velocity fades (1/s).
    pub shape_friction: f32,
}

impl Default for MoveParams {
    fn default() -> Self {
        Self {
            max_speed: 0.0,
            move_friction: 4.0,
            recoil_decay: 2.0,
            radius: 21.0,
            map_bound: 2500.0,
            contact: ContactParams::default(),
            shape_friction: 3.0,
        }
    }
}

/// The server's tick rate. Contact response is tuned per tick, so the client scales it by how
/// much of a tick each frame covers.
pub const TICK_RATE: f32 = 10.0;

/// How two overlapping bodies (tanks, shapes) push apart.
#[derive(Clone, Copy, Debug, PartialEq, Encode, Decode)]
pub struct ContactParams {
    /// Bounce velocity per tick of contact, split by mass.
    pub knockback: f32,
    /// Overlap left alone, so resting contacts don't jitter.
    pub slop: f32,
    /// Fraction of the overlap removed per tick.
    pub correction: f32,
}

impl Default for ContactParams {
    fn default() -> Self {
        Self {
            knockback: 80.0,
            slop: 0.05,
            correction: 0.9,
        }
    }
}

/// Mass of a tank of `radius`.
pub fn tank_mass(radius: f32) -> f32 {
    radius * radius
}

/// Mass of a shape of `radius`: lighter than a tank its size, so tanks shove shapes aside.
pub fn shape_mass(radius: f32) -> f32 {
    radius * radius * 0.5
}

/// Collision radius of a shape by kind id (1 square, 2 triangle, 3 pentagon).
pub fn shape_radius(kind: u32) -> f32 {
    match kind {
        3 => 26.0,
        _ => 16.0,
    }
}

/// A circle that can be pushed around.
#[derive(Clone, Copy, Debug)]
pub struct Circle {
    pub pos: Vec2,
    pub radius: f32,
    pub mass: f32,
}

/// How a contact moves two bodies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// Unit vector from body `a` toward body `b`.
    pub normal: Vec2,
    /// Position change for `a`.
    pub push_a: Vec2,
    /// Position change for `b`.
    pub push_b: Vec2,
    /// Knockback velocity for `a`.
    pub kick_a: Vec2,
    /// Knockback velocity for `b`.
    pub kick_b: Vec2,
}

/// Resolves an overlap between `a` and `b`, the lighter body taking more of it. `ticks` is how
/// much time this covers in server ticks: the server passes 1, the client's prediction passes a
/// frame's fraction of a tick so the response doesn't depend on frame rate.
pub fn contact(a: Circle, b: Circle, params: &ContactParams, ticks: f32) -> Option<Contact> {
    let delta = b.pos - a.pos;
    let dist = delta.length();
    let min_dist = a.radius + b.radius;

    if dist >= min_dist {
        return None;
    }

    let normal = if dist > 1e-4 {
        delta / dist
    } else {
        Vec2::X
    };
    let overlap = min_dist - dist;

    let inv_a = 1.0 / a.mass;
    let inv_b = 1.0 / b.mass;
    // removing `c` of the overlap per tick is removing `1 - (1 - c)^ticks` over `ticks` ticks
    let correction = 1.0 - (1.0 - params.correction.clamp(0.0, 1.0)).powf(ticks);
    let corr = (overlap - params.slop).max(0.0) * correction / (inv_a + inv_b);

    let total = a.mass + b.mass;

    Some(Contact {
        normal,
        push_a: -normal * corr * inv_a,
        push_b: normal * corr * inv_b,
        kick_a: -normal * params.knockback * (b.mass / total) * ticks,
        kick_b: normal * params.knockback * (a.mass / total) * ticks,
    })
}

/// Removes the part of `vel` heading along `toward` (into whatever is there).
pub fn stop_into(vel: Vec2, toward: Vec2) -> Vec2 {
    let into = vel.dot(toward);

    if into > 0.0 { vel - toward * into } else { vel }
}

/// Pushes a circle at `pos` out of the box `min..max`. Returns the outward normal and depth.
pub fn push_out_of_box(pos: Vec2, radius: f32, min: Vec2, max: Vec2) -> Option<(Vec2, f32)> {
    let closest = pos.clamp(min, max);
    let delta = pos - closest;
    let dist_sq = delta.length_squared();

    if dist_sq >= radius * radius {
        return None;
    }

    if dist_sq > 1e-6 {
        let dist = dist_sq.sqrt();
        return Some((delta / dist, radius - dist));
    }

    // centre inside the box: leave by the nearest side
    let left = pos.x - min.x;
    let right = max.x - pos.x;
    let down = pos.y - min.y;
    let up = max.y - pos.y;
    let nearest = left.min(right).min(down).min(up);

    Some(if nearest == left {
        (Vec2::NEG_X, left + radius)
    } else if nearest == right {
        (Vec2::X, right + radius)
    } else if nearest == down {
        (Vec2::NEG_Y, down + radius)
    } else {
        (Vec2::Y, up + radius)
    })
}

/// A moving tank: its own velocity plus knockback (recoil, hits, collisions) on top.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Body {
    pub pos: Vec2,
    pub vel: Vec2,
    pub recoil: Vec2,
}

/// The inputs that set a tank's top speed. All multipliers, so 1.0 / 0 means "no effect".
#[derive(Clone, Copy, Debug)]
pub struct SpeedInputs {
    pub base_speed: f32,
    /// The tank class's speed multiplier.
    pub class_speed: f32,
    /// Each movement point multiplies speed by this.
    pub move_factor: f32,
    pub move_points: u8,
    /// Each level divides speed by this.
    pub level_falloff: f32,
    pub level: u32,
}

pub fn tank_max_speed(s: SpeedInputs) -> f32 {
    s.base_speed * s.class_speed * s.move_factor.powi(i32::from(s.move_points))
        / s.level_falloff.powi(s.level.saturating_sub(1) as i32)
}

/// For a velocity that decays as `e^(-k t)`: how much is left after `dt`, and how far it
/// carries you meanwhile (per unit of starting velocity).
pub fn friction_step(k: f32, dt: f32) -> (f32, f32) {
    let k = k.max(0.01);
    let decay = (-k * dt).exp();
    (decay, (1.0 - decay) / k)
}

/// Advances a tank by `dt`. Velocity eases toward the input direction exponentially
/// (diep-style glide) and knockback fades the same way at its own rate. Both are integrated
/// exactly, so the result doesn't depend on the step size: the server's 10 Hz ticks and the
/// client's per-frame steps land in the same place.
pub fn step(body: &mut Body, move_dir: Option<f32>, params: &MoveParams, dt: f32) {
    if dt <= 0.0 {
        return;
    }

    let desired = move_dir.map_or(Vec2::ZERO, |dir| Vec2::from_angle(dir) * params.max_speed);

    let (move_decay, move_travel) = friction_step(params.move_friction, dt);
    let (recoil_decay, recoil_travel) = friction_step(params.recoil_decay, dt);

    body.pos += desired * dt + (body.vel - desired) * move_travel + body.recoil * recoil_travel;
    body.vel = desired + (body.vel - desired) * move_decay;
    body.recoil *= recoil_decay;
}

/// Keeps a tank of `radius` inside a square map of half-size `bound`, killing velocity into
/// the edge.
pub fn clamp_to_map(body: &mut Body, bound: f32) {
    let clamped = body.pos.clamp(Vec2::splat(-bound), Vec2::splat(bound));

    if clamped.x != body.pos.x {
        body.vel.x = 0.0;
        body.recoil.x = 0.0;
    }

    if clamped.y != body.pos.y {
        body.vel.y = 0.0;
        body.recoil.y = 0.0;
    }

    body.pos = clamped;
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_SPEED: f32 = 168.0;

    fn params() -> MoveParams {
        MoveParams {
            max_speed: MAX_SPEED,
            move_friction: 4.0,
            recoil_decay: 2.0,
            ..MoveParams::default()
        }
    }

    fn run(steps: u32, dt: f32) -> Body {
        let mut body = Body {
            recoil: Vec2::new(0.0, 80.0),
            ..Body::default()
        };

        for _ in 0..steps {
            step(&mut body, Some(0.0), &params(), dt);
        }

        body
    }

    fn circle(x: f32, radius: f32, mass: f32) -> Circle {
        Circle {
            pos: Vec2::new(x, 0.0),
            radius,
            mass,
        }
    }

    #[test]
    fn contact_splits_by_mass() {
        let tank = circle(0.0, 21.0, tank_mass(21.0));
        let shape = circle(30.0, 16.0, shape_mass(16.0));
        let c = contact(tank, shape, &ContactParams::default(), 1.0).unwrap();

        // the shape is lighter, so it moves (and bounces) more
        assert!(c.push_b.x > -c.push_a.x && c.push_a.x < 0.0);
        assert!(c.kick_b.x > -c.kick_a.x && c.kick_a.x < 0.0);
        assert!(contact(tank, circle(40.0, 16.0, 1.0), &ContactParams::default(), 1.0).is_none());
    }

    #[test]
    fn frame_contacts_add_up_to_a_tick() {
        let p = ContactParams {
            slop: 0.0,
            ..ContactParams::default()
        };
        let (mut a, mut b) = (circle(0.0, 21.0, 441.0), circle(30.0, 16.0, 128.0));
        let tick = contact(a, b, &p, 1.0).unwrap();

        // six 1/60 s frames (one server tick), separating a little each frame
        for _ in 0..6 {
            let c = contact(a, b, &p, 1.0 / 6.0).unwrap();
            a.pos += c.push_a;
            b.pos += c.push_b;
        }

        let left = a.radius + b.radius - a.pos.distance(b.pos);
        assert!((left - 7.0 * (1.0 - p.correction)).abs() < 1e-3, "{left}");
        assert!(tick.push_b.x > 0.0);
    }

    #[test]
    fn step_size_does_not_change_the_result() {
        // one second: server ticks at 10 Hz, a client frame is ~1/60 s
        let server = run(10, 0.1);
        let client = run(60, 1.0 / 60.0);

        assert!(server.pos.distance(client.pos) < 1e-3, "{server:?} vs {client:?}");
        assert!(server.vel.distance(client.vel) < 1e-3);
    }

    #[test]
    fn approaches_top_speed() {
        let body = run(30, 0.1);
        assert!((body.vel.x - MAX_SPEED).abs() < 0.01);
        assert!(body.recoil.length() < 0.5);
    }
}
