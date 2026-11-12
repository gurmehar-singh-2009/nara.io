//! Client-side prediction for the player's own tank.
//!
//! The tank moves locally every frame with the same physics as the server
//! (`shared::physics`), so input feels instant instead of waiting a round trip. Each server
//! snapshot is compared with where we predicted the tank to be when the server produced it, and
//! the difference (knockback, rounding, anything we mispredicted) is blended in over a few
//! frames.
//!
//! The rest of the world is drawn interpolated, a round trip and a tick behind our tank. So the
//! tank also collides locally with what's on screen (shapes, tanks, walls, the map edge), using
//! the server's contact response, and shoves the shapes it hits. A shove is drawn until the
//! server's own version of it has had time to arrive, then handed back to the interpolated
//! position, so the shape doesn't freeze until the server catches up or get pushed twice.

use std::collections::{HashMap, VecDeque};

use glam::Vec2;
use shared::{
    packets::client_bound::WallDef,
    physics::{self, Body, Circle, MoveParams},
};

/// Server tick length. Input waits on average half a tick before the server applies it.
const SERVER_TICK_MS: f64 = 100.0;
/// How much predicted history to keep for comparing with snapshots.
const HISTORY_MS: f64 = 1000.0;
/// Errors bigger than this (respawn, teleport) snap instead of blending.
const SNAP_DISTANCE: f32 = 200.0;
/// How fast a correction is blended in (1/s). ~0.1s to fix most of an error.
const CORRECTION_RATE: f32 = 10.0;
/// Only obstacles this close (beyond touching distance) are worth checking.
pub const OBSTACLE_MARGIN: f32 = 80.0;

/// Something on screen the predicted tank can bump into.
#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub id: u32,
    /// Where it's drawn, before any predicted shove.
    pub pos: Vec2,
    pub radius: f32,
    /// Shapes get shoved (and drawn shoved); other tanks only push back.
    pub is_shape: bool,
}

/// What the tank can collide with this frame.
pub struct World<'a> {
    pub obstacles: &'a [Obstacle],
    pub walls: &'a [WallDef],
}

/// The player's input and timing for one frame.
pub struct Frame {
    pub player_id: u32,
    /// The latest server position and when it arrived (ms, `performance.now()`). A changed
    /// `snapshot_time` means a new snapshot.
    pub server_pos: Vec2,
    pub snapshot_time: f64,
    pub move_dir: Option<f32>,
    pub ping_ms: Option<f32>,
    pub now: f64,
    pub dt: f32,
}

#[derive(Default)]
pub struct Prediction {
    active: Option<Active>,
    /// Predicted shoves per shape id, oldest first.
    shoves: HashMap<u32, VecDeque<Shove>>,
    /// How fast a shoved shape's glide fades (1/s), from the server.
    shape_friction: f32,
}

struct Active {
    player_id: u32,
    body: Body,
    /// (time in ms, predicted position), oldest first.
    history: VecDeque<(f64, Vec2)>,
    last_snapshot: f64,
    /// Error still to blend in.
    correction: Vec2,
}

/// One frame's predicted push on a shape.
struct Shove {
    /// When it happened (ms).
    at: f64,
    /// When the server's version of it starts showing up in the shape's interpolated position.
    lands_at: f64,
    /// Immediate position change.
    push: Vec2,
    /// Knockback velocity, which glides and fades with shape friction.
    kick: Vec2,
}

impl Shove {
    fn offset(&self, now: f64, friction: f32) -> Vec2 {
        // interpolation fades the server's version in over one tick; fade ours out to match
        let handed_back = ((now - self.lands_at) / SERVER_TICK_MS).clamp(0.0, 1.0) as f32;
        let age = ((now - self.at) / 1000.0).max(0.0) as f32;
        let (_, glide) = physics::friction_step(friction, age);

        (self.push + self.kick * glide) * (1.0 - handed_back)
    }
}

fn shove_offset(shoves: &HashMap<u32, VecDeque<Shove>>, id: u32, now: f64, friction: f32) -> Vec2 {
    shoves.get(&id).map_or(Vec2::ZERO, |list| {
        list.iter().map(|s| s.offset(now, friction)).sum()
    })
}

impl Active {
    fn new(player_id: u32, pos: Vec2, snapshot_time: f64) -> Self {
        Self {
            player_id,
            body: Body {
                pos,
                ..Body::default()
            },
            history: VecDeque::with_capacity(128),
            last_snapshot: snapshot_time,
            correction: Vec2::ZERO,
        }
    }

    fn snap(&mut self, pos: Vec2) {
        self.body = Body {
            pos,
            ..Body::default()
        };
        self.history.clear();
        self.correction = Vec2::ZERO;
    }

    /// Where we predicted the tank to be at `time`, interpolating between frames.
    fn position_at(&self, time: f64) -> Vec2 {
        let Some(&(first_t, first_p)) = self.history.front() else {
            return self.body.pos;
        };

        if time <= first_t {
            return first_p;
        }

        let (mut t0, mut p0) = (first_t, first_p);

        for &(t1, p1) in self.history.iter().skip(1) {
            if t1 >= time {
                let span = (t1 - t0).max(1e-6);
                return p0.lerp(p1, ((time - t0) / span) as f32);
            }

            (t0, p0) = (t1, p1);
        }

        p0
    }

