use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{theme, entity_color, with_alpha},
    },
};

const FADE_IN: f32 = 0.10;
const FADE_OUT: f32 = 0.25;

/// Represents a drone in the game for the client.
pub struct Drone {
    /// The ID of the drone.
    pub id: u32,
    /// The position of the drone.
    pub pos: glam::Vec2,
    /// The previous position of the drone.
    pub last_pos: glam::Vec2,
    /// The render position of the drone. This is the interpolated value from
    /// `last_pos` to `pos`.
    pub render_pos: glam::Vec2,
    /// The drone velocity.
    pub vel: glam::Vec2,
    /// The drone rotation.
    pub rot: f32,
    /// The previous drone rotation.
    pub last_rot: f32,
    /// The render rotation. This is the interpolated value from `last_rot` to
    /// `rot`.
    pub render_rot: f32,
    /// The time the last update occured.
    pub last_update_time: f64,
    /// The radius of the drone.
    pub radius: f32,
    /// The recoil of the parent barrel.
    pub recoil: u32,
    /// Whether or not this drone is mine.
    pub is_mine: bool,
    /// Which team this drone belongs to.
    pub team: u8,
    /// Whether or not this is a newly encountered drone.
    pub is_new: bool,
    /// Whether or not this drone is in it's dying stage.
    pub dying: bool,
    /// The render opacity of the drone. Used for the death/spawn animation.
    pub render_alpha: f32,
}

impl Drone {
    /// Creates a new `Drone`.
    ///
    /// # Arguments
    /// + `id` - the drone ID.
    /// + `x` - the x-coordinate.
    /// + `y` - the y-coordinate.
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
            dying: false,
            render_alpha: 0.0,
        }
    }

    /// Advances the bullet animation.
    ///
    /// # Arguments
    /// + `dt` - the delta time (between frames, in milliseconds).
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

impl Entity for Drone {
    /// Returns the render instance for a `Bullet`.
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        let diameter = self.radius * 2.0;

        let fill = entity_color(self.team, self.is_mine);
        let outline = theme().outline_for(fill);

        vec![EntityInstance {
            position: [self.render_pos.x, self.render_pos.y],
            size: [diameter, diameter],
            rotation: self.render_rot + std::f32::consts::PI,
            shape_type: 3,
            sides: 3,
            fill_color: with_alpha(fill, self.render_alpha),
            border_color: with_alpha(outline, self.render_alpha),
            border_thickness: (self.radius * 0.35).clamp(2.0, 6.0),
            extra_param: 1.0,
        }]
    }
}
