use glam::Vec2;

// lowkey i dont know how this works because i
// copied the math from diep/arras and just merged it somehow (dieps cam good,
// arras bad)

pub struct SpringPos {
    pub pos: Vec2,
    vel: Vec2,
    smooth_time: f32,
    snap_distance: f32,
}

impl SpringPos {
    pub fn new(smooth_time: f32) -> Self {
        Self {
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            smooth_time,
            snap_distance: 1500.0,
        }
    }

    pub fn snap_to(&mut self, pos: Vec2) {
        self.pos = pos;
        self.vel = Vec2::ZERO;
    }

    pub fn update(&mut self, target: Vec2, dt: f32) -> Vec2 {
        if dt <= 0.0 {
            return self.pos;
        }

        if !self.pos.is_finite() || self.pos.distance(target) > self.snap_distance {
            self.snap_to(target);
            return self.pos;
        }

        let omega = 2.0 / self.smooth_time.max(0.0001);
        let x = omega * dt;
        let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);

        let offset = self.pos - target;
        let temp = (self.vel + offset * omega) * dt;

        self.vel = (self.vel - temp * omega) * decay;
        self.pos = target + (offset + temp) * decay;
        self.pos
    }
}

pub struct CameraController {
    pub pos: Vec2,
    pub zoom: f32,
    spring: SpringPos,
}

impl CameraController {
    const FOLLOW_TIME: f32 = 0.035;
    const ZOOM_TIME: f32 = 0.3;

    pub fn new() -> Self {
        Self {
            pos: Vec2::ZERO,
            zoom: 1.0,
            spring: SpringPos::new(Self::FOLLOW_TIME),
        }
    }

    pub fn snap_to(&mut self, pos: Vec2) {
        self.spring.snap_to(pos);
        self.pos = pos;
    }

    pub fn snap_zoom(&mut self, zoom: f32) {
        if zoom.is_finite() && zoom > 0.0 {
            self.zoom = zoom;
        }
    }

    pub fn update(&mut self, target: Vec2, dt: f32) {
        self.pos = self.spring.update(target, dt);
    }

    pub fn update_zoom(&mut self, target_zoom: f32, dt: f32) {
        if !target_zoom.is_finite() || target_zoom <= 0.0 {
            return;
        }

        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            self.zoom = target_zoom;
            return;
        }

        let t = 1.0 - (-dt / Self::ZOOM_TIME).exp();
        let current = self.zoom.ln();

        self.zoom = (current + (target_zoom.ln() - current) * t).exp();
    }
}