    /// Collides the tank with everything in `world`, the same way the server does: shapes and
    /// tanks first, then the map edge, then walls.
    fn collide(
        &mut self,
        world: &World,
        params: &MoveParams,
        ticks: f32,
        mut shape_offset: impl FnMut(u32) -> Vec2,
        mut on_shove: impl FnMut(u32, Vec2, Vec2),
    ) {
        let radius = params.radius;
        let mass = physics::tank_mass(radius);

        for ob in world.obstacles {
            let (pos, other_mass) = if ob.is_shape {
                (ob.pos + shape_offset(ob.id), physics::shape_mass(ob.radius))
            } else {
                (ob.pos, physics::tank_mass(ob.radius))
            };

            let Some(c) = physics::contact(
                Circle {
                    pos: self.body.pos,
                    radius,
                    mass,
                },
                Circle {
                    pos,
                    radius: ob.radius,
                    mass: other_mass,
                },
                &params.contact,
                ticks,
            ) else {
                continue;
            };

            self.body.pos += c.push_a;
            self.body.recoil += c.kick_a;
            self.body.vel = physics::stop_into(self.body.vel, c.normal);

            if ob.is_shape {
                on_shove(ob.id, c.push_b, c.kick_b);
            }
        }

        // the server keeps the tank's centre inside the map
        self.body.pos = self
            .body
            .pos
            .clamp(Vec2::splat(-params.map_bound), Vec2::splat(params.map_bound));

        let mut normal = Vec2::ZERO;

        for w in world.walls {
            let half = Vec2::new(w.w, w.h) * 0.5;
            let center = Vec2::new(w.x, w.y);

            if let Some((n, depth)) =
                physics::push_out_of_box(self.body.pos, radius, center - half, center + half)
            {
                self.body.pos += n * depth;
                normal += n;
            }
        }

        let normal = normal.normalize_or_zero();

        if normal != Vec2::ZERO {
            self.body.vel = physics::stop_into(self.body.vel, -normal);
            self.body.recoil = physics::stop_into(self.body.recoil, -normal);
        }
    }
}

impl Prediction {
    pub fn reset(&mut self) {
        self.active = None;
    }

    /// Where the predicted tank is, if prediction is running.
    pub fn position(&self) -> Option<Vec2> {
        self.active.as_ref().map(|a| a.body.pos)
    }

    /// How far a shape is drawn from its interpolated position because we shoved it.
    pub fn shape_offset(&self, id: u32, now: f64) -> Vec2 {
        shove_offset(&self.shoves, id, now, self.shape_friction)
    }

    /// Steps the prediction one frame and returns where to draw the tank.
    pub fn update(&mut self, frame: &Frame, params: &MoveParams, world: &World) -> Vec2 {
        let now = frame.now;
        let dt = frame.dt;
        let rtt = f64::from(frame.ping_ms.unwrap_or(100.0));

        self.shape_friction = params.shape_friction;
        self.prune_shoves(now);

        let a = match &mut self.active {
            Some(a) if a.player_id == frame.player_id => a,
            slot => slot.insert(Active::new(
                frame.player_id,
                frame.server_pos,
                frame.snapshot_time,
            )),
        };

        if frame.snapshot_time != a.last_snapshot {
            a.last_snapshot = frame.snapshot_time;

            // The snapshot reflects input sent about a round trip ago, plus the wait for the
            // next server tick.
            let produced_at = frame.snapshot_time - rtt - SERVER_TICK_MS * 0.5;
            let error = frame.server_pos - a.position_at(produced_at);

            if error.length() > SNAP_DISTANCE {
                a.snap(frame.server_pos);
            } else {
                // replaces any unapplied correction: it was measured against history that
                // already includes everything applied so far
                a.correction = error;
            }
        }

        physics::step(&mut a.body, frame.move_dir, params, dt);

        // what we do now reaches the server in half a round trip and waits for its next tick;
        // the result takes the other half back
        let lands_at = now + rtt + SERVER_TICK_MS * 0.5;
        let shoves = &mut self.shoves;
        let friction = self.shape_friction;
        let mut new_shoves: Vec<(u32, Vec2, Vec2)> = Vec::new();

        a.collide(
            world,
            params,
            dt * physics::TICK_RATE,
            |id| shove_offset(shoves, id, now, friction),
            |id, push, kick| new_shoves.push((id, push, kick)),
        );

        for (id, push, kick) in new_shoves {
            shoves.entry(id).or_default().push_back(Shove {
                at: now,
                lands_at,
                push,
                kick,
            });
        }

        let shift = a.correction * (1.0 - (-CORRECTION_RATE * dt).exp());
        a.correction -= shift;
        a.body.pos += shift;

        // keep history on the corrected path so the next comparison only sees new error
        for (_, pos) in a.history.iter_mut() {
            *pos += shift;
        }

        a.history.push_back((now, a.body.pos));

        while a
            .history
            .front()
            .is_some_and(|&(t, _)| t < now - HISTORY_MS)
        {
            a.history.pop_front();
        }

        a.body.pos
    }

    /// Drops shoves the server's snapshots have fully taken over.
    fn prune_shoves(&mut self, now: f64) {
        self.shoves.retain(|_, list| {
            while list
                .front()
                .is_some_and(|s| now >= s.lands_at + SERVER_TICK_MS)
            {
                list.pop_front();
            }

            !list.is_empty()
        });
    }
}
