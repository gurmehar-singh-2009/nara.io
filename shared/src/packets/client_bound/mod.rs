// TODO LIST:
//
// - make add_entity and remove_entity work in bulk (ie, 1 packet will send ALL
//   player AND bullet AND shapes) for both add, remove, update

mod add_entity;
mod arena;
mod death;
mod init;
mod leaderboard;
mod player_stats;
mod remove_entity;
mod server_info;
mod spectate;
mod tank_catalog;
mod tank_info;
mod update_component;
mod update_entity;

pub use add_entity::AddEntityPacket;
pub use arena::{ArenaPacket, WallDef, ZoneDef};
pub use death::DeathPacket;
pub use init::InitPacket;
pub use leaderboard::LeaderboardPacket;
pub use player_stats::PlayerStatsPacket;
pub use remove_entity::RemoveEntityPacket;
pub use server_info::ServerInfoPacket;
pub use spectate::SpectatePacket;
pub use tank_info::TankInfoPacket;
pub use update_entity::{UpdateEntityPacket, UpdateEntityPacketData};

#[derive(bitcode::Encode, bitcode::Decode, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityType {
    Player = 0,
    Shape = 1,
    Bullet = 2,
    Drone = 3,
    Trap = 4,
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
                is_trapezoid: table.get("isTrapezoid")?,
                trap: table.get::<Option<bool>>("trap")?.unwrap_or(false),
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
    pub is_trapezoid: bool,
    pub trap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct TurretDef {
    pub angle: f32,
    pub distance: f32,
}

#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct TankSpec {
    pub name: String,
    pub health: i32,
    pub speed: f32,
    pub barrels: Vec<BarrelDef>,
}
