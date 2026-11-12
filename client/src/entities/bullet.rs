use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{theme, entity_color, with_alpha},
    },
};

const FADE_IN: f32 = 0.10;
const FADE_OUT: f32 = 0.25;
const DEATH_GROWTH: f32 = 0.5;
const TRAP_SCALE: f32 = 1.6;

/// Represents a bullet in the game for the client.
pub struct Bullet {
    /// The ID of the bullet.
    pub id: u32,
    /// The position of the bullet.
    pub pos: glam::Vec2,
    /// The previous position of the bullet.
    pub last_pos: glam::Vec2,
    /// The render position of the bullet. This is the interpolated value from
    /// `last_pos` to `pos`.
    pub render_pos: glam::Vec2,
    /// The bullet velocity.
    pub vel: glam::Vec2,
    /// The bullet rotation.
    pub rot: f32,
    /// The previous bullet rotation.
    pub last_rot: f32,
    /// The render rotation. This is the interpolated value from `last_rot` to
    /// `rot`.
    pub render_rot: f32,
    /// The time the last update occured.
    pub last_update_time: f64,
    /// The radius of the bullet.
    pub radius: f32,
    /// The recoil of the parent barrel.
    pub recoil: u32,
    /// Whether or not this bullet is mine.
    pub is_mine: bool,
    /// Which team this bullet belongs to.
    pub team: u8,
    /// Whether or not this a newly encountered bullet.
    pub is_new: bool,
    /// Whether or not this is a trap.
    pub trap: bool,
    /// Whether or not this is a dead bullet.
    pub dead: bool,
    /// Whether or not this bullet is in it's dying stage. Used for fade
    /// animation.
    pub dying: bool,
    /// The render opacity of the bullet. Used for the death/spawn animation.
    pub render_alpha: f32,
    /// For our own bullets: how far our predicted tank was ahead of where the server fired
    /// from, so the bullet leaves the barrel we see. Eases out as the bullet flies.
    pub lead: Option<(glam::Vec2, f64)>,
}

/// How long (seconds) a bullet takes to ease from our barrel onto its real path.
const LEAD_BLEND: f32 = 0.35;

impl Bullet {
    /// Records the lead to draw this bullet with, once, when we first see it.
    pub fn set_lead_once(&mut self, lead: glam::Vec2, now: f64) {
        if self.lead.is_none() {
            self.lead = Some((lead, now));
        }
    }

    /// How far from its real position to draw the bullet at `now`.
    pub fn lead_offset(&self, now: f64) -> glam::Vec2 {
        let Some((lead, born)) = self.lead else {
            return glam::Vec2::ZERO;
        };

        let t = (((now - born) / 1000.0) as f32 / LEAD_BLEND).clamp(0.0, 1.0);

        // smoothstep: leaves the barrel straight, then eases onto the real path
        lead * (1.0 - t * t * (3.0 - 2.0 * t))
    }
}

impl Bullet {
    /// Creates a new `Bullet`.
    ///
    /// # Arguments
    /// + `id` - the bullet ID.
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
            trap: false,
            dead: false,
            dying: false,
            render_alpha: 0.0,
            lead: None,
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

impl Entity for Bullet {
    /// Returns the render instance for a `Bullet`.
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        let pop = if self.dying {
            1.0 + (1.0 - self.render_alpha) * DEATH_GROWTH
        } else {
            1.0
        };
        let diameter = self.radius * 2.0 * pop;

        let fill = entity_color(self.team, self.is_mine);
        let outline = theme().outline_for(fill);

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
