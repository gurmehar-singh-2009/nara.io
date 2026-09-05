use shared::packets::client_bound::BarrelDef;

use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, with_alpha},
    },
};

const FADE_IN: f32 = 0.25;
const FADE_OUT: f32 = 0.30;

pub struct Tank {
    pub id: u32,
    pub name: String,

    pub pos: glam::Vec2,
    pub last_pos: glam::Vec2,
    pub render_pos: glam::Vec2,

    pub rot: f32,
    pub last_rot: f32,
    pub render_rot: f32,

    pub last_update_time: f64,

    pub scale: f32,

    pub health: u32,
    pub max_health: u32,

    pub render_health: f32,

    pub dying: bool,
    pub render_alpha: f32,

    pub barrels: Vec<BarrelDef>,
}

impl Tank {
    pub fn new(id: u32, name: String, pos: glam::Vec2) -> Self {
        Self {
            id,
            name,

            pos,
            last_pos: pos,
            render_pos: pos,

            rot: 0.0,
            last_rot: 0.0,
            render_rot: 0.0,

            last_update_time: 0.0,

            scale: 1.0,

            health: 100,
            max_health: 100,

            render_health: 100.0,

            dying: false,
            render_alpha: 0.0,

            barrels: vec![],
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

impl Entity for Tank {
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        let mut instances = Vec::new();

        for barrel in &self.barrels {
            let barrel_angle = barrel.angle.to_radians();

            let world_angle = self.render_rot + barrel_angle;

            let local_base = glam::Vec2::new(barrel.x * self.scale, barrel.y * self.scale);

            let base_offset = glam::Vec2::from_angle(self.render_rot).rotate(local_base);

            let base_pos = self.render_pos + base_offset;

            let center_offset =
                glam::Vec2::from_angle(world_angle) * (barrel.length * self.scale * 0.5);

            let world_pos = base_pos + center_offset;

            instances.push(EntityInstance {
                position: [world_pos.x, world_pos.y],

                size: [barrel.length * self.scale, barrel.width * self.scale],

                rotation: world_angle,

                shape_type: 1,
                sides: 4,

                fill_color: with_alpha(DARK_THEME.barrel, self.render_alpha),

                border_color: with_alpha(DARK_THEME.barrel_outline, self.render_alpha),

                border_thickness: 3.0 * self.scale,

                extra_param: 1.0,
            });
        }

        let size = 42.0 * self.scale;

        instances.push(EntityInstance {
            position: [self.render_pos.x, self.render_pos.y],

            size: [size, size],

            rotation: self.render_rot,

            shape_type: 0,
            sides: 0,

            fill_color: with_alpha(DARK_THEME.tank_body, self.render_alpha),

            border_color: with_alpha(DARK_THEME.tank_outline, self.render_alpha),

            border_thickness: 3.0 * self.scale,

            extra_param: 1.0,
        });

        instances
    }
}
