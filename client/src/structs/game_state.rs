use shared::packets::{
    NO_TEAM,
    client_bound::{WallDef, ZoneDef},
};

use crate::entities::{bullet::Bullet, drone::Drone, shape::Shape, tank::Tank};

pub struct ArenaInfo {
    pub bound: f32,
    pub team_count: u8,
    pub walls: Vec<WallDef>,
    pub zones: Vec<ZoneDef>,
}

impl Default for ArenaInfo {
    fn default() -> Self {
        Self {
            bound: 2500.0,
            team_count: 0,
            walls: Vec::new(),
            zones: Vec::new(),
        }
    }
}

pub struct DeathInfo {
    pub killer: String,
    pub killer_kind: u8,
    pub killer_tank: String,
    pub killer_team: u8,
    pub killer_sides: u32,
    pub killer_barrels: Vec<shared::packets::client_bound::BarrelDef>,
    pub score: u32,
    pub level: u32,
    pub kills: u32,
    pub time_alive: u32,
}

pub struct TankInfo {
    pub name: String,
    pub field_factor: f32,
    pub team: u8,
    pub stat_names: Vec<String>,
    pub stat_maxes: [u8; 8],
}

impl Default for TankInfo {
    fn default() -> Self {
        Self {
            name: "Tank".to_string(),
            field_factor: 1.0,
            team: NO_TEAM,
            stat_names: [
                "Health Regen",
                "Max Health",
                "Body Damage",
                "Bullet Speed",
                "Bullet Penetration",
                "Bullet Damage",
                "Reload",
                "Movement Speed",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            stat_maxes: [7; 8],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatChannel {
    Global,
    Team,
}

#[derive(Clone)]
pub struct IncomingChat {
    pub sender_id: u32,
    pub channel: ChatChannel,
    pub team: u8,
    pub sender: String,
    pub text: String,
    pub timestamp: u64,
}

pub struct GameState {
    pub my_player_id: Option<u32>,
    pub players: Vec<Tank>,
    pub shapes: Vec<Shape>,
    pub bullets: Vec<Bullet>,
    pub drones: Vec<Drone>,

    // local input, sent to the server by the socket
    pub movement_dir: Option<f32>,
    pub mouse_angle: Option<f32>,
    pub mouse_screen: Option<[u16; 4]>,
    pub auto_fire: bool,
    pub auto_fire_toggle: bool,
    pub auto_spin: bool,
    pub repel: bool,
    pub fire_held: bool,
    pub fire_clicked: bool,
    pub repel_held: bool,
    pub shift_held: bool,
    pub move_up: bool,
    pub move_down: bool,
    pub move_left: bool,
    pub move_right: bool,

    // my player's stats (PlayerStatsPacket)
    pub level: u32,
    pub score: u32,
    pub xp: u32,
    pub xp_to_next: u32,
    pub health: u32,
    pub max_health: u32,

    pub leaderboard: Vec<(String, u32)>,

    // pending requests, drained by the socket
    pub upgrade_request: Option<u8>,
    pub upgrade_levels: [u8; 8],
    pub upgrade_points: u32,

    pub chat_message: Option<String>,
    pub chat_channel: ChatChannel,
    pub incoming_chat: Vec<IncomingChat>,

    pub class_upgrades_available: bool,
    pub class_choice: Option<u8>,

    pub arena: ArenaInfo,
    pub tank_info: TankInfo,

    pub ping_ms: Option<f32>,
    pub server_load: f32,
    pub server_players: u32,
    pub server_bots: u32,
    pub server_shapes: u32,
    pub spectate: Option<glam::Vec2>,

    pub death: Option<DeathInfo>,
    pub died_at: Option<f64>,
}

impl GameState {
    pub fn tick_render(&mut self, dt: f32) {
        for p in self.players.iter_mut() {
            p.tick(dt);
        }
        for s in self.shapes.iter_mut() {
            s.tick(dt);
        }
        for b in self.bullets.iter_mut() {
            b.tick(dt);
        }
        for d in self.drones.iter_mut() {
            d.tick(dt);
        }
    }

    pub fn my_player(&self) -> Option<&Tank> {
        self.players
            .iter()
            .find(|p| Some(p.id) == self.my_player_id)
    }

    pub fn my_player_mut(&mut self) -> Option<&mut Tank> {
        self.players
            .iter_mut()
            .find(|p| Some(p.id) == self.my_player_id)
    }

    pub fn update_movement_dir(&mut self) {
        let dx = self.move_right as i32 - self.move_left as i32;
        let dy = self.move_up as i32 - self.move_down as i32;
        self.movement_dir = match (dx, dy) {
            (0, 0) => None,
            (dx, dy) => Some((dy as f32).atan2(dx as f32)),
        };
    }
}
