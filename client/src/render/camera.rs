use glam::Vec2;

// lowkey i dont know how this works because i
// copied the math from diep/arras and just merged it somehow (dieps cam good,
// arras bad)

pub struct SpringPos {
    pub pos: Vec2,
    vel: Vec2,
    smooth_time: f32,
    max_speed: f32,
    snap_distance: f32,
}

impl SpringPos {
    pub fn new(smooth_time: f32) -> Self {
        Self {
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            smooth_time,
            max_speed: 6000.0,
            snap_distance: 1500.0,
        }
    }

    pub fn snap_to(&mut self, pos: Vec2) {
        self.pos = pos;
        self.vel = Vec2::ZERO;
    }

    pub fn update(&mut self, target: Vec2, dt: f32) -> Vec2 {
        if self.pos.distance(target) > self.snap_distance {
            self.snap_to(target);
            return self.pos;
        }

        let mut change = self.pos - target;
        let max_change = self.max_speed * self.smooth_time;
        let len = change.length();
        if len > max_change && len > 0.0 {
            change *= max_change / len;
        }
        let clamped_target = self.pos - change;

        let omega = 2.0 / self.smooth_time.max(0.0001);
        let x = omega * dt;
        let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);

        let temp = (self.vel + omega * change) * dt;
        self.vel = (self.vel - omega * temp) * exp;
        self.pos = clamped_target + (change + temp) * exp;
        self.pos
    }
}

pub struct CameraController {
    pub pos: Vec2,
    pub zoom: f32,
    spring: SpringPos,
}

impl CameraController {
    const CAMERA_SMOOTH_TIME: f32 = 0.10;
    const ZOOM_SMOOTH_SPEED: f32 = 8.0;

    pub fn new() -> Self {
        Self {
            pos: Vec2::ZERO,
            zoom: 1.0,
            spring: SpringPos::new(Self::CAMERA_SMOOTH_TIME),
        }
    }

    pub fn snap_to(&mut self, pos: Vec2) {
        self.spring.snap_to(pos);
        self.pos = pos;
    }

    pub fn update(&mut self, target: Vec2, dt: f32) {
        self.pos = self.spring.update(target, dt);
    }

    pub fn update_zoom(&mut self, target_zoom: f32, dt: f32) {
        let factor = 1.0 - (-Self::ZOOM_SMOOTH_SPEED * dt).exp();
        self.zoom += (target_zoom - self.zoom) * factor;
    }
}
