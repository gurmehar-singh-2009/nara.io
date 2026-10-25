use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::SystemTime,
};

use mlua::{Function, Lua};
use paris::error;

use crate::{
    entities::entity::Entities,
    fs::tank_defs::TankTree,
    game::{arena::Arena, game_state::GameEvents, scheduler::Scheduler},
    scripting::{
        api::{LuaBridge, args_table, register_api},
        registery::{TankRegistry, WeaponRegistry, register_commands, register_events},
        userdata::player::LuaPlayer,
    },
};

const PRELUDE: &str = "
function wait(seconds) return coroutine.yield(seconds) end

function every(seconds, f)
    task.spawn(function()
        while true do
            wait(seconds)
            f()
        end
    end)
end
";

pub struct Scripting {
    pub lua: Lua,
    pub entities: Arc<Mutex<Entities>>,
    pub arena: Arc<Mutex<Arena>>,
    pub bridge: Arc<Mutex<LuaBridge>>,
    pub weapons: WeaponRegistry,
    pub tanks: TankRegistry,
    #[allow(dead_code)]
    pub abilities: Option<()>,
    pub commands: Arc<Mutex<HashMap<String, Function>>>,
    pub scheduler: Scheduler,

    tanks_dir: Option<PathBuf>,
    tank_mtimes: Vec<(PathBuf, SystemTime)>,
}

