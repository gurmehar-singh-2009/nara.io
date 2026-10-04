use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, with_alpha},
    },
};

const FADE_IN: f32 = 0.10;
const FADE_OUT: f32 = 0.25;

const TEAM_BLUE: [f32; 4] = [0.22, 0.55, 0.98, 1.0];

pub struct Bullet {
    pub id: u32,
    pub pos: glam::Vec2,
    pub last_pos: glam::Vec2,
    pub render_pos: glam::Vec2,
    pub vel: glam::Vec2,
    pub rot: f32,
    pub last_rot: f32,
    pub render_rot: f32,
    pub last_update_time: f64,
    pub radius: f32,
    pub recoil: u32,
    pub is_mine: bool,
    pub is_new: bool,
    pub dying: bool,
    pub render_alpha: f32,
}

impl Bullet {
    pub fn new(id: u32, x: f32, y: f32) -> Self {
        let pos = glam::Vec2::new(x, y);
        Self {
            id,
            pos,
            last_pos: pos,
            render_pos: pos,
            vel: glam::Vec2::ZERO,
            rot: 0.0,
            last_rot: 0.0,
            render_rot: 0.0,
            last_update_time: 0.0,
            radius: 8.0,
            recoil: 0,
            is_mine: false,
            is_new: true,
            dying: false,
            render_alpha: 0.0,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        if self.dying {
            let drift = self.vel * dt;
            self.pos += drift;
            self.last_pos += drift;
            self.render_pos += drift;
            self.render_alpha = (self.render_alpha - dt / FADE_OUT).max(0.0);
        } else {
            self.render_alpha = (self.render_alpha + dt / FADE_IN).min(1.0);
        }
    }
}

impl Entity for Bullet {
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        let diameter = self.radius * 2.0;

        let fill = if self.is_mine {
            TEAM_BLUE
        } else {
            DARK_THEME.team_red
        };
        let outline = DARK_THEME.outline_for(fill);

        vec![EntityInstance {
            position: [self.render_pos.x, self.render_pos.y],
            size: [diameter, diameter],
            rotation: self.render_rot,
            shape_type: 0,
            sides: 0,
            fill_color: with_alpha(fill, self.render_alpha),
            border_color: with_alpha(outline, self.render_alpha),
            border_thickness: (self.radius * 0.35).clamp(2.0, 6.0),
            extra_param: 1.0,
        }]
    }
}
