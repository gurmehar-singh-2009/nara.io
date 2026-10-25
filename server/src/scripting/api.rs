use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use glam::Vec2;
use mlua::{Function, Lua, Table, Value};
use shared::packets::NO_TEAM;

use crate::{
    entities::entity::{Entities, EntityId},
    game::arena::{Arena, Zone},
    scripting::userdata::player::LuaPlayer,
};

pub const TICK_RATE: f64 = 10.0;
pub const MAX_TEAMS: u8 = 4;

#[derive(Debug, Clone)]
pub enum LuaAction {
    Broadcast(String),
    Message(u32, String),
    SetTank(u32, String),
    SetLevel(u32, u32),
    AddXp(u32, u32),
    Kill(u32),
    SetTeam(u32, u8),
}

pub struct LuaBridge {
    pub players: HashMap<u32, EntityId>,
    pub bot_id_base: u32,
    pub tick: u64,
    pub actions: Vec<LuaAction>,
    pub spawned: Vec<Function>,
}

impl LuaBridge {
    pub fn new(bot_id_base: u32) -> Self {
        Self {
            players: HashMap::new(),
            bot_id_base,
            tick: 0,
            actions: Vec::new(),
            spawned: Vec::new(),
        }
    }
}

pub fn lua_team(team: u8) -> u8 {
    if team == NO_TEAM { 0 } else { team + 1 }
}

pub fn server_team(team: u8) -> u8 {
    if team == 0 { NO_TEAM } else { team - 1 }
}

pub fn bridge(lua: &Lua) -> mlua::Result<Arc<Mutex<LuaBridge>>> {
    lua.app_data_ref::<Arc<Mutex<LuaBridge>>>()
        .map(|b| b.clone())
        .ok_or_else(|| mlua::Error::runtime("lua bridge is missing"))
}

pub fn entities(lua: &Lua) -> mlua::Result<Arc<Mutex<Entities>>> {
    lua.app_data_ref::<Arc<Mutex<Entities>>>()
        .map(|e| e.clone())
        .ok_or_else(|| mlua::Error::runtime("entities are missing"))
}

pub fn arena(lua: &Lua) -> mlua::Result<Arc<Mutex<Arena>>> {
    lua.app_data_ref::<Arc<Mutex<Arena>>>()
        .map(|a| a.clone())
        .ok_or_else(|| mlua::Error::runtime("arena is missing"))
}

pub fn push_action(lua: &Lua, action: LuaAction) -> mlua::Result<()> {
    bridge(lua)?
        .lock()
        .map_err(|e| mlua::Error::external(e.to_string()))?
        .actions
        .push(action);

    Ok(())
}

pub fn entity_of(lua: &Lua, conn: u32) -> mlua::Result<Option<EntityId>> {
    Ok(bridge(lua)?
        .lock()
        .map_err(|e| mlua::Error::external(e.to_string()))?
        .players
        .get(&conn)
        .copied())
}

fn lock<T>(m: &Arc<Mutex<T>>) -> mlua::Result<std::sync::MutexGuard<'_, T>> {
    m.lock().map_err(|e| mlua::Error::external(e.to_string()))
}

pub fn register_api(lua: &Lua) -> mlua::Result<()> {
    register_arena(lua)?;
    register_teams(lua)?;
    register_players(lua)?;
    register_chat(lua)?;
    register_task(lua)?;
    register_game(lua)?;

    Ok(())
}

fn zone_from_table(t: &Table, defaults: Zone) -> mlua::Result<Zone> {
    let x: f32 = t.get("x")?;
    let y: f32 = t.get("y")?;
    let w: f32 = t.get("w")?;
    let h: f32 = t.get("h")?;
    let half = Vec2::new(w, h).abs() * 0.5;
    let team: Option<u8> = t.get("team")?;

    Ok(Zone {
        id: 0,
        min: Vec2::new(x, y) - half,
        max: Vec2::new(x, y) + half,
        team: team.map(server_team).unwrap_or(defaults.team),
        damage: t.get::<Option<f32>>("damage")?.unwrap_or(defaults.damage),
        spawn: t.get::<Option<bool>>("spawn")?.unwrap_or(defaults.spawn),
        shapes: t.get::<Option<bool>>("shapes")?.unwrap_or(defaults.shapes),
    })
}

