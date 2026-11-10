use std::f32::consts::FRAC_PI_2;

use shared::packets::client_bound::{BarrelDef, TurretDef};

use crate::{
    entities::Entity,
    render::{
        buffers::EntityInstance,
        colours::{Color, theme, entity_color, with_alpha},
    },
};

const FADE_IN: f32 = 0.25;
const FADE_OUT: f32 = 0.30;
const RECOIL_KICK_FRAC: f32 = 0.18;
const RECOIL_TIME: f32 = 0.12;
const TRAPEZOID_TAPER: f32 = 0.75;
const TRAPEZOID_WIDTH_SCALE: f32 = 1.75;
const TRAP_FLARE_LENGTH: f32 = 0.3;
const TRAP_FLARE_WIDTH: f32 = 1.7;
const TRAP_FLARE_TAPER: f32 = 0.6;
const TURRET_BASE: f32 = 0.5;
const TURRET_BARREL_LENGTH: f32 = 1.15;
const TURRET_BARREL_WIDTH: f32 = 0.5;

/// Represents a tank in the game for the client.
pub struct Tank {
    /// The ID of the tank.
    pub id: u32,
    /// The name of the tank.
    pub name: String,
    /// The position of the tank.
    pub pos: glam::Vec2,
    /// The previous position of the tank.
    pub last_pos: glam::Vec2,
    /// The render position of the tank. This is the interpolated value from
    /// `last_pos` to `pos`.
    pub render_pos: glam::Vec2,
    /// The rotation of the tank.
    pub rot: f32,
    /// The previous rotation of the tank.
    pub last_rot: f32,
    /// The render rotation. This is the interpolated value from
    /// `last_rot` to `rot`.
    pub render_rot: f32,
    /// The time the last update occured.
    pub last_update_time: f64,
    /// The size of the tank.
    pub scale: f32,
    /// The health of the tank.
    pub health: u32,
    /// The maximum health of the tank.
    pub max_health: u32,
    /// The render health.
    pub render_health: f32,
    /// The health bar opacity. 0 when full health (hidden).
    pub health_bar_alpha: f32,
    /// Whether or not the tank is mine.
    pub is_mine: bool,
    /// Which team the tank belongs to.
    pub team: u8,
    /// Whether or not the tank is in it's dying stage.
    pub dying: bool,
    /// The render opacity of the tank. Used for the death/spawn animation.
    pub render_alpha: f32,
    /// The barrels of the tank.
    /// Contains `BarrelDef`.
    pub barrels: Vec<BarrelDef>,
    /// The barrel recoils of the tank.
    pub barrel_recoils: Vec<f32>,
    /// The auto turrets on the tank.
    /// Contains `TurretDef`.
    pub turrets: Vec<TurretDef>,
    /// The auto turret aim directions on the tank.
    pub turret_aims: Vec<f32>,
    /// The previous auto turret aim directions.
    pub last_turret_aims: Vec<f32>,
    /// The render auto turret aim directions. Interpolated from
    /// `last_turret_aims` to `turret_aims`.
    pub render_turret_aims: Vec<f32>,
}

impl Tank {
    /// Creates a new `Tank`.
    ///
    /// # Arguments
    /// + `id` - the shape ID.
    /// + `name` - the tank name.
    /// + `pos` - the vector position.
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
            health_bar_alpha: 0.0,
            is_mine: false,
            team: shared::packets::NO_TEAM,
            dying: false,
            render_alpha: 0.0,
            barrels: vec![],
            barrel_recoils: vec![],
            turrets: vec![],
            turret_aims: vec![],
            last_turret_aims: vec![],
            render_turret_aims: vec![],
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

        self.barrel_recoils.resize(self.barrels.len(), 0.0);
        for r in self.barrel_recoils.iter_mut() {
            *r = (*r - dt / RECOIL_TIME).max(0.0);
        }
    }

    /// Kicks the specific barrel back.
    /// Used for recoil animation.
    ///
    /// # Arguments
    /// + `idx` - the barrel index in the `barrels` attribute.
    pub fn kick_barrel(&mut self, idx: usize) {
        if idx < self.barrel_recoils.len() {
            self.barrel_recoils[idx] = 1.0;
        }
    }
}

pub struct TankDraw<'a> {
    pub pos: glam::Vec2,
    pub rot: f32,
    pub scale: f32,
    pub barrels: &'a [BarrelDef],
    pub recoils: &'a [f32],
    pub turrets: &'a [TurretDef],
    pub turret_aims: &'a [f32],
    pub sides: u32,
    pub body: Color,
    pub alpha: f32,
}

