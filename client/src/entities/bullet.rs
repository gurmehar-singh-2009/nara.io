use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, entity_color, with_alpha},
    },
};

const FADE_IN: f32 = 0.10;
const FADE_OUT: f32 = 0.25;
const DEATH_GROWTH: f32 = 0.5;
const TRAP_SCALE: f32 = 1.6;

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
    pub team: u8,
    pub is_new: bool,
    pub trap: bool,
    pub dead: bool,
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
            team: shared::packets::NO_TEAM,
            is_new: true,
            trap: false,
            dead: false,
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
        let pop = if self.dying {
            1.0 + (1.0 - self.render_alpha) * DEATH_GROWTH
        } else {
            1.0
        };
        let diameter = self.radius * 2.0 * pop;

        let fill = entity_color(self.team, self.is_mine);
        let outline = DARK_THEME.outline_for(fill);

        let (size, shape_type, sides) = if self.trap {
            (diameter * TRAP_SCALE, 3, 3)
        } else {
            (diameter, 0, 0)
        };

        vec![EntityInstance {
            position: [self.render_pos.x, self.render_pos.y],
            size: [size, size],
            rotation: self.render_rot,
            shape_type,
            sides,
            fill_color: with_alpha(fill, self.render_alpha),
            border_color: with_alpha(outline, self.render_alpha),
            border_thickness: (self.radius * 0.35).clamp(2.0, 6.0),
            extra_param: 1.0,
        }]
    }
}
