use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, with_alpha},
    },
};

const FADE_IN: f32 = 0.20;
const FADE_OUT: f32 = 0.30;

pub struct Shape {
    pub id: u32,
    pub pos: glam::Vec2,
    pub last_pos: glam::Vec2,
    pub render_pos: glam::Vec2,
    pub rot: f32,
    pub last_rot: f32,
    pub render_rot: f32,
    pub last_update_time: f64,
    pub sides: u32,
    pub fill_color: [f32; 4],
    pub size: f32,
    pub health: u32,
    pub max_health: u32,
    pub render_health: f32,
    pub dying: bool,
    pub render_alpha: f32,
}

impl Shape {
    pub fn new(id: u32, x: f32, y: f32, kind: u32) -> Self {
        let pos = glam::Vec2::new(x, y);
        let (sides, fill_color, size, max_health) = match kind {
            2 => (3, DARK_THEME.triangle, 40.0, 30),
            3 => (5, DARK_THEME.pentagon, 60.0, 100),
            _ => (4, DARK_THEME.square, 40.0, 10),
        };
        Self {
            id,
            pos,
            last_pos: pos,
            render_pos: pos,
            rot: 0.0,
            last_rot: 0.0,
            render_rot: 0.0,
            last_update_time: 0.0,
            sides,
            fill_color,
            size,
            health: max_health,
            max_health,
            render_health: max_health as f32,
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

impl Entity for Shape {
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        let border = DARK_THEME.outline_for(self.fill_color);

        vec![EntityInstance {
            position: [self.render_pos.x, self.render_pos.y],
            size: [self.size, self.size],
            rotation: self.render_rot,
            shape_type: 3,
            sides: self.sides,
            fill_color: with_alpha(self.fill_color, self.render_alpha),
            border_color: with_alpha(border, self.render_alpha),
            border_thickness: 3.0,
            extra_param: 1.,
        }]
    }
}