fn snapshot_tank_files(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("lua"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    files
        .into_iter()
        .map(|p| {
            let mtime = p
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (p, mtime)
        })
        .collect()
}

impl Scripting {
    pub fn new(entities: Entities, map_bound: f32, bot_id_base: u32) -> mlua::Result<Self> {
        let lua = Lua::new();

        let entities = Arc::new(Mutex::new(entities));
        let arena = Arc::new(Mutex::new(Arena::new(map_bound)));
        let bridge = Arc::new(Mutex::new(LuaBridge::new(bot_id_base)));

        lua.set_app_data(entities.clone());
        lua.set_app_data(arena.clone());
        lua.set_app_data(bridge.clone());

        let commands = register_commands(&lua)?;
        register_events(&lua)?;
        register_api(&lua)?;

        lua.load(PRELUDE).set_name("prelude").exec()?;

        Ok(Self {
            lua,
            entities,
            arena,
            bridge,
            weapons: WeaponRegistry::default(),
            tanks: TankRegistry::default(),
            abilities: None,
            commands,
            scheduler: Scheduler::default(),
            tanks_dir: None,
            tank_mtimes: Vec::new(),
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
                self.run_file(&path)?;
            }
        }

        let weapons_dir = format!("{dir}/weapons");
        if Path::new(&weapons_dir).is_dir() {
            self.weapons = WeaponRegistry::load_all(&self.lua, &weapons_dir)?;
        }
        let tanks_dir = format!("{dir}/tanks");
        if Path::new(&tanks_dir).is_dir() {
            self.tanks = TankRegistry::load_all(&self.lua, &tanks_dir)?;
            self.tanks_dir = Some(PathBuf::from(&tanks_dir));
            self.tank_mtimes = snapshot_tank_files(self.tanks_dir.as_deref().unwrap());
        }

        Ok(())
    }

    pub fn load_plugin(&mut self, dir: &str, name: &str) -> mlua::Result<()> {
        let path = Path::new(dir).join("plugins").join(format!("{name}.lua"));

        if !path.is_file() {
            return Err(mlua::Error::runtime(format!(
                "plugin \"{name}\" not found at {}",
                path.display()
            )));
        }

        self.run_file(&path)
    }

    fn run_file(&mut self, path: &Path) -> mlua::Result<()> {
        let src = std::fs::read_to_string(path).map_err(mlua::Error::external)?;
        let name = path.to_string_lossy().to_string();
        self.lua.load(&src).set_name(name.as_str()).exec()?;
        self.start_spawned();

        Ok(())
    }

    pub fn entities(&self) -> MutexGuard<'_, Entities> {
        self.entities.lock().expect("entities mutex poisoned")
    }

    pub fn entities_mut(&self) -> MutexGuard<'_, Entities> {
        self.entities()
    }

    pub fn arena(&self) -> MutexGuard<'_, Arena> {
        self.arena.lock().expect("arena mutex poisoned")
    }

    pub fn bridge(&self) -> MutexGuard<'_, LuaBridge> {
        self.bridge.lock().expect("lua bridge mutex poisoned")
    }

    pub fn start_spawned(&mut self) {
        let spawned = std::mem::take(&mut self.bridge().spawned);

        for f in spawned {
            let result = self
                .lua
                .create_thread(f)
                .and_then(|thread| self.scheduler.start_thread(thread, ()));

            if let Err(err) = result {
                error!("lua task error: {err}");
            }
        }
    }

    pub fn tick(&mut self) {
        self.scheduler.on_tick();
        self.start_spawned();
    }

    pub fn run_command(&mut self, conn: u32, line: &str) -> mlua::Result<bool> {
        let mut parts = line.trim_start_matches('/').split_whitespace();
        let Some(name) = parts.next().map(|n| n.to_lowercase()) else {
            return Ok(false);
        };
        let args: Vec<&str> = parts.collect();

        let Some(func) = self
            .commands
            .lock()
            .map_err(|e| mlua::Error::external(e.to_string()))?
            .get(&name)
            .cloned()
        else {
            return Ok(false);
        };

        let thread = self.lua.create_thread(func)?;
        let args = args_table(&self.lua, &args)?;
        self.scheduler
            .start_thread(thread, (LuaPlayer(conn), args))?;
        self.start_spawned();

        Ok(true)
    }

    pub fn reload_tanks_if_changed(&mut self) -> mlua::Result<Option<TankTree>> {
        let Some(dir) = self.tanks_dir.clone() else {
            return Ok(None);
        };

        let current = snapshot_tank_files(&dir);
        if current == self.tank_mtimes {
            return Ok(None);
        }

        let registry = TankRegistry::load_all(&self.lua, dir.to_string_lossy().as_ref())?;
        let tree = registry.build_tree()?;

        self.tanks = registry;
        self.tank_mtimes = current;

        Ok(Some(tree))
    }

    pub fn dispatch_event(&mut self, event: &GameEvents) -> mlua::Result<()> {
        let payload = self.lua.create_table()?;

        let name = match event {
            GameEvents::PlayerSpawn { id, name } => {
                payload.set("id", *id)?;
                payload.set("name", name.as_str())?;
                payload.set("player", LuaPlayer(*id))?;
                "player_spawn"
            }
            GameEvents::PlayerDisconnect { id } => {
                payload.set("id", *id)?;
                "player_disconnect"
            }
            GameEvents::PlayerMovement { id, dir } => {
                payload.set("id", *id)?;
                payload.set("dir", *dir)?;
                "player_movement"
            }
            GameEvents::PlayerAutoFire { id, enabled } => {
                payload.set("id", *id)?;
                payload.set("enabled", *enabled)?;
                "player_autofire"
            }
            GameEvents::PlayerRepel { id, enabled } => {
                payload.set("id", *id)?;
                payload.set("enabled", *enabled)?;
                "player_repel"
            }
            GameEvents::PlayerAutoSpin { id, enabled } => {
                payload.set("id", *id)?;
                payload.set("enabled", *enabled)?;
                "player_autospin"
            }
            GameEvents::PlayerAim { id, dir } => {
                payload.set("id", *id)?;
                payload.set("dir", *dir)?;
                "player_aim"
            }
            GameEvents::TankSelect { id, tank_id } => {
                payload.set("id", *id)?;
                payload.set("tankId", *tank_id)?;
                payload.set("player", LuaPlayer(*id))?;
                "tank_select"
            }
            GameEvents::StatUpgrade { id, stat } => {
                payload.set("id", *id)?;
                payload.set("stat", *stat + 1)?;
                "stat_upgrade"
            }
            GameEvents::ChatMessage { id, channel, text } => {
                if text.starts_with('/') {
                    return Ok(());
                }
                payload.set("id", *id)?;
                payload.set("channel", if *channel == 1 { "team" } else { "global" })?;
                payload.set("text", text.as_str())?;
                payload.set("player", LuaPlayer(*id))?;
                "chat"
            }
            GameEvents::TankTree { .. } => "tank_tree",
            GameEvents::Tick { tick } => {
                payload.set("tick", *tick)?;
                "tick"
            }
            GameEvents::PlayerDeath { id, killer } => {
                payload.set("id", *id)?;
                payload.set("killerId", *killer)?;
                payload.set("player", LuaPlayer(*id))?;
                payload.set("killer", killer.map(LuaPlayer))?;
                "player_death"
            }
            GameEvents::ShapeKill { id, kind } => {
                payload.set("id", *id)?;
                payload.set("kind", *kind)?;
                payload.set("player", LuaPlayer(*id))?;
                "shape_kill"
            }
            GameEvents::LevelUp { id, level } => {
                payload.set("id", *id)?;
                payload.set("level", *level)?;
                payload.set("player", LuaPlayer(*id))?;
                "level_up"
            }
            GameEvents::MouseMove { .. } => return Ok(()),
        };

        self.fire_listeners(name, payload);
        self.start_spawned();

        Ok(())
    }

    pub fn has_listeners(&self, event_name: &str) -> bool {
        self.lua
            .named_registry_value::<mlua::Table>("EVENT_LISTENERS")
            .and_then(|l| l.get::<Option<mlua::Table>>(event_name))
            .map(|t| t.is_some_and(|t| t.raw_len() > 0))
            .unwrap_or(false)
    }

    fn fire_listeners(&mut self, event_name: &str, payload: mlua::Table) {
        let Ok(listeners) = self
            .lua
            .named_registry_value::<mlua::Table>("EVENT_LISTENERS")
        else {
            return;
        };

        let Ok(Some(handlers)) = listeners.get::<Option<mlua::Table>>(event_name) else {
            return;
        };

        for func in handlers.sequence_values::<Function>() {
            let result = func
                .and_then(|f| self.lua.create_thread(f))
                .and_then(|thread| self.scheduler.start_thread(thread, payload.clone()));

            if let Err(err) = result {
                error!("lua \"{event_name}\" handler error: {err}");
            }
        }
    }
}
