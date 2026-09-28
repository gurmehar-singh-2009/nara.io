use crate::entities::{bullet::Bullet, shape::Shape, tank::Tank};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatChannel {
    Global,
    Team,
}

#[derive(Clone)]
pub struct IncomingChat {
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

    // local input, sent to the server by the socket
    pub movement_dir: Option<f32>,
    pub mouse_angle: Option<f32>,
    pub auto_fire: bool,
    pub move_up: bool,
    pub move_down: bool,
    pub move_left: bool,
    pub move_right: bool,

    // my player's stats (PlayerStatsPacket)
    pub level: u32,
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
