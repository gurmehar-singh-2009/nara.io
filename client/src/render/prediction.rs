//! Client-side prediction for the player's own tank.
//!
//! The tank moves locally every frame with the same physics as the server
//! (`shared::physics`), so input feels instant instead of waiting a round trip. Each server
//! snapshot is compared with where we predicted the tank to be when the server produced it, and
//! the difference (knockback, collisions, walls, rounding) is blended in over a few frames.

use std::collections::VecDeque;

use glam::Vec2;
use shared::physics::{self, Body, MoveParams};

/// Server tick length. Input waits on average half a tick before the server applies it.
const SERVER_TICK_MS: f64 = 100.0;
/// How much predicted history to keep for comparing with snapshots.
const HISTORY_MS: f64 = 1000.0;
/// Errors bigger than this (respawn, teleport) snap instead of blending.
const SNAP_DISTANCE: f32 = 200.0;
/// How fast a correction is blended in (1/s). ~0.1s to fix most of an error.
const CORRECTION_RATE: f32 = 10.0;

#[derive(Default)]
pub struct Prediction {
    active: Option<Active>,
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
}

impl Prediction {
    pub fn reset(&mut self) {
        self.active = None;
    }

    /// Steps the prediction one frame and returns where to draw the tank.
    ///
    /// `server_pos` and `snapshot_time` are the latest server position and when it arrived
    /// (ms, `performance.now()`); a changed `snapshot_time` means a new snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        player_id: u32,
        server_pos: Vec2,
        snapshot_time: f64,
        move_dir: Option<f32>,
        params: &MoveParams,
        ping_ms: Option<f32>,
        now: f64,
        dt: f32,
    ) -> Vec2 {
        let a = match &mut self.active {
            Some(a) if a.player_id == player_id => a,
            slot => slot.insert(Active::new(player_id, server_pos, snapshot_time)),
        };

        if snapshot_time != a.last_snapshot {
            a.last_snapshot = snapshot_time;

            // The snapshot reflects input sent about a round trip ago, plus the wait for the
            // next server tick.
            let rtt = f64::from(ping_ms.unwrap_or(100.0));
            let produced_at = snapshot_time - rtt - SERVER_TICK_MS * 0.5;
            let error = server_pos - a.position_at(produced_at);

            if error.length() > SNAP_DISTANCE {
                a.snap(server_pos);
            } else {
                // replaces any unapplied correction: it was measured against history that
                // already includes everything applied so far
                a.correction = error;
            }
        }

        physics::step(&mut a.body, move_dir, params, dt);

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
}
