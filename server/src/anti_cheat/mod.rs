// Ok so a diep.io style game has a few vectors we can monitor:
//
// - Tank upgrades speed (tank upgrade and level upgrades). Can't be *too* fast
//   (prevent auto-upgrade scripts).
// - Tank aim. Calculate the higher-order derivatives for jerk, and see if it
//   spikes. Obviously you can't move your mouse
// across your screen instantly. Also, we should check if the movement is
// linear, we expect a "bell curve" shape for their movement. Also: check the
// error value (actual - expected) for the bell curve shape, if it's
// consistently off then they are offsetting using Math.random() or such.
// Extreme: perform Frequency Analysis, turn a list of previous angles into the
// freq domain using FFT and check the avg frequency. Something high like 20Hz
// is impossible. Later: train a small NN based on valid movement data and use
// that instead.
//
// Of course, you should only be flagging the user. We will implement a score
// based system and ban appropriately.
//
// For multibox:
// - check if multiple clients are sending same packets (most likely
//   movement/aim)
// - they will probably add some noise to it though so check if some people are
//   aiming in a relative direction often.
// - also just block vpns, proxy, etc (scrape free proxy sites, they used that
//   for arras).
//
// Wired into the game loop by `GameState`: aim samples are recorded on every
// `PlayerAim` event, upgrade requests are interval-checked in
// `apply_tank_upgrade`, and the multibox scan / decay / flag reporting pass is
// `run_anti_cheat_pass`.

use std::{
    collections::VecDeque,
    f32::consts::{PI, TAU},
};

pub mod aimbot_anti {
    use super::{PI, TAU};

    #[inline(always)]
    pub fn normalize_angle(mut angle: f32) -> f32 {
        angle = (angle + PI) % TAU;
        if angle < 0.0 {
            angle += TAU;
        }
        angle - PI
    }

    #[inline(always)]
    pub fn angular_delta(angle: f32, angle_old: f32) -> f32 {
        normalize_angle(angle - angle_old)
    }

    #[inline(always)]
    pub fn angular_accel(vel: f32, vel_old: f32, time: f32) -> f32 {
        (vel - vel_old) / time
    }

    #[inline(always)]
    pub fn angular_jerk(accel: f32, accel_old: f32, time: f32) -> f32 {
        (accel - accel_old) / time
    }

    pub fn sign_inversion_ratio(velocities: &[f32], epsilon: f32) -> f32 {
        if velocities.len() < 2 {
            return 0.0;
        }

        let mut flips = 0;
        let total = (velocities.len() - 1) as f32;

        for i in 1..velocities.len() {
            let v_curr = velocities[i];
            let v_prev = velocities[i - 1];

            if v_curr.signum() * v_prev.signum() < 0.0 && v_curr.abs() > epsilon {
                flips += 1;
            }
        }

        flips as f32 / total
    }

    /// Kurtosis evaluation.
    pub fn residual_kurtosis(actual_angles: &[f32], expected_angles: &[f32]) -> f32 {
        if actual_angles.is_empty() || actual_angles.len() != expected_angles.len() {
            return 0.0;
        }

        let n = actual_angles.len() as f32;
        let mut sum = 0.0;

        for i in 0..actual_angles.len() {
            sum += angular_delta(actual_angles[i], expected_angles[i]);
        }

        let mean = sum / n;
        let mut variance_sum = 0.0;
        let mut fourth_moment_sum = 0.0;

        for i in 0..actual_angles.len() {
            let err = angular_delta(actual_angles[i], expected_angles[i]);
            let diff = err - mean;
            let diff_sq = diff * diff;
            variance_sum += diff_sq;
            fourth_moment_sum += diff_sq * diff_sq;
        }

        let variance = variance_sum / n;
        if variance < 1e-6 {
            return 0.0;
        }

        let fourth_moment = fourth_moment_sum / n;
        fourth_moment / (variance * variance)
    }
}

pub mod upgrade_anti {
    #[inline(always)]
    pub fn check_upgrade_interval(
        last_upgrade_ms: u64,
        current_ms: u64,
        min_interval_ms: u64,
    ) -> bool {
        // clock skew (current < last) counts as zero elapsed instead of
        // permanently rejecting the player
        current_ms.saturating_sub(last_upgrade_ms) >= min_interval_ms
    }

    pub fn check_valid_upgrade() {
        todo!()
    }
}

