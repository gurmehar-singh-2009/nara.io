use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use mlua::{Function, Lua};
use paris::error;

use crate::{
    fs::tank_defs::{Tank, TankTree},
    scripting::loader::{TankDef, WeaponDef, load_tank, load_weapon},
};

#[derive(Default)]
pub struct WeaponRegistry(std::collections::HashMap<String, WeaponDef>);

impl WeaponRegistry {
    pub fn load_all(lua: &mlua::Lua, dir: &str) -> mlua::Result<Self> {
        let mut map = std::collections::HashMap::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            map.insert(name, load_weapon(lua, path.to_str().unwrap())?);
        }
        Ok(Self(map))
    }
}

pub fn register_commands(lua: &Lua) -> mlua::Result<Arc<Mutex<HashMap<String, Function>>>> {
    let commands: Arc<Mutex<HashMap<String, Function>>> = Arc::new(Mutex::new(HashMap::new()));

    let reg = commands.clone();
    let register_fn = lua.create_function(move |_, (name, func): (String, Function)| {
        reg.lock()
            .map_err(|e| mlua::Error::external(e.to_string()))?
            .insert(name, func);
        // reg.borrow_mut().insert(name, func);
        Ok(())
    })?;

    let table = lua.create_table()?;

    table.set("register", register_fn)?;
    lua.globals().set("commands", table)?;

    Ok(commands)
}

pub fn register_events(lua: &Lua) -> mlua::Result<()> {
    let listeners_table = lua.create_table()?;
    lua.set_named_registry_value("EVENT_LISTENERS", listeners_table)?;

    let events_table = lua.create_table()?;

    let on_fn = lua.create_function(|lua, (event_name, func): (String, Function)| {
        let listeners: mlua::Table = lua.named_registry_value("EVENT_LISTENERS")?;

        let list: mlua::Table = match listeners.get(&*event_name)? {
            mlua::Value::Table(t) => t,
            _ => {
                let new_table = lua.create_table()?;
                listeners.set(event_name.as_str(), new_table.clone())?;
                new_table
            }
        };

        list.push(func)?;

        Ok(())
    })?;

    events_table.set("on", on_fn)?;
    lua.globals().set("events", events_table)?;

    Ok(())
}

#[derive(Default)]
pub struct TankRegistry(pub HashMap<String, TankDef>);

impl TankRegistry {
    pub fn load_all(lua: &Lua, dir: &str) -> mlua::Result<Self> {
        let mut map = HashMap::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lua") {
                continue;
            }
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            map.insert(name, load_tank(lua, path.to_str().unwrap())?);
        }
        Ok(Self(map))
    }

    fn resolved(&self, key: &str) -> Option<Tank> {
        let def = self.0.get(key)?;
        let mut tank = def.tank.clone();
        tank.upgrades = def
            .upgrade_names
            .iter()
            .filter_map(|name| match self.0.get(name) {
                Some(target) => Some(target.tank.id),
                None => {
                    error!("tank \"{key}\" references unknown upgrade \"{name}\"");
                    None
                }
            })
            .collect();
        Some(tank)
    }

    pub fn build_tree(&self) -> mlua::Result<TankTree> {
        let basic_key = self
            .0
            .iter()
            .find(|(_, def)| def.tank.name == "Tank")
            .map(|(key, _)| key.clone())
            .ok_or_else(|| {
                mlua::Error::runtime(
                    "basic tank (name = \"Tank\") not found in content/tanks  \
                     run `cargo run --bin gen_tanks` first",
                )
            })?;

        let basic = self
            .resolved(&basic_key)
            .ok_or_else(|| mlua::Error::runtime("failed to resolve basic tank"))?;

        let mut tree: TankTree = vec![vec![basic]];
        let mut visited: HashSet<u32> = tree[0].iter().map(|t| t.id).collect();
        let mut current = vec![basic_key];

        loop {
            let mut next: Vec<String> = Vec::new();

            for key in &current {
                let Some(def) = self.0.get(key) else {
                    continue;
                };
                for up in &def.upgrade_names {
                    let Some(target) = self.0.get(up) else {
                        continue;
                    };
                    if visited.contains(&target.tank.id) {
                        continue;
                    }
                    if next.iter().any(|k| k == up) {
                        continue;
                    }
                    visited.insert(target.tank.id);
                    next.push(up.clone());
                }
            }

            if next.is_empty() {
                break;
            }

            let tier: Vec<Tank> = next.iter().filter_map(|k| self.resolved(k)).collect();
            tree.push(tier);
            current = next;
        }

        Ok(tree)
    }
}
