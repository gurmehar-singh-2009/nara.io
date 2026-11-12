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
}

impl Default for MoveParams {
    fn default() -> Self {
        Self {
            max_speed: 0.0,
            move_friction: 4.0,
            recoil_decay: 2.0,
        }
    }
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

    const PARAMS: MoveParams = MoveParams {
        max_speed: 168.0,
        move_friction: 4.0,
        recoil_decay: 2.0,
    };

    fn run(steps: u32, dt: f32) -> Body {
        let mut body = Body {
            recoil: Vec2::new(0.0, 80.0),
            ..Body::default()
        };

        for _ in 0..steps {
            step(&mut body, Some(0.0), &PARAMS, dt);
        }

        body
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
        assert!((body.vel.x - PARAMS.max_speed).abs() < 0.01);
        assert!(body.recoil.length() < 0.5);
    }
}