pub fn tank_instances(draw: &TankDraw) -> (Vec<EntityInstance>, usize) {
    let mut instances = Vec::new();
    let scale = draw.scale;
    let barrel_fill = with_alpha(theme().barrel, draw.alpha);
    let barrel_edge = with_alpha(theme().outline_for(theme().barrel), draw.alpha);
    let border = 3.0 * scale;

    for (i, barrel) in draw.barrels.iter().enumerate() {
        let barrel_angle = barrel.angle.to_radians();
        let world_angle = draw.rot + barrel_angle;
        let dir = glam::Vec2::from_angle(world_angle);

        let recoil = draw.recoils.get(i).copied().unwrap_or(0.0);
        let kick = recoil * barrel.length * scale * RECOIL_KICK_FRAC;

        let local_base = glam::Vec2::new(barrel.x, barrel.y) * scale;
        let base_pos = draw.pos + glam::Vec2::from_angle(draw.rot).rotate(local_base);

        let length = barrel.length * scale;
        let width = barrel.width * scale;
        let center = base_pos + dir * (length * 0.5 - kick);

        if barrel.is_trapezoid {
            instances.push(EntityInstance {
                position: [center.x, center.y],
                size: [width * TRAPEZOID_WIDTH_SCALE, length * 0.9],
                rotation: world_angle + FRAC_PI_2,
                shape_type: 8,
                sides: 0,
                fill_color: barrel_fill,
                border_color: barrel_edge,
                border_thickness: border,
                extra_param: TRAPEZOID_TAPER,
            });
            continue;
        }

        instances.push(EntityInstance {
            position: [center.x, center.y],
            size: [length, width],
            rotation: world_angle,
            shape_type: 1,
            sides: 4,
            fill_color: barrel_fill,
            border_color: barrel_edge,
            border_thickness: border,
            extra_param: 1.0,
        });

        if barrel.trap {
            let flare_len = length * TRAP_FLARE_LENGTH;
            let tip = base_pos + dir * (length - kick + flare_len * 0.5);

            instances.push(EntityInstance {
                position: [tip.x, tip.y],
                size: [width * TRAP_FLARE_WIDTH, flare_len],
                rotation: world_angle + FRAC_PI_2,
                shape_type: 8,
                sides: 0,
                fill_color: barrel_fill,
                border_color: barrel_edge,
                border_thickness: border,
                extra_param: TRAP_FLARE_TAPER,
            });
        }
    }

    let body_size = 42.0 * scale;
    let (shape_type, sides) = if draw.sides >= 3 {
        (3, draw.sides)
    } else {
        (0, 0)
    };

    let body_index = instances.len();

    instances.push(EntityInstance {
        position: [draw.pos.x, draw.pos.y],
        size: [body_size, body_size],
        rotation: draw.rot,
        shape_type,
        sides,
        fill_color: with_alpha(draw.body, draw.alpha),
        border_color: with_alpha(theme().outline_for(draw.body), draw.alpha),
        border_thickness: border,
        extra_param: 1.0,
    });

    let radius = body_size * 0.5;

    for (i, mount) in draw.turrets.iter().enumerate() {
        let mount_pos = draw.pos
            + glam::Vec2::from_angle(draw.rot + mount.angle.to_radians()) * mount.distance * radius;
        let aim = draw
            .turret_aims
            .get(i)
            .copied()
            .unwrap_or(draw.rot + mount.angle.to_radians());
        let length = radius * TURRET_BARREL_LENGTH;
        let barrel_center = mount_pos + glam::Vec2::from_angle(aim) * (length * 0.5);

        instances.push(EntityInstance {
            position: [barrel_center.x, barrel_center.y],
            size: [length, radius * TURRET_BARREL_WIDTH],
            rotation: aim,
            shape_type: 1,
            sides: 4,
            fill_color: barrel_fill,
            border_color: barrel_edge,
            border_thickness: border,
            extra_param: 1.0,
        });

        let base = radius * TURRET_BASE * 2.0;

        instances.push(EntityInstance {
            position: [mount_pos.x, mount_pos.y],
            size: [base, base],
            rotation: aim,
            shape_type: 0,
            sides: 0,
            fill_color: barrel_fill,
            border_color: barrel_edge,
            border_thickness: border,
            extra_param: 1.0,
        });
    }

    (instances, body_index)
}

impl Tank {
    pub fn render_with_body(&self) -> (Vec<EntityInstance>, usize) {
        tank_instances(&TankDraw {
            pos: self.render_pos,
            rot: self.render_rot,
            scale: self.scale,
            barrels: &self.barrels,
            recoils: &self.barrel_recoils,
            turrets: &self.turrets,
            turret_aims: &self.render_turret_aims,
            sides: 0,
            body: entity_color(self.team, self.is_mine),
            alpha: self.render_alpha,
        })
    }
}

impl Entity for Tank {
    fn get_render_instances(&self) -> Vec<EntityInstance> {
        self.render_with_body().0
    }
}
