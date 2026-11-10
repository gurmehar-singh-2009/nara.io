use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{theme, with_alpha},
    },
};

const FADE_IN: f32 = 0.20;
const FADE_OUT: f32 = 0.30;

/// Represents a shape in the game for the client.
pub struct Shape {
    /// The ID of the shape.
    pub id: u32,
    /// The position of the shape.
    pub pos: glam::Vec2,
    /// THe previous position of the shape.
    pub last_pos: glam::Vec2,
    /// The render position of the shape. This is the interpolated value from
    /// `last_pos` to `pos`.
    pub render_pos: glam::Vec2,
    /// The shape rotation.
    pub rot: f32,
    /// The previous shape rotation.
    pub last_rot: f32,
    /// The render rotation. This is the interpolated value from `last_rot` to
    /// `rot`.
    pub render_rot: f32,
    /// The time the last update occured.
    pub last_update_time: f64,
    /// The number of sides the shape has.
    pub sides: u32,
    /// The fill color of the shape.
    pub fill_color: [f32; 4],
    /// The size of the shape.
    pub size: f32,
    /// The health of the shape.
    pub health: u32,
    /// The maximum health of the shape.
    pub max_health: u32,
    /// The render health.
    pub render_health: f32,
    // Whether or not the shape is in the dying stage.
    pub dying: bool,
    /// The render opacity of the shape. Used for the death/spawn animation.
    pub render_alpha: f32,
    /// The health bar opacity. 0 when full health (hidden).
    pub health_bar_alpha: f32,
}

impl Shape {
    /// Creates a new `Shape`.
    ///
    /// # Arguments
    /// + `id` - the shape ID.
    /// + `x` - the x-coordinate.
    /// + `y` - the y-coordinate.
    pub fn new(id: u32, x: f32, y: f32, kind: u32) -> Self {
        let pos = glam::Vec2::new(x, y);
        let (sides, fill_color, size, max_health) = match kind {
            2 => (3, theme().triangle, 40.0, 30),
            3 => (5, theme().pentagon, 60.0, 100),
            _ => (4, theme().square, 40.0, 10),
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
            health_bar_alpha: 0.0,
        }
    }

    /// Advances the shape animation.
    ///
    /// # Arguments
    /// + `dt` - the delta time (between frames, in milliseconds).
    pub fn tick(&mut self, dt: f32) {
        if self.dying {
            self.render_alpha = (self.render_alpha - dt / FADE_OUT).max(0.0);
        } else {
            self.render_alpha = (self.render_alpha + dt / FADE_IN).min(1.0);
        }
    }
}

impl Entity for Shape {
    /// Returns the render instance for a `Shape`.
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        let border = theme().outline_for(self.fill_color);

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
