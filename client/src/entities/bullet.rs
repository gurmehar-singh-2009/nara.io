use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, with_alpha},
    },
};

const FADE_IN: f32 = 0.08;
const FADE_OUT: f32 = 0.12;

pub struct Bullet {
    pub id: u32,
    pub pos: glam::Vec2,
    pub last_pos: glam::Vec2,
    pub render_pos: glam::Vec2,
    pub rot: f32,
    pub last_rot: f32,
    pub render_rot: f32,
    pub last_update_time: f64,
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
            rot: 0.0,
            last_rot: 0.0,
            render_rot: 0.0,
            last_update_time: 0.0,
            dying: false,
            render_alpha: 0.0,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        if self.dying {
            self.render_alpha = (self.render_alpha - dt / FADE_OUT).max(0.0);
        } else {
            self.render_alpha = (self.render_alpha + dt / FADE_IN).min(1.0);
        }
    }
}

impl Entity for Bullet {
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        vec![EntityInstance {
            position: [self.render_pos.x, self.render_pos.y],
            size: [20., 20.],
            rotation: self.render_rot,
            shape_type: 0,
            sides: 0,
            fill_color: with_alpha(DARK_THEME.bullet, self.render_alpha),
            border_color: with_alpha(DARK_THEME.tank_outline, self.render_alpha),
            border_thickness: 2.,
            extra_param: 1.,
        }]
    }
}
