#[derive(Debug, Clone)]
pub struct Tank {
    pub id: u32,
    pub name: String,
    pub upgrade_message: String,
    pub level_requirement: u32,
    pub upgrades: Vec<u32>,
    pub flags: TankFlags,
    pub field_factor: f32,
    pub absorbtion_factor: f32,
    pub max_health: u32,
    pub pre_addon: u32,
    pub post_addon: u32,
    pub sides: u32,
    pub speed: f32,
    pub barrels: Vec<Barrel>,
    pub stats: Vec<Stat>,
}

#[derive(Debug, Clone)]
pub struct TankFlags {
    pub invisibility: bool,
    pub zoom_ability: bool,
    pub can_shoot: bool,
    pub dev_only: bool,
}

#[derive(Debug, Clone)]
pub struct Stat {
    pub name: String,
    pub max: u32,
}

#[derive(Debug, Clone)]
pub struct Barrel {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub width: f32,
    pub length: f32,
    pub delay: f32,
    pub reload: f32,
    pub recoil: f32,
    pub is_trapezoid: bool,
    pub trapezoid_direction: f32,
    pub addon: f32,
    pub bullet: Bullet,
    pub drone_count: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Bullet {
    pub bullet_type: String,
    pub health: f32,
    pub damage: f32,
    pub speed: f32,
    pub scatter_rate: f32,
    pub life_length: f32,
    pub absorbtion_factor: f32,
    pub size_ratio: f32,
}

pub type TankTree = Vec<Vec<Tank>>;
