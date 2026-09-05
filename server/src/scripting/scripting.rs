use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use mlua::{Function, Lua};

use crate::{
    entities::entity::Entities,
    game::{game_state::GameEvents, scheduler::Scheduler},
    scripting::registery::{TankRegistry, WeaponRegistry, register_commands, register_events},
};

const PRELUDE: &str = "function wait(seconds) return coroutine.yield(seconds) end";

pub struct Scripting {
    pub lua: Lua,
    pub entities: Arc<Mutex<Entities>>,
    pub weapons: WeaponRegistry,
    pub tanks: TankRegistry,
    pub abilities: Option<()>,
    pub commands: Arc<Mutex<HashMap<String, Function>>>,
    pub scheduler: Scheduler,
}

impl Scripting {
    pub fn new(entities: Entities) -> mlua::Result<Self> {
        let lua = Lua::new();

        let entities = Arc::new(Mutex::new(entities));
        lua.set_app_data(entities.clone());

        lua.load(PRELUDE).exec()?;

        let commands = register_commands(&lua)?;
        register_events(&lua)?;

        let weapons = WeaponRegistry::default();
        let tanks = TankRegistry::default();

        Ok(Self {
            lua,
            entities,
            weapons,
            tanks,
            abilities: None,
            commands,
            scheduler: Scheduler::default(),
        })
    }

    pub fn load_scripts(&mut self, dir: &str) -> mlua::Result<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("lua"))
                .collect();
            paths.sort();

            for path in paths {
                let src = std::fs::read_to_string(&path).map_err(mlua::Error::external)?;
                let name = path.to_string_lossy().to_string();
                self.lua.load(&src).set_name(name.as_str()).exec()?;
            }
        }

        let weapons_dir = format!("{dir}/weapons");
        if std::path::Path::new(&weapons_dir).is_dir() {
            self.weapons = WeaponRegistry::load_all(&self.lua, &weapons_dir)?;
        }
        let tanks_dir = format!("{dir}/tanks");
        if std::path::Path::new(&tanks_dir).is_dir() {
            self.tanks = TankRegistry::load_all(&self.lua, &tanks_dir)?;
        }

        Ok(())
    }

    // note: this is NOT `Send`!!!
    // dont hold across an await
    pub fn entities(&self) -> MutexGuard<'_, Entities> {
        self.entities.lock().expect("entities mutex poisoned")
    }

    pub fn entities_mut(&self) -> MutexGuard<'_, Entities> {
        self.entities()
    }

    pub fn dispatch_event(&mut self, event: &GameEvents) -> mlua::Result<()> {
        match event {
            GameEvents::PlayerSpawn { id, name } => {
                let payload = self.lua.create_table()?;
                payload.set("id", *id)?;
                payload.set("name", name.as_str())?;
                Self::fire_listeners(&self.lua, &mut self.scheduler, "player_spawn", &payload)?;
            }
            GameEvents::PlayerDisconnect { id } => {
                let payload = self.lua.create_table()?;
                payload.set("id", *id)?;
                Self::fire_listeners(
                    &self.lua,
                    &mut self.scheduler,
                    "player_disconnect",
                    &payload,
                )?;
            }
            GameEvents::PlayerMovement { id, dir } => {
                let payload = self.lua.create_table()?;
                payload.set("id", *id)?;
                payload.set("dir", *dir)?;
                Self::fire_listeners(&self.lua, &mut self.scheduler, "player_movement", &payload)?;
            }
            GameEvents::PlayerAutoFire { id, enabled } => {
                let payload = self.lua.create_table()?;
                payload.set("id", *id)?;
                payload.set("enabled", *enabled)?;
                Self::fire_listeners(&self.lua, &mut self.scheduler, "player_autofire", &payload)?;
            }
            GameEvents::PlayerAim { id, dir } => {
                let payload = self.lua.create_table()?;
                payload.set("id", *id)?;
                payload.set("dir", *dir)?;
                Self::fire_listeners(&self.lua, &mut self.scheduler, "player_aim", &payload)?;
            }
            GameEvents::TankTree { .. } => {
                let payload = self.lua.create_table()?;
                Self::fire_listeners(&self.lua, &mut self.scheduler, "tank_tree", &payload)?;
            }
        }
        Ok(())
    }

    fn fire_listeners(
        lua: &Lua,
        scheduler: &mut Scheduler,
        event_name: &str,
        payload: &mlua::Table,
    ) -> mlua::Result<()> {
        let listeners: mlua::Table = lua.named_registry_value("EVENT_LISTENERS")?;

        if let Ok(handlers) = listeners.get::<mlua::Table>(event_name) {
            for func in handlers.sequence_values::<Function>() {
                let func = func?;
                let thread = lua.create_thread(func)?;
                scheduler.start_thread(thread, payload.clone())?;
            }
        }

        Ok(())
    }
}
