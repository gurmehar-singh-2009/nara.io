use std::{collections::HashMap, fs};

use paris::info;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Tank {
    pub id: u32,
    pub name: String,
    pub upgrade_message: String,
    pub level_requirement: u32,
    pub upgrades: Vec<u32>,
    pub flags: TankFlags,
    pub invisibility: Option<serde_json::Value>,
    pub field_factor: f32,
    pub absorbtion_factor: f32,
    pub max_health: u32,
    pub pre_addon: u32,
    pub post_addon: u32,
    pub sides: u32,
    pub barrels: Vec<Barrel>,
    pub stats: Vec<Stat>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TankFlags {
    pub invisibility: bool,
    pub zoom_ability: bool,
    pub can_shoot: bool,
    pub dev_only: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Stat {
    pub name: String,
    pub max: u32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Barrel {
    pub angle: f64,
    pub offset: f64,
    pub size: f64,
    pub width: f64,
    pub delay: f64,
    pub reload: f64,
    pub recoil: f64,
    pub flags: BarrelFlags,
    pub trapezoid_direction: f32,
    pub addon: f32,
    pub bullet: Bullet,
    pub drone_count: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BarrelFlags {
    pub is_trapezoid: bool,
    pub force_fire: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Bullet {
    #[serde(rename = "type")]
    pub bullet_type: String,
    pub health: f64,
    pub damage: f64,
    pub speed: f64,
    pub scatter_rate: f64,
    pub life_length: f64,
    pub absorbtion_factor: f64,
    pub size_ratio: f64,
}

pub type TankTree = Vec<Vec<Tank>>;

pub fn load_tank_tree() -> TankTree {
    let json_str = fs::read_to_string("src/fs/data/tank_defs.json").unwrap();
    let tanks: Vec<Tank> = serde_json::from_str(&json_str).unwrap();

    info!("Loaded {} tanks successfully!", tanks.len());

    let tank_by_id: HashMap<u32, Tank> = tanks.into_iter().map(|tank| (tank.id, tank)).collect();

    let basic = tank_by_id
        .values()
        .find(|tank| tank.name == "Tank")
        .cloned()
        .expect("Basic tank not found");

    let mut tree: TankTree = vec![vec![basic.clone()]];
    let mut current_tier = vec![basic];

    loop {
        let mut next_tier: Vec<Tank> = Vec::new();

        for tank in &current_tier {
            for &upgrade_id in &tank.upgrades {
                let Some(upgrade) = tank_by_id.get(&upgrade_id) else {
                    continue;
                };

                if !next_tier.iter().any(|tank| tank.id == upgrade.id) {
                    next_tier.push(upgrade.clone());
                }
            }
        }

        if next_tier.is_empty() {
            break;
        }

        let tier = tree.len();

        for tank in &next_tier {
            info!("Tank tier {}: {}", tier, tank.name);
        }

        current_tier = next_tier.clone();
        tree.push(next_tier);
    }

    tree
}
