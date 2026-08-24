use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use mlua::{Function, Lua};

use crate::{
    entities::{entity::Entities},
    game::{game_state::GameEvents, scheduler::Scheduler},
    scripting::registery::{TankRegistry, WeaponRegistry, register_commands, register_events},
};

const PRELUDE: &str = "function wait(seconds) return coroutine.yield(seconds) end";

pub struct Scripting {
    pub lua: Lua,
    pub weapons: WeaponRegistry,
    pub tanks: TankRegistry,
    pub abilities: Option<()>,
    pub commands: Arc<Mutex<HashMap<String, Function>>>,
    pub scheduler: Scheduler,
}

impl Scripting {
    // pub fn new(entities: Entities) -> mlua::Result<Self> {
    //     let lua = Lua::new();
    //     lua.set_app_data(entities);
    //     lua.load(PRELUDE).exec();

    //     let commands = register_commands(&lua)?;

    //     let weapons = WeaponRegistry::load_all(&lua, "content/weapons")?;
    //     // let tanks = TankRegistry::load_all(&lua, "content/tanks")?;
    //     // let abilities = AbilityRegistry::load_all(&lua, "content/abilities")?;
    //     lua.load(&std::fs::read_to_string("content/commands.lua")?)
    //         .exec()?;

    //     Ok(Self {
    //         lua,
    //         weapons,
    //         tanks: None,
    //         abilities: None,
    //         commands,
    //         scheduler: Scheduler::default(),
    //     })

    // pub fn new(entities: Entities) -> mlua::Result<Self> {
    //     let lua = Lua::new();
    //     lua.set_app_data(entities);
    //     lua.load(PRELUDE).exec().unwrap();

    //     let commands = register_commands(&lua)?;
    //     register_events(&lua)?;

    //     // let base_path =
    // PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../content");
    //     let base_path = PathBuf::from("content");
    //     let weapons_path = base_path.join("weapons");
    //     let commands_path = base_path.join("commands.lua");

    //     let weapons = WeaponRegistry::load_all(&lua,
    // weapons_path.to_str().unwrap())?;     let tanks =
    // TankRegistry::load_all(&lua, base_path.join("tanks").to_str().unwrap())?;

    //     let commands_code = std::fs::read_to_string(&commands_path).map_err(|e| {
    //         mlua::Error::external(format!("Failed to read {:?}: {}",
    // commands_path, e))     })?;

    //     let events_path = base_path.join("events.lua");
    //     if events_path.exists() {
    //         let events_code = std::fs::read_to_string(&events_path)?;
    //         lua.load(&events_code).exec()?;
    //     }

    //     lua.load(&commands_code).exec()?;

    //     Ok(Self {
    //         lua,
    //         weapons,
    //         tanks,
    //         abilities: None,
    //         commands,
    //         scheduler: Scheduler::default(),
    //     })
    // }

    pub fn new(entities: Entities) -> mlua::Result<Self> {
        let lua = Lua::new();
        lua.set_app_data(entities);
        lua.load(PRELUDE).exec().unwrap();

        let commands = register_commands(&lua)?;
        register_events(&lua)?;

        let weapons = WeaponRegistry::default();
        let tanks = TankRegistry::default();

        Ok(Self {
            lua,
            weapons,
            tanks,
            abilities: None,
            commands,
            scheduler: Scheduler::default(),
        })
    }

    pub fn entities_mut(&self) -> mlua::AppDataRefMut<'_, Entities> {
        self.lua
            .app_data_mut::<Entities>()
            .expect("entities app data missing")
    }

    pub fn dispatch_event(&mut self, event: GameEvents) -> mlua::Result<()> {
        let listeners: mlua::Table = self.lua.named_registry_value("EVENT_LISTENERS")?;

        match event {
            GameEvents::PlayerSpawn { id, name } => {
                let event_name = "player_spawn";

                if let Ok(handlers) = listeners.get::<mlua::Table>(event_name) {
                    let payload = self.lua.create_table()?;
                    payload.set("name", name)?;

                    for func in handlers.sequence_values::<Function>() {
                        let func = func?;

                        let thread = self.lua.create_thread(func)?;

                        self.scheduler.start_thread(thread, payload.clone())?;
                    }
                }
            }

            _ => {}
        }

        Ok(())
    }
}