fn register_arena(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;

    t.set(
        "setSize",
        lua.create_function(|lua, bound: f32| {
            lock(&arena(lua)?)?
                .set_bound(bound)
                .map_err(mlua::Error::runtime)
        })?,
    )?;

    t.set(
        "size",
        lua.create_function(|lua, ()| Ok(lock(&arena(lua)?)?.bound))?,
    )?;

    t.set(
        "addWall",
        lua.create_function(|lua, (x, y, w, h): (f32, f32, f32, f32)| {
            Ok(lock(&arena(lua)?)?.add_wall(Vec2::new(x, y), Vec2::new(w, h)))
        })?,
    )?;

    t.set(
        "removeWall",
        lua.create_function(|lua, id: u32| Ok(lock(&arena(lua)?)?.remove_wall(id)))?,
    )?;

    t.set(
        "clearWalls",
        lua.create_function(|lua, ()| {
            lock(&arena(lua)?)?.clear_walls();
            Ok(())
        })?,
    )?;

    t.set(
        "wallCount",
        lua.create_function(|lua, ()| Ok(lock(&arena(lua)?)?.walls().len()))?,
    )?;

    t.set(
        "addZone",
        lua.create_function(|lua, def: Table| {
            let zone = zone_from_table(
                &def,
                Zone {
                    id: 0,
                    min: Vec2::ZERO,
                    max: Vec2::ZERO,
                    team: NO_TEAM,
                    damage: 0.0,
                    spawn: false,
                    shapes: true,
                },
            )?;

            Ok(lock(&arena(lua)?)?.add_zone(zone))
        })?,
    )?;

    t.set(
        "addBase",
        lua.create_function(
            |lua, (team, x, y, w, h, damage): (u8, f32, f32, f32, f32, Option<f32>)| {
                let half = Vec2::new(w, h).abs() * 0.5;

                Ok(lock(&arena(lua)?)?.add_zone(Zone {
                    id: 0,
                    min: Vec2::new(x, y) - half,
                    max: Vec2::new(x, y) + half,
                    team: server_team(team),
                    damage: damage.unwrap_or(150.0),
                    spawn: true,
                    shapes: false,
                }))
            },
        )?,
    )?;

    t.set(
        "removeZone",
        lua.create_function(|lua, id: u32| Ok(lock(&arena(lua)?)?.remove_zone(id)))?,
    )?;

    t.set(
        "clearZones",
        lua.create_function(|lua, ()| {
            lock(&arena(lua)?)?.clear_zones();
            Ok(())
        })?,
    )?;

    t.set(
        "isBlocked",
        lua.create_function(|lua, (x, y, radius): (f32, f32, Option<f32>)| {
            Ok(lock(&arena(lua)?)?.circle_hits_wall(Vec2::new(x, y), radius.unwrap_or(1.0)))
        })?,
    )?;

    lua.globals().set("arena", t)?;

    Ok(())
}

fn register_teams(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;

    t.set(
        "setCount",
        lua.create_function(|lua, count: u8| {
            if count > MAX_TEAMS {
                return Err(mlua::Error::runtime(format!(
                    "at most {MAX_TEAMS} teams are supported"
                )));
            }

            let arena = arena(lua)?;
            let mut arena = lock(&arena)?;
            arena.team_count = count;
            arena.version += 1;

            Ok(())
        })?,
    )?;

    t.set(
        "count",
        lua.create_function(|lua, ()| Ok(lock(&arena(lua)?)?.team_count))?,
    )?;

    lua.globals().set("teams", t)?;

    Ok(())
}

fn register_players(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;

    t.set(
        "get",
        lua.create_function(|lua, id: u32| Ok(entity_of(lua, id)?.map(|_| LuaPlayer(id))))?,
    )?;

    t.set(
        "all",
        lua.create_function(|lua, ()| {
            let bridge = bridge(lua)?;
            let mut ids: Vec<u32> = lock(&bridge)?.players.keys().copied().collect();
            ids.sort_unstable();

            lua.create_sequence_from(ids.into_iter().map(LuaPlayer))
        })?,
    )?;

    t.set(
        "count",
        lua.create_function(|lua, ()| Ok(lock(&bridge(lua)?)?.players.len()))?,
    )?;

    lua.globals().set("players", t)?;

    Ok(())
}

fn register_chat(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;

    t.set(
        "broadcast",
        lua.create_function(|lua, text: String| push_action(lua, LuaAction::Broadcast(text)))?,
    )?;

    t.set(
        "send",
        lua.create_function(|lua, (id, text): (u32, String)| {
            push_action(lua, LuaAction::Message(id, text))
        })?,
    )?;

    lua.globals().set("chat", t)?;

    Ok(())
}

fn register_task(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;

    t.set(
        "spawn",
        lua.create_function(|lua, f: Function| {
            lock(&bridge(lua)?)?.spawned.push(f);
            Ok(())
        })?,
    )?;

    lua.globals().set("task", t)?;

    Ok(())
}

fn register_game(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;

    t.set("tickRate", TICK_RATE)?;
    t.set(
        "tick",
        lua.create_function(|lua, ()| Ok(lock(&bridge(lua)?)?.tick))?,
    )?;

    lua.globals().set("game", t)?;

    let shapes = lua.create_table()?;
    shapes.set(
        "setMax",
        lua.create_function(|lua, max: Option<usize>| {
            lock(&arena(lua)?)?.max_shapes = max;
            Ok(())
        })?,
    )?;

    lua.globals().set("shapes", shapes)?;

    Ok(())
}

pub fn args_table(lua: &Lua, args: &[&str]) -> mlua::Result<Table> {
    lua.create_sequence_from(args.iter().map(|a| a.to_string()))
}

pub fn value_or_nil<T: mlua::IntoLua>(lua: &Lua, v: Option<T>) -> mlua::Result<Value> {
    match v {
        Some(v) => v.into_lua(lua),
        None => Ok(Value::Nil),
    }
}
