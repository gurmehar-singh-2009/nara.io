// TODO LIST:
//
// - make add_entity and remove_entity work in bulk (ie, 1 packet will send ALL
//   player AND bullet AND shapes) for both add, remove, update

mod add_entity;
mod leaderboard;
mod player_stats;
mod remove_entity;
mod tank_catalog;
mod update_component;
mod update_entity;

pub use add_entity::AddEntityPacket;
pub use leaderboard::LeaderboardPacket;
pub use player_stats::PlayerStatsPacket;
pub use remove_entity::RemoveEntityPacket;
pub use update_entity::{UpdateEntityPacket, UpdateEntityPacketData};

#[derive(bitcode::Encode, bitcode::Decode, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityType {
    Player = 0,
    Shape = 1,
    Bullet = 2,
}

#[cfg(feature = "lua")]
mod lua_impls {
    use mlua::{FromLua, Lua, Table, Value};

    use super::BarrelDef;

    impl FromLua for BarrelDef {
        fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
            let table = Table::from_lua(value, lua)?;
            Ok(BarrelDef {
                x: table.get("x")?,
                y: table.get("y")?,
                angle: table.get("angle")?,
                width: table.get("width")?,
                length: table.get("length")?,
            })
        }
    }
}

#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct BarrelDef {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub width: f32,
    pub length: f32,
}

#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct TankSpec {
    pub name: String,
    pub health: i32,
    pub speed: f32,
    pub barrels: Vec<BarrelDef>,
}
