mod ai;
mod combat;
mod navigation;
mod perception;
mod spawning;
mod steering;

pub use ai::tick_ai;
use glam::Vec2;
use navigation::DStarLite;
pub use navigation::{NAV_LEN, NAV_REBUILD_INTERVAL, rebuild_nav_grid};
pub use perception::is_bot_id;
pub use spawning::{spend_stat_points, tick as tick_spawns};

use crate::entities::entity::EntityId;

pub struct BotBrain {
    pub entity: EntityId,

    pub wander_target: Vec2,
    pub next_wander_tick: u64,

    pub target: Option<EntityId>,
    pub target_is_tank: bool,
    pub target_is_bot: bool,
    pub target_last_pos: Vec2,
    pub target_last_tick: u64,
    pub target_vel: Vec2,
    pub farm_ram: bool,

    pub flee_dir: Vec2,
    pub flee_from: Option<Vec2>,

    pub aggro_on: Option<u32>,
    pub aggro_tick: u64,
    pub force_macro: bool,

    pub react_until_tick: u64,

    pub planner: DStarLite,
    pub nav_version_seen: u64,
    pub path: Vec<Vec2>,
    pub path_goal: Vec2,
    pub next_repath_tick: u64,

    pub combat_move: Option<f32>,
    pub combat_fire: bool,

    pub move_vec: Vec2,

    pub skill: f32,
    pub aim_phase: f32,
}
