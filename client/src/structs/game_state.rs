use crate::entities::{bullet::Bullet, square::Shape, tank::Tank};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

    pub movement_dir: Option<f32>,
    pub mouse_angle: Option<f32>,
    pub auto_fire: bool,

    pub move_up: bool,
    pub move_down: bool,
    pub move_left: bool,
    pub move_right: bool,

    pub level: u32,
    pub xp: u32,
    pub xp_to_next: u32,
    pub health: u32,
    pub max_health: u32,

    pub leaderboard: Vec<(String, u32)>,

    pub upgrade_request: Option<u8>,
    pub upgrade_levels: [u8; 8],

    pub chat_message: Option<String>,
    pub chat_channel: ChatChannel,
    pub incoming_chat: Vec<IncomingChat>,

    pub class_upgrades_available: bool,
    pub class_choice: Option<u8>,
}

impl GameState {
    pub fn my_player(&self) -> Option<&Tank> {
        let id = self.my_player_id?;
        self.players.iter().find(|t| t.id == id)
    }

    pub fn my_player_mut(&mut self) -> Option<&mut Tank> {
        let id = self.my_player_id?;
        self.players.iter_mut().find(|t| t.id == id)
    }

    pub fn update_movement_dir(&mut self) {
        let x = self.move_right as i8 - self.move_left as i8;
        let y = self.move_up as i8 - self.move_down as i8;

        if x == 0 && y == 0 {
            self.movement_dir = None;
            return;
        }

        let direction = glam::Vec2::new(x as f32, y as f32);
        self.movement_dir = Some(direction.y.atan2(direction.x));
    }

    pub fn tick_render(&mut self, dt: f32) {
        for t in &mut self.players {
            t.tick(dt);
        }
        for s in &mut self.shapes {
            s.tick(dt);
        }
        for b in &mut self.bullets {
            b.tick(dt);
        }

        self.players.retain(|p| !(p.dying && p.render_alpha <= 0.0));
        self.shapes.retain(|s| !(s.dying && s.render_alpha <= 0.0));
        self.bullets.retain(|b| !(b.dying && b.render_alpha <= 0.0));
    }
}