pub mod multibox_anti {
    use super::aimbot_anti::angular_delta;

    pub fn calculate_input_similarity(
        angles_a: &[f32],
        angles_b: &[f32],
        threshold_rad: f32,
    ) -> f32 {
        // compare the overlapping tails so streams of different lengths
        // (different connect times) can still be matched
        if angles_a.len() < 2 || angles_b.len() < 2 {
            return 0.0;
        }

        let overlap = angles_a.len().min(angles_b.len());
        let a = &angles_a[angles_a.len() - overlap..];
        let b = &angles_b[angles_b.len() - overlap..];

        let mut matches = 0;

        for i in 0..overlap {
            let diff = angular_delta(a[i], b[i]).abs();
            if diff <= threshold_rad {
                matches += 1;
            }
        }

        matches as f32 / overlap as f32
    }
}

#[derive(Debug, Clone)]
pub struct SuspicionTracker {
    pub score: u32,
}

impl SuspicionTracker {
    pub fn new() -> Self {
        Self { score: 0 }
    }

    #[inline(always)]
    pub fn add_score(&mut self, amount: u32) {
        self.score = self.score.saturating_add(amount);
    }

    #[inline(always)]
    pub fn decay(&mut self, amount: u32) {
        self.score = self.score.saturating_sub(amount);
    }

    #[inline(always)]
    pub fn is_flagged(&self, threshold: u32) -> bool {
        self.score >= threshold
    }
}

impl Default for SuspicionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// score at which a player gets reported to the log
pub const SUSPICION_FLAG_THRESHOLD: u32 = 100;

#[derive(Debug, Clone, Default)]
pub struct AimAnalysis {
    pub samples: usize,
    pub max_velocity: f32,
    pub max_jerk: f32,
    pub sign_inversion: f32,
}

/// rolling window of (timestamp_ms, aim angle) samples
#[derive(Debug, Clone)]
pub struct AimSamples {
    samples: VecDeque<(u64, f32)>,
    capacity: usize,
}

impl AimSamples {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, t_ms: u64, angle: f32) {
        if self.samples.len() >= self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back((t_ms, angle));
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn angles(&self) -> Vec<f32> {
        self.samples.iter().map(|(_, angle)| *angle).collect()
    }

    pub fn analysis(&self) -> AimAnalysis {
        let pts: Vec<(u64, f32)> = self.samples.iter().copied().collect();

        let mut velocities: Vec<f32> = Vec::with_capacity(pts.len());
        let mut dts: Vec<f32> = Vec::with_capacity(pts.len());
        for window in pts.windows(2) {
            let (t0, a0) = window[0];
            let (t1, a1) = window[1];
            let dt = ((t1 - t0) as f32 / 1000.0).max(1e-3);
            velocities.push(aimbot_anti::angular_delta(a1, a0) / dt);
            dts.push(dt);
        }

        let mut accels: Vec<f32> = Vec::with_capacity(velocities.len());
        for i in 1..velocities.len() {
            let dt = ((dts[i - 1] + dts[i]) * 0.5).max(1e-3);
            accels.push(aimbot_anti::angular_accel(
                velocities[i],
                velocities[i - 1],
                dt,
            ));
        }

        let mut max_velocity = 0.0f32;
        for &v in &velocities {
            max_velocity = max_velocity.max(v.abs());
        }

        let mut max_jerk = 0.0f32;
        for i in 1..accels.len() {
            let dt = dts[i.min(dts.len() - 1)].max(1e-3);
            let jerk = aimbot_anti::angular_jerk(accels[i], accels[i - 1], dt);
            max_jerk = max_jerk.max(jerk.abs());
        }

        AimAnalysis {
            samples: pts.len(),
            max_velocity,
            max_jerk,
            sign_inversion: aimbot_anti::sign_inversion_ratio(&velocities, 1e-3),
        }
    }
}

impl Default for AimSamples {
    fn default() -> Self {
        Self::new(64)
    }
}

/// per-connection anti-cheat state
#[derive(Default)]
pub struct PlayerWatch {
    pub tracker: SuspicionTracker,
    pub aim: AimSamples,
    pub last_upgrade_ms: u64,
    pub last_seen_ms: u64,
    pub reported: bool,
}

impl PlayerWatch {
    pub fn record_aim(&mut self, t_ms: u64, angle: f32) {
        self.aim.push(t_ms, angle);
        self.last_seen_ms = t_ms;
    }
}
