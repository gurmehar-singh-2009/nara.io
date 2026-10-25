use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use glam::Vec2;
use nanorand::Rng;
use paris::{error, info, warn};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rustrict::CensorStr;
use shared::packets::{
    KILLER_ARENA, KILLER_PENTAGON, KILLER_SQUARE, KILLER_TANK, KILLER_TRIANGLE, NO_SENDER, NO_TEAM,
    PACKET_SEED, PlayerUpgradesPacket, TankOption, TankTreePacket, camera_zoom,
    client_bound::{
        AddEntityPacket, BarrelDef, DeathPacket, EntityType, LeaderboardPacket, PlayerStatsPacket,
        RemoveEntityPacket, ServerInfoPacket, SpectatePacket, TankInfoPacket, TurretDef,
        UpdateEntityPacket, UpdateEntityPacketData,
    },
    cursor_world_offset, level_scale,
    server_bound::ChatMessagePacket,
};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    anti_cheat::{PlayerWatch, SUSPICION_FLAG_THRESHOLD, multibox_anti, upgrade_anti},
    entities::{
        connections::Connections,
        drone::{DroneGoal, DroneSpawn, HitKey},
        entity::{Entities, EntityId},
        shape::ShapeKind,
        spatial_hash::{HashEntity, SpatialHash},
        tank::{barrel_defs, stat_max, stat_names},
    },
    fs::{
        load_config::{Config, TurretLayout},
        tank_defs::{Tank, TankTree},
    },
    game::bots::{
        BotBrain, NAV_LEN, NAV_REBUILD_INTERVAL, is_bot_id, rebuild_nav_grid, spend_stat_points,
        tick_ai, tick_spawns,
    },
    scripting::{
        api::{LuaAction, TICK_RATE},
        scripting::Scripting,
    },
};

#[derive(Debug)]
pub enum GameEvents {
    PlayerSpawn {
        id: u32,
        name: String,
    },
    PlayerDisconnect {
        id: u32,
    },
    PlayerMovement {
        id: u32,
        dir: Option<f32>,
    },
    PlayerAutoFire {
        id: u32,
        enabled: bool,
    },
    PlayerRepel {
        id: u32,
        enabled: bool,
    },
    PlayerAutoSpin {
        id: u32,
        enabled: bool,
    },
    PlayerAim {
        id: u32,
        dir: f32,
    },
    MouseMove {
        id: u32,
        x: u16,
        y: u16,
        window_width: u16,
        window_height: u16,
    },
    TankSelect {
        id: u32,
        tank_id: u32,
    },
    StatUpgrade {
        id: u32,
        stat: u8,
    },
    TankTree {
        tree: TankTree,
    },
    ChatMessage {
        id: u32,
        channel: u8,
        text: String,
    },
    Tick {
        tick: u64,
    },
    PlayerDeath {
        id: u32,
        killer: Option<u32>,
    },
    ShapeKill {
        id: u32,
        kind: &'static str,
    },
    LevelUp {
        id: u32,
        level: u32,
    },
}

type DamageLog = HashMap<EntityId, Vec<(EntityId, f32, u64)>>;

pub struct TurretState {
    aims: Vec<f32>,
    timers: Vec<f32>,
    targets: Vec<Option<Vec2>>,
}

pub struct SpectatorCam {
    pos: Vec2,
    from: Vec2,
    to: Vec2,
    elapsed: f32,
    duration: f32,
    hold_until: u64,
}

impl SpectatorCam {
    fn at(pos: Vec2, hold_until: u64) -> Self {
        Self {
            pos,
            from: pos,
            to: pos,
            elapsed: 0.0,
            duration: 0.0,
            hold_until,
        }
    }
}

pub struct GameState {
    pub scripting: Scripting,
    pub spatial_hash: SpatialHash,
    pub game_channel_recv: UnboundedReceiver<GameEvents>,
    pub connections: Connections,
    players: HashMap<u32, EntityId>,
    watch: HashMap<u32, PlayerWatch>,
    rnd: nanorand::WyRand,
    tick_count: u64,
    tank_tree: TankTree,
    config: Arc<Config>,
    teams: HashMap<u32, u8>,
    chat_times: HashMap<u32, VecDeque<u64>>,
    interest: HashMap<u32, HashMap<u32, EntityType>>,
    bots: HashMap<u32, BotBrain>,
    bot_respawns: Vec<(u32, u64)>,
    nav_danger: Vec<f32>,
    nav_version: u64,
    bullet_hash: SpatialHash,
    mouse_offsets: HashMap<u32, Vec2>,
    lua_events: Vec<GameEvents>,
    arena_version_sent: u64,
    spectators: HashMap<u32, SpectatorCam>,
    spawn_ticks: HashMap<u32, u64>,
    turrets: HashMap<EntityId, TurretState>,
    base_drone_respawn: HashMap<u32, u64>,
    damage_log: DamageLog,
    hotspot: Vec2,
    tick_time_acc: f64,
    load: f32,

    buf_collidable: Vec<Option<(Vec2, f32, f32, bool, f32)>>,
    buf_conn_by_slot: Vec<u32>,
    buf_slot_net_id: Vec<u32>,
    buf_slot_is_player: Vec<bool>,
    buf_slot_update: Vec<usize>,
    buf_entity_seen: Vec<bool>,
    buf_bullet_seen: Vec<bool>,
    buf_spent_flags: Vec<bool>,
    buf_bot_nearby: Vec<HashEntity>,
}

impl GameState {
    pub fn new(
        game_channel_recv: UnboundedReceiver<GameEvents>,
        connections: Connections,
        config: Arc<Config>,
    ) -> mlua::Result<Self> {
        let mut scripting =
            Scripting::new(Entities::new(), config.world.map_bound, config.bots.id_base)?;

        let mut loaded = false;
        for dir in ["content", "../content", "server/content"] {
            if std::path::Path::new(dir).is_dir() {
                if let Err(err) = scripting.load_scripts(dir) {
                    error!("Lua script load error ({dir}): {err}");
                }

                for plugin in &config.game.plugins {
                    match scripting.load_plugin(dir, plugin) {
                        Ok(()) => info!("loaded plugin {plugin}"),
                        Err(err) => error!("plugin {plugin} failed to load: {err}"),
                    }
                }

                loaded = true;

                break;
            }
        }

        if !loaded {
            error!("content/ not found!! maybe run from the repo root or adjust path?");
        }

        let config = {
            let mut arena = scripting.arena();
            arena.locked = true;

            let mut config = (*config).clone();
            config.world.map_bound = arena.bound;

            info!(
                "arena: bound {}, {} walls, {} zones, {} teams",
                arena.bound,
                arena.walls().len(),
                arena.zones().len(),
                arena.team_count
            );

            Arc::new(config)
        };

        let tank_tree = scripting.tanks.build_tree()?;
        info!("loaded {} tank tiers from content/tanks", tank_tree.len());

        scripting
            .entities_mut()
            .set_tank_tree(Arc::new(tank_tree.clone()));

        Ok(Self {
            scripting,
            spatial_hash: SpatialHash::new(),
            game_channel_recv,
            connections,
            players: HashMap::new(),
            watch: HashMap::new(),
            rnd: nanorand::WyRand::new(),
            tick_count: 0,
            tank_tree,
            config,
            teams: HashMap::new(),
            chat_times: HashMap::new(),
            interest: HashMap::new(),
            bots: HashMap::new(),
            bot_respawns: Vec::new(),
            nav_danger: vec![0.0; NAV_LEN],
            nav_version: 0,
            bullet_hash: SpatialHash::new(),
            mouse_offsets: HashMap::new(),
            lua_events: Vec::new(),
            arena_version_sent: 0,
            spectators: HashMap::new(),
            spawn_ticks: HashMap::new(),
            turrets: HashMap::new(),
            base_drone_respawn: HashMap::new(),
            damage_log: HashMap::new(),
            hotspot: Vec2::ZERO,
            tick_time_acc: 0.0,
            load: 0.0,
            buf_collidable: Vec::new(),
            buf_conn_by_slot: Vec::new(),
            buf_slot_net_id: Vec::new(),
            buf_slot_is_player: Vec::new(),
            buf_slot_update: Vec::new(),
            buf_entity_seen: Vec::new(),
            buf_bullet_seen: Vec::new(),
            buf_spent_flags: Vec::new(),
            buf_bot_nearby: Vec::with_capacity(512),
        })
    }

    pub async fn game_loop(&mut self) {
        let tick_rate = Duration::from_millis(100);
        let dt = tick_rate.as_secs_f32();
        let mut interval = tokio::time::interval(tick_rate);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut phase_us = [0.0f64; 7];
        const REPORT_EVERY: u64 = 50;

        loop {
            let tick_start = std::time::Instant::now();
            self.tick_count += 1;

            let t = std::time::Instant::now();

            if self.scripting.has_listeners("tick") {
                self.lua_events.push(GameEvents::Tick {
                    tick: self.tick_count,
                });
            }

            self.drain_events();
            self.tick_bot_spawns();
            self.reload_tanks();

            phase_us[0] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.tick_shape_orbits(dt);
            self.tick_auto_spin(dt);
            self.prune_damage_log();
            self.regen_health();
            self.apply_base_zones(dt);
            self.rebuild_spatial_hash();
            if self.tick_count % NAV_REBUILD_INTERVAL == 0 {
                rebuild_nav_grid(
                    &mut self.scripting,
                    &mut self.nav_danger,
                    &mut self.nav_version,
                    &self.config,
                );
            }

            phase_us[1] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.tick_bot_ai();
            phase_us[2] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.resolve_entity_collisions();
            self.clamp_shape_positions();
            phase_us[3] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.update_spectators(dt);
            self.broadcast_entity_updates(dt);
            self.broadcast_arena();
            self.broadcast_player_stats();
            self.send_tank_upgrade_offers();
            self.run_anti_cheat_pass();

            if self.tick_count % 10 == 0 {
                self.broadcast_leaderboard();
                self.broadcast_server_info();
            }

            phase_us[4] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.simulate_combat(dt);
            phase_us[5] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.sync_lua();
            self.scripting.tick();
            self.flush_lua_events();
            self.apply_lua_actions();
            self.tick_shape_spawns();
            self.tick_base_drones();
            phase_us[6] += t.elapsed().as_secs_f64() * 1.0e6;

            let elapsed = tick_start.elapsed();
            self.tick_time_acc += elapsed.as_secs_f64();
            if self.tick_count % 10 == 0 {
                self.load = (self.tick_time_acc / 10.0 / tick_rate.as_secs_f64()) as f32;
                self.tick_time_acc = 0.0;
            }

            if elapsed > tick_rate {
                warn!("tick overran: {:#?} (budget 100ms)", elapsed);
            }

            if self.tick_count % REPORT_EVERY == 0 {
                let n = REPORT_EVERY as f64;
                let total: f64 = phase_us.iter().sum::<f64>() / n / 1000.0;
                let avg = |i: usize| phase_us[i] / n / 1000.0;

                info!(
                    "tick avg: {:.2}ms total | events/reload {:.2} | sim+hash {:.2} | bot_ai {:.2} | collisions {:.2} | broadcast {:.2} | combat {:.2} | lua+spawn {:.2}",
                    total,
                    avg(0),
                    avg(1),
                    avg(2),
                    avg(3),
                    avg(4),
                    avg(5),
                    avg(6),
                );

                phase_us = [0.0; 7];
            }

            interval.tick().await;
        }
    }

    fn tick_bot_spawns(&mut self) {
        let spawned = tick_spawns(
            &mut self.scripting,
            &self.config,
            &mut self.players,
            &mut self.bots,
            &mut self.bot_respawns,
            &mut self.rnd,
            &self.nav_danger,
            self.nav_version,
            self.tick_count,
        );

        for (bot_id, entity_id) in spawned {
            self.place_tank(bot_id, entity_id);
        }
    }

    fn tick_bot_ai(&mut self) {
        if self.bots.is_empty() {
            return;
        }

        let upgrade_candidates = tick_ai(
            &mut self.bots,
            &self.players,
            &self.spatial_hash,
            &self.scripting.arena(),
            &self.nav_danger,
            &mut self.rnd,
            &mut self.scripting.entities_mut(),
            self.nav_version,
            self.tick_count,
            self.config.player.speed,
            &mut self.buf_bot_nearby,
            &self.config,
        );

        for (bot_id, eid) in upgrade_candidates {
            self.bot_try_upgrade(bot_id, eid);
        }

        if self.tick_count % 5 == 0 {
            let spend: Vec<EntityId> = self.bots.values().map(|b| b.entity).collect();
            for eid in spend {
                let mut entities = self.scripting.entities_mut();
                spend_stat_points(&mut entities, &mut self.rnd, eid, 0, &self.config);
                refresh_max_health(&mut entities, eid, &self.config);
            }
        }
    }

    fn bot_try_upgrade(&mut self, bot_id: u32, entity_id: EntityId) {
        let pick = {
            let entities = self.scripting.entities_mut();
            let Some(tank) = entities.tanks.get(entity_id) else {
                return;
            };
            let level = *tank.level;
            let upgrade_ids = tank.tank_type.upgrades.clone();
            let tree = &self.tank_tree;

            let options: Vec<u32> = upgrade_ids
                .iter()
                .filter_map(|uid| find_def(tree, *uid))
                .filter(|(_, def)| !def.flags.dev_only && level >= def.level_requirement)
                .map(|(_, def)| def.id)
                .collect();

            if options.is_empty() {
                return;
            }
            options[self.rnd.generate::<u32>() as usize % options.len()]
        };

        let Some((_, def)) = find_def(&self.tank_tree, pick) else {
            return;
        };

        {
            let mut entities = self.scripting.entities_mut();
            entities.tanks.apply_def(entity_id, def);
            refresh_max_health(&mut entities, entity_id, &self.config);
        }

        info!("bot {bot_id} upgraded to {}", def.name);
        self.broadcast_player_def(bot_id, entity_id);
    }

    fn reload_tanks(&mut self) {
        if self.tick_count % self.config.world.tank_reload_ticks != 0 {
            return;
        }

        match self.scripting.reload_tanks_if_changed() {
            Ok(Some(tree)) => {
                info!("tank definitions reloaded from content/tanks");
                let event = GameEvents::TankTree { tree };
                self.handle_game_events(&event);
                if let Err(err) = self.scripting.dispatch_event(&event) {
                    error!("Lua event error: {err}");
                }
            }
            Ok(None) => {}
            Err(err) => error!("tank reload failed (keeping old defs): {err}"),
        }
    }

    fn drain_events(&mut self) {
        while let Ok(msg) = self.game_channel_recv.try_recv() {
            self.handle_game_events(&msg);
            self.sync_lua();
            if let Err(err) = self.scripting.dispatch_event(&msg) {
                error!("Lua event error: {err}");
            }
        }

        self.apply_lua_actions();
    }

    fn sync_lua(&mut self) {
        let mut bridge = self.scripting.bridge();
        bridge.players.clone_from(&self.players);
        bridge.tick = self.tick_count;
    }

    fn flush_lua_events(&mut self) {
        if self.lua_events.is_empty() {
            return;
        }

        self.sync_lua();

        for event in std::mem::take(&mut self.lua_events) {
            if let Err(err) = self.scripting.dispatch_event(&event) {
                error!("Lua event error: {err}");
            }
        }
    }

    fn apply_lua_actions(&mut self) {
        loop {
            let actions = std::mem::take(&mut self.scripting.bridge().actions);
            if actions.is_empty() {
                break;
            }

            for action in actions {
                self.apply_lua_action(action);
            }
        }
    }

    fn apply_lua_action(&mut self, action: LuaAction) {
        match action {
            LuaAction::Broadcast(text) => {
                self.connections.broadcast(system_message(text));
            }

            LuaAction::Message(id, text) => {
                self.connections.send_to(id, system_message(text));
            }

            LuaAction::SetTank(id, name) => {
                let Some(&entity_id) = self.players.get(&id) else {
                    return;
                };

                let wanted = name.to_lowercase().replace([' ', '-'], "_");
                let def = self
                    .scripting
                    .tanks
                    .0
                    .iter()
                    .find(|(key, def)| {
                        key.to_lowercase() == wanted
                            || def.tank.name.to_lowercase().replace([' ', '-'], "_") == wanted
                    })
                    .map(|(_, def)| def.tank.id)
                    .and_then(|tank_id| find_def(&self.tank_tree, tank_id))
                    .map(|(_, def)| def.clone());

                let Some(def) = def else {
                    warn!("lua asked for unknown tank \"{name}\"");
                    return;
                };

                {
                    let mut entities = self.scripting.entities_mut();
                    entities.tanks.apply_def(entity_id, &def);
                    refresh_max_health(&mut entities, entity_id, &self.config);
                }

                self.broadcast_player_def(id, entity_id);
            }

            LuaAction::SetLevel(id, level) => {
                let Some(&entity_id) = self.players.get(&id) else {
                    return;
                };

                {
                    let mut entities = self.scripting.entities_mut();
                    let Some(t) = entities.tanks.get_mut(entity_id) else {
                        return;
                    };

                    let level = level.clamp(1, self.config.world.max_level);
                    let spent: u32 = t.stat_levels.iter().map(|&l| l as u32).sum();

                    *t.level = level;
                    t.xp.0 = 0;
                    *t.stat_points = total_stat_points(level).saturating_sub(spent);

                    refresh_max_health(&mut entities, entity_id, &self.config);
                }
            }

            LuaAction::AddXp(id, amount) => {
                let Some(&entity_id) = self.players.get(&id) else {
                    return;
                };

                let leveled = {
                    let mut entities = self.scripting.entities_mut();
                    grant_xp(&mut entities, entity_id, amount, &self.config)
                };

                if let Some(level) = leveled {
                    self.lua_events.push(GameEvents::LevelUp { id, level });
                }
            }

            LuaAction::Kill(id) => {
                let Some(&entity_id) = self.players.get(&id) else {
                    return;
                };

                if let Some(h) = self.scripting.entities_mut().health_mut(entity_id) {
                    *h = 0;
                }

                let entity_to_conn = self.entity_to_conn();
                self.finish_kills(vec![(entity_id, None)], &entity_to_conn);
            }

            LuaAction::SetTeam(id, team) => {
                let Some(&entity_id) = self.players.get(&id) else {
                    return;
                };

                if let Some(t) = self.scripting.entities_mut().tanks.get_mut(entity_id) {
                    *t.team = team;
                }

                if team != NO_TEAM {
                    self.teams.insert(id, team);
                }

                self.send_tank_info(id, entity_id);
            }
        }
    }

    fn entity_to_conn(&self) -> HashMap<EntityId, u32> {
        self.players.iter().map(|(&k, &v)| (v, k)).collect()
    }

    fn handle_game_events(&mut self, msg: &GameEvents) {
        match &msg {
            GameEvents::PlayerSpawn { id, name } => {
                self.watch.entry(*id).or_default();
                self.spawn_player(*id, name);
            }

            GameEvents::PlayerDisconnect { id } => {
                if let Some(entity_id) = self.players.remove(id) {
                    self.scripting.entities_mut().despawn(entity_id);
                }

                self.connections.remove(*id);
                self.connections.broadcast(RemoveEntityPacket::new(
                    *id,
                    EntityType::Player,
                    PACKET_SEED as u64,
                ));

                forget_entity(&mut self.interest, *id);

                self.interest.remove(id);
                self.watch.remove(id);
                self.teams.remove(id);
                self.chat_times.remove(id);
                self.mouse_offsets.remove(id);
                self.spectators.remove(id);
                self.spawn_ticks.remove(id);
            }

            GameEvents::ChatMessage { id, channel, text } => {
                self.handle_chat(*id, *channel, text.clone())
            }

            GameEvents::PlayerMovement { id, dir } => {
                self.with_live_entity(*id, |entities, eid| {
                    entities.tanks.set_movement_dir(eid, *dir);
                });
            }

            GameEvents::PlayerAutoFire { id, enabled } => {
                self.with_live_entity(*id, |entities, eid| {
                    entities.tanks.set_auto_fire(eid, *enabled);
                });
            }

            GameEvents::PlayerRepel { id, enabled } => {
                self.with_live_entity(*id, |entities, eid| {
                    if let Some(t) = entities.tanks.get_mut(eid) {
                        *t.repel = *enabled;
                    }
                });
            }

            GameEvents::PlayerAutoSpin { id, enabled } => {
                self.with_live_entity(*id, |entities, eid| {
                    if let Some(t) = entities.tanks.get_mut(eid) {
                        *t.auto_spin = *enabled;
                    }
                });
            }

            GameEvents::Tick { .. }
            | GameEvents::PlayerDeath { .. }
            | GameEvents::ShapeKill { .. }
            | GameEvents::LevelUp { .. } => {}

            GameEvents::PlayerAim { id, dir } => {
                let mut spinning = false;
                self.with_live_entity(*id, |entities, eid| {
                    if let Some(tank) = entities.tanks.get_mut(eid) {
                        spinning = *tank.auto_spin;
                        if !spinning {
                            *tank.aim = *dir;
                        }
                    }
                });

                if !spinning {
                    self.record_aim_sample(*id, *dir);
                }
            }

            GameEvents::MouseMove {
                id,
                x,
                y,
                window_width,
                window_height,
            } => self.record_mouse(*id, *x, *y, *window_width, *window_height),

            GameEvents::TankSelect { id, tank_id } => self.apply_tank_upgrade(*id, *tank_id),
            GameEvents::StatUpgrade { id, stat } => self.apply_stat_upgrade(*id, *stat),

            GameEvents::TankTree { tree } => {
                self.tank_tree = tree.clone();

                let mut updated: Vec<(u32, EntityId)> = Vec::new();
                {
                    let mut entities = self.scripting.entities_mut();
                    entities.set_tank_tree(Arc::new(tree.clone()));

                    for (&conn_id, &entity_id) in self.players.iter() {
                        let current_id = match entities.tanks.get(entity_id) {
                            Some(tank) => tank.tank_type.id,
                            None => continue,
                        };

                        if let Some((_, def)) = find_def(tree, current_id) {
                            if entities.tanks.apply_def(entity_id, def) {
                                refresh_max_health(&mut entities, entity_id, &self.config);
                                updated.push((conn_id, entity_id));
                            }
                        }
                    }

                    entities.tanks.reset_offers();
                }

                for (conn_id, entity_id) in updated {
                    self.broadcast_player_def(conn_id, entity_id);
                }
            }
        }
    }

    fn apply_stat_upgrade(&mut self, conn_id: u32, stat: u8) {
        let Some(&entity_id) = self.players.get(&conn_id) else {
            return;
        };

        if stat as usize >= 8 {
            return;
        }

        let mut entities = self.scripting.entities_mut();
        let Some(t) = entities.tanks.get_mut(entity_id) else {
            return;
        };

        let max = stat_max(
            t.tank_type,
            stat as usize,
            self.config.player.stat_max_level,
        );
        if *t.stat_points == 0 || t.stat_levels[stat as usize] >= max {
            return;
        }

        t.stat_levels[stat as usize] += 1;
        *t.stat_points -= 1;

        if stat == 1 {
            refresh_max_health(&mut entities, entity_id, &self.config);
        }
    }

    fn handle_chat(&mut self, conn_id: u32, channel: u8, text: String) {
        let Some(&entity_id) = self.players.get(&conn_id) else {
            return;
        };

        let name = {
            let entities = self.scripting.entities_mut();
            entities
                .tanks
                .get(entity_id)
                .map(|t| t.name.to_string())
                .unwrap_or_default()
        };

        let now = now_ms();

        {
            let times = self.chat_times.entry(conn_id).or_default();

            if let Some(&last) = times.back() {
                if now.saturating_sub(last) < self.config.world.chat_min_gap_ms {
                    return;
                }
            }

            times.push_back(now);

            while let Some(&front) = times.front() {
                if now.saturating_sub(front) > self.config.world.chat_window_ms {
                    times.pop_front();
                } else {
                    break;
                }
            }

            if times.len() > self.config.world.chat_burst {
                let notice = ChatMessagePacket::new(
                    NO_SENDER,
                    channel,
                    0,
                    now / 1000,
                    String::new(),
                    "You are sending messages too quickly.".to_string(),
                    PACKET_SEED as u64,
                );
                self.connections.send_to(conn_id, notice);

                return;
            }
        }

        let text = text.trim();
        if text.is_empty() || text.chars().count() > self.config.world.chat_max_chars {
            return;
        }

        if text.starts_with('/') {
            self.sync_lua();

            match self.scripting.run_command(conn_id, text) {
                Ok(true) => {}
                Ok(false) => {
                    let name = text.split_whitespace().next().unwrap_or(text);
                    self.connections
                        .send_to(conn_id, system_message(format!("Unknown command {name}")));
                }
                Err(err) => {
                    error!("command \"{text}\" from {conn_id} failed: {err}");
                    self.connections
                        .send_to(conn_id, system_message("That command failed.".to_string()));
                }
            }

            return;
        }
        let censored = text.censor().to_string();

        let team = self.teams.get(&conn_id).unwrap_or(&0);
        let packet = ChatMessagePacket::new(
            conn_id,
            channel,
            *team,
            now / 1000,
            name,
            censored,
            PACKET_SEED as u64,
        );

        if channel == 1 {
            let recipients: Vec<u32> = self
                .teams
                .iter()
                .filter(|&(_, &t)| &t == team)
                .map(|(&id, _)| id)
                .collect();

            for id in recipients {
                self.connections.send_to(id, packet.clone());
            }
        } else {
            self.connections.broadcast(packet);
        }
    }

    fn least_populated_team(&self) -> u8 {
        let mut counts = vec![0usize; self.config.world.num_teams.max(1) as usize];

        for &team in self.teams.values() {
            let i = (team as usize).min(counts.len() - 1);
            counts[i] += 1;
        }

        counts
            .iter()
            .enumerate()
            .min_by_key(|&(_, &count)| count)
            .map(|(i, _)| i as u8)
            .unwrap_or(0)
    }

    fn pick_tank_team(&self) -> u8 {
        let team_count = self.scripting.arena().team_count as usize;
        if team_count == 0 {
            return NO_TEAM;
        }

        let mut counts = vec![0usize; team_count];
        let entities = self.scripting.entities_mut();

        for &eid in self.players.values() {
            let team = entities.tanks.team_of(eid) as usize;
            if team < team_count {
                counts[team] += 1;
            }
        }

        counts
            .iter()
            .enumerate()
            .min_by_key(|&(_, &count)| count)
            .map(|(i, _)| i as u8)
            .unwrap_or(0)
    }

    fn place_tank(&mut self, conn_id: u32, entity_id: EntityId) {
        let team = self.pick_tank_team();
        let radius = self.config.player.radius;
        let spawn = self
            .scripting
            .arena()
            .spawn_point(&mut self.rnd, team, radius * 2.0);

        let mut entities = self.scripting.entities_mut();
        if let Some(t) = entities.tanks.get_mut(entity_id) {
            *t.team = team;
        }

        entities.positions[entity_id.index] = spawn;
        entities.velocities[entity_id.index] = Vec2::ZERO;
        refresh_max_health(&mut entities, entity_id, &self.config);

        let max = entities.tanks.get(entity_id).map(|t| *t.max_health);
        if let (Some(max), Some(h)) = (max, entities.health_mut(entity_id)) {
            *h = max;
        }
        drop(entities);

        if !is_bot_id(conn_id, &self.config) {
            let chat_team = if team == NO_TEAM {
                self.least_populated_team()
            } else {
                team
            };
            self.teams.insert(conn_id, chat_team);
        }
    }

    fn send_tank_info(&self, conn_id: u32, entity_id: EntityId) {
        if is_bot_id(conn_id, &self.config) {
            return;
        }

        let packet = {
            let entities = self.scripting.entities_mut();
            let Some(t) = entities.tanks.get(entity_id) else {
                return;
            };

            let mut maxes = [0u8; 8];
            for (i, max) in maxes.iter_mut().enumerate() {
                *max = stat_max(t.tank_type, i, self.config.player.stat_max_level);
            }

            TankInfoPacket::new(
                t.tank_type.name.clone(),
                t.tank_type.field_factor,
                *t.team,
                stat_names(t.tank_type),
                maxes,
                PACKET_SEED as u64,
            )
        };

        self.connections.send_to(conn_id, packet);
    }

    fn broadcast_server_info(&mut self) {
        let bots = self
            .players
            .keys()
            .filter(|&&id| is_bot_id(id, &self.config))
            .count() as u32;
        let players = self.connections.ids().len() as u32;
        let shapes = self.scripting.entities_mut().shapes.len() as u32;

        self.connections.broadcast(ServerInfoPacket::new(
            self.load * 100.0,
            players,
            bots,
            shapes,
            PACKET_SEED as u64,
        ));
    }

    fn find_hotspot(&mut self) {
        let bound = self.config.world.map_bound;
        let cell = self.config.spectate.cell.max(1.0);
        let cells = ((bound * 2.0) / cell).ceil().max(1.0) as usize;
        let mut heat = vec![0.0f32; cells * cells];

        let cell_of = |p: Vec2| {
            let x = (((p.x + bound) / cell) as usize).min(cells - 1);
            let y = (((p.y + bound) / cell) as usize).min(cells - 1);
            y * cells + x
        };

        let mut points: Vec<(Vec2, f32)> = Vec::new();
        {
            let entities = self.scripting.entities_mut();

            for &eid in self.players.values() {
                if let Some(t) = entities.tanks.get(eid) {
                    points.push((entities.positions[eid.index], 3.0 + *t.level as f32 * 0.08));
                }
            }

            for (_, pos, _, _, _, _) in entities.bullets.iter() {
                points.push((*pos, 0.35));
            }

            for (_, pos, _, _, _, _, _, _) in entities.drones.iter_net() {
                points.push((pos, 0.25));
            }
        }

        if points.is_empty() {
            return;
        }

        for &(p, w) in &points {
            heat[cell_of(p)] += w;
        }

        let window = |cx: usize, cy: usize| {
            let mut sum = 0.0;
            for y in cy.saturating_sub(1)..=(cy + 1).min(cells - 1) {
                for x in cx.saturating_sub(1)..=(cx + 1).min(cells - 1) {
                    sum += heat[y * cells + x];
                }
            }
            sum
        };

        let mut best = (0usize, 0usize, f32::MIN);
        for cy in 0..cells {
            for cx in 0..cells {
                let score = window(cx, cy);
                if score > best.2 {
                    best = (cx, cy, score);
                }
            }
        }

        let current = cell_of(self.hotspot);
        let current_score = window(current % cells, current / cells);
        if best.2 < current_score * self.config.spectate.stickiness + 1.0 {
            return;
        }

        let center = Vec2::new(
            -bound + (best.0 as f32 + 0.5) * cell,
            -bound + (best.1 as f32 + 0.5) * cell,
        );

        let reach = cell * 1.5;
        let (sum, weight) = points
            .iter()
            .filter(|(p, _)| (p.x - center.x).abs() <= reach && (p.y - center.y).abs() <= reach)
            .fold((Vec2::ZERO, 0.0), |(s, tw), &(p, w)| (s + p * w, tw + w));

        self.hotspot = if weight > 0.0 { sum / weight } else { center };
    }

    fn update_spectators(&mut self, dt: f32) {
        if self.tick_count % self.config.spectate.interval.max(1) == 1 {
            self.find_hotspot();
        }

        let connected = self.connections.ids();

        self.spectators
            .retain(|id, _| connected.contains(id) && !self.players.contains_key(id));

        for id in connected {
            if self.players.contains_key(&id) || self.spectators.contains_key(&id) {
                continue;
            }

            self.spectators
                .insert(id, SpectatorCam::at(self.hotspot, 0));

            let arena_packet = self.scripting.arena().packet();
            self.connections.send_to(id, arena_packet);
        }

        let cfg = &self.config.spectate;

        for (&id, cam) in self.spectators.iter_mut() {
            if self.tick_count >= cam.hold_until {
                if cam.to.distance_squared(self.hotspot) > 1.0 {
                    let distance = cam.pos.distance(self.hotspot);
                    cam.from = cam.pos;
                    cam.to = self.hotspot;
                    cam.elapsed = 0.0;
                    cam.duration =
                        (distance / cfg.travel_speed).clamp(cfg.min_travel, cfg.max_travel);
                }

                cam.elapsed = (cam.elapsed + dt).min(cam.duration);
                let t = if cam.duration > 0.0 {
                    ease_in_out_quad(cam.elapsed / cam.duration)
                } else {
                    1.0
                };
                cam.pos = cam.from.lerp(cam.to, t);
            }

            self.connections.send_to(
                id,
                SpectatePacket::new(cam.pos.x, cam.pos.y, PACKET_SEED as u64),
            );
        }
    }

    fn broadcast_arena(&mut self) {
        let (version, packet) = {
            let arena = self.scripting.arena();
            if arena.version == self.arena_version_sent {
                return;
            }
            (arena.version, arena.packet())
        };

        self.arena_version_sent = version;
        self.connections.broadcast(packet);
    }

    fn apply_base_zones(&mut self, dt: f32) {
        let zones: Vec<_> = self
            .scripting
            .arena()
            .zones()
            .iter()
            .filter(|z| z.team != NO_TEAM && z.damage > 0.0)
            .copied()
            .collect();

        if zones.is_empty() {
            return;
        }

        let mut killed = Vec::new();
        {
            let mut entities = self.scripting.entities_mut();

            for &eid in self.players.values() {
                if !entities.is_alive(eid) {
                    continue;
                }

                let team = entities.tanks.team_of(eid);
                let pos = entities.positions[eid.index];

                let Some(zone) = zones.iter().find(|z| z.team != team && z.contains(pos)) else {
                    continue;
                };

                let center = (zone.min + zone.max) * 0.5;
                let out = (pos - center).normalize_or_zero();
                entities.recoil_velocities[eid.index] += out * self.config.world.base_push * dt;

                let damage = ((zone.damage * dt).round() as u32).max(1);
                if let Some(h) = entities.health_mut(eid) {
                    let was_alive = *h > 0;
                    *h = h.saturating_sub(damage);
                    if was_alive && *h == 0 {
                        killed.push((eid, None));
                    }
                }
            }
        }

        if !killed.is_empty() {
            let entity_to_conn = self.entity_to_conn();
            self.finish_kills(killed, &entity_to_conn);
        }
    }

    fn broadcast_player_def(&mut self, conn_id: u32, entity_id: EntityId) {
        let (x, y, level, name, barrels, team, turrets) = {
            let entities = self.scripting.entities_mut();

            let Some(entity) = entities.get(entity_id) else {
                return;
            };

            let Some(tank) = entities.tanks.get(entity_id) else {
                return;
            };

            (
                entity.position.x,
                entity.position.y,
                *tank.level,
                tank.name.to_string(),
                tank.barrels.clone(),
                *tank.team,
                turret_mounts(tank.tank_type, &self.config),
            )
        };

        self.send_tank_info(conn_id, entity_id);

        let my_packet = AddEntityPacket::new(
            conn_id,
            EntityType::Player,
            x,
            y,
            level,
            name.clone(),
            true,
            team,
            barrels.clone(),
            turrets.clone(),
            PACKET_SEED as u64,
        );
        self.connections.send_to(conn_id, my_packet);
        self.interest
            .entry(conn_id)
            .or_default()
            .insert(conn_id, EntityType::Player);

        let def_pos = Vec2::new(x, y);
        let enter_sq = self.config.world.view_enter_rad.powi(2);

        let mut recipients: Vec<u32> = Vec::new();
        {
            let entities = self.scripting.entities_mut();

            for (&other_conn, &other_entity) in self.players.iter() {
                if other_conn == conn_id || !entities.is_alive(other_entity) {
                    continue;
                }

                let near =
                    entities.positions[other_entity.index].distance_squared(def_pos) <= enter_sq;
                let known = self
                    .interest
                    .get(&other_conn)
                    .is_some_and(|visible| visible.contains_key(&conn_id));

                if near || known {
                    recipients.push(other_conn);
                }
            }
        }

        for other_conn in recipients {
            let packet = AddEntityPacket::new(
                conn_id,
                EntityType::Player,
                x,
                y,
                level,
                name.clone(),
                false,
                team,
                barrels.clone(),
                turrets.clone(),
                PACKET_SEED as u64,
            );
            self.connections.send_to(other_conn, packet);
            self.interest
                .entry(other_conn)
                .or_default()
                .insert(conn_id, EntityType::Player);
        }
    }

    fn with_live_entity(&mut self, id: u32, f: impl FnOnce(&mut Entities, EntityId)) {
        let Some(&entity_id) = self.players.get(&id) else {
            return;
        };

        let mut entities = self.scripting.entities_mut();

        if entities.is_alive(entity_id) {
            f(&mut entities, entity_id);
        }
    }

    fn record_mouse(&mut self, id: u32, x: u16, y: u16, width: u16, height: u16) {
        if x > width || y > height {
            return;
        }

        let Some(&entity_id) = self.players.get(&id) else {
            return;
        };

        let zoom = {
            let entities = self.scripting.entities_mut();
            match entities.tanks.get(entity_id) {
                Some(tank) => camera_zoom(*tank.level, tank.tank_type.field_factor),
                None => return,
            }
        };

        let Some((dx, dy)) =
            cursor_world_offset(x as f32, y as f32, width as f32, height as f32, zoom)
        else {
            return;
        };

        let offset = Vec2::new(dx, dy).clamp_length_max(self.config.world.view_enter_rad);
        self.mouse_offsets.insert(id, offset);
    }

    fn record_aim_sample(&mut self, id: u32, dir: f32) {
        let now = now_ms();

        let Some(watch) = self.watch.get_mut(&id) else {
            return;
        };

        watch.record_aim(now, dir);

        let analysis = watch.aim.analysis();
        if analysis.samples >= self.config.anticheat.min_aim_samples {
            if analysis.max_velocity > self.config.anticheat.max_human_angular_velocity {
                watch.tracker.add_score(10);
            }

            if analysis.max_jerk > self.config.anticheat.max_human_angular_jerk {
                watch.tracker.add_score(6);
            }

            if analysis.sign_inversion > self.config.anticheat.max_sign_inversion_ratio {
                watch.tracker.add_score(4);
            }
        }
    }

    fn apply_tank_upgrade(&mut self, conn_id: u32, tank_id: u32) {
        let Some(&entity_id) = self.players.get(&conn_id) else {
            return;
        };

        let Some((tier, def)) = find_def(&self.tank_tree, tank_id) else {
            // later: temp ban player
            error!("player {conn_id} requested unknown tank {tank_id}");

            return;
        };

        let now = now_ms();
        {
            let watch = self.watch.entry(conn_id).or_default();
            if !upgrade_anti::check_upgrade_interval(
                watch.last_upgrade_ms,
                now,
                self.config.anticheat.min_upgrade_interval_ms,
            ) {
                watch.tracker.add_score(25);

                // later: temp ban player
                error!("player {conn_id} tank upgrade rejected: interval too short");

                return;
            }
            watch.last_upgrade_ms = now;
        }

        {
            let mut entities = self.scripting.entities_mut();

            let (level, upgrade_ids) = {
                let Some(tank) = entities.tanks.get(entity_id) else {
                    return;
                };

                (*tank.level, tank.tank_type.upgrades.clone())
            };

            if !upgrade_ids.contains(&tank_id) {
                if let Some(watch) = self.watch.get_mut(&conn_id) {
                    watch.tracker.add_score(40);
                }

                // later: temp ban player
                error!(
                    "player {conn_id} requested tank {} ({}) outside their upgrade path",
                    tank_id, def.name
                );

                return;
            }

            if level < def.level_requirement {
                if let Some(watch) = self.watch.get_mut(&conn_id) {
                    watch.tracker.add_score(40);
                }

                // later: temp ban player
                error!("player {conn_id} requested tank {tank_id} below its level requirement");

                return;
            }

            entities.tanks.apply_def(entity_id, def);
            refresh_max_health(&mut entities, entity_id, &self.config);
        }

        info!("player {conn_id} upgraded to {} (tier {tier})", def.name);
        self.broadcast_player_def(conn_id, entity_id);
    }

    fn send_tank_upgrade_offers(&mut self) {
        let mut offers: Vec<(u32, Vec<TankOption>)> = Vec::new();

        {
            let mut entities = self.scripting.entities_mut();
            for (&conn_id, &entity_id) in self.players.iter() {
                if is_bot_id(conn_id, &self.config) {
                    continue;
                }

                let (current_id, level, upgrade_ids) = {
                    let Some(tank) = entities.tanks.get(entity_id) else {
                        continue;
                    };

                    (
                        tank.tank_type.id,
                        *tank.level,
                        tank.tank_type.upgrades.clone(),
                    )
                };

                if entities.tanks.offered_for(entity_id) == Some(current_id) {
                    continue;
                }

                let options: Vec<TankOption> = upgrade_ids
                    .iter()
                    .filter_map(|uid| find_def(&self.tank_tree, *uid))
                    .filter(|(_, def)| !def.flags.dev_only)
                    .filter(|(_, def)| level >= def.level_requirement)
                    .map(|(tier, def)| TankOption {
                        id: def.id,
                        name: def.name.clone(),
                        tier,
                        sides: def.sides,
                        barrels: barrel_defs(def),
                        turrets: turret_mounts(def, &self.config),
                    })
                    .collect();

                if options.is_empty() {
                    continue;
                }

                entities.tanks.set_offered(entity_id, current_id);
                offers.push((conn_id, options));
            }
        }

        for (conn_id, options) in offers {
            info!(
                "offering {} tank upgrades to player {conn_id}",
                options.len()
            );

            self.connections
                .send_to(conn_id, TankTreePacket::new(options, PACKET_SEED as u64));
        }
    }

    fn run_anti_cheat_pass(&mut self) {
        if self.tick_count % 100 == 0 {
            for watch in self.watch.values_mut() {
                watch.tracker.decay(self.config.anticheat.suspicion_decay);
            }
        }

        if self.tick_count % 10 != 0 {
            return;
        }

        let series: Vec<(u32, Vec<f32>)> = self
            .watch
            .iter()
            .filter(|(_, watch)| watch.aim.len() >= self.config.anticheat.multibox_min_samples)
            .map(|(id, watch)| (*id, watch.aim.angles()))
            .collect();

        // optimize this later:
        for i in 0..series.len() {
            for j in (i + 1)..series.len() {
                let similarity = multibox_anti::calculate_input_similarity(
                    &series[i].1,
                    &series[j].1,
                    self.config.anticheat.multibox_threshold_rad,
                );

                if similarity >= self.config.anticheat.multibox_similarity_flag {
                    let (a, b) = (series[i].0, series[j].0);

                    if let Some(watch) = self.watch.get_mut(&a) {
                        watch.tracker.add_score(20);
                    }

                    if let Some(watch) = self.watch.get_mut(&b) {
                        watch.tracker.add_score(20);
                    }

                    error!("multibox suspicion between players {a} and {b} ({similarity:.3})");
                }
            }
        }

        let flagged: Vec<(u32, u32)> = self
            .watch
            .iter()
            .filter(|(_, watch)| {
                !watch.reported && watch.tracker.is_flagged(SUSPICION_FLAG_THRESHOLD)
            })
            .map(|(id, watch)| (*id, watch.tracker.score))
            .collect();

        for (id, score) in flagged {
            if let Some(watch) = self.watch.get_mut(&id) {
                watch.reported = true;
            }
            error!("player {id} flagged by anti-cheat (score {score})");
        }
    }

    fn spawn_player(&mut self, id: u32, name: &str) {
        let entity_id = self.scripting.entities_mut().spawn_tank(
            Vec2::ZERO,
            Vec2::ZERO,
            self.config.player.default_health,
            name.to_string(),
        );
        self.players.insert(id, entity_id);
        self.interest.insert(id, HashMap::new());
        self.spawn_ticks.insert(id, self.tick_count);
        self.place_tank(id, entity_id);

        let (x, y, team) = {
            let entities = self.scripting.entities_mut();
            let pos = entities.positions[entity_id.index];
            (pos.x, pos.y, entities.tanks.team_of(entity_id))
        };

        let arena_packet = self.scripting.arena().packet();
        self.connections.send_to(id, arena_packet);
        self.send_tank_info(id, entity_id);

        let barrels = self
            .tank_tree
            .first()
            .and_then(|tier| tier.first())
            .map(barrel_defs)
            .unwrap_or_else(default_barrels);

        let my_packet = AddEntityPacket::new(
            id,
            EntityType::Player,
            x,
            y,
            1,
            name.to_string(),
            true,
            team,
            barrels.clone(),
            Vec::new(),
            PACKET_SEED as u64,
        );
        self.connections.send_to(id, my_packet);
        self.interest
            .entry(id)
            .or_default()
            .insert(id, EntityType::Player);

        let enter_sq = self.config.world.view_enter_rad.powi(2);
        let spawn_pos = Vec2::new(x, y);

        {
            let entities = self.scripting.entities_mut();
            for (&other_id, &other_entity_id) in self.players.iter() {
                if other_id == id {
                    continue;
                }

                if !entities.is_alive(other_entity_id) {
                    continue;
                }

                let other_pos = entities.positions[other_entity_id.index];
                if other_pos.distance_squared(spawn_pos) > enter_sq {
                    continue;
                }

                let Some(tank) = entities.tanks.get(other_entity_id) else {
                    continue;
                };

                // the new player sees them (bots included)
                let existing_packet = AddEntityPacket::new(
                    other_id,
                    EntityType::Player,
                    other_pos.x,
                    other_pos.y,
                    *tank.level,
                    tank.name.to_string(),
                    false,
                    *tank.team,
                    tank.barrels.clone(),
                    turret_mounts(tank.tank_type, &self.config),
                    PACKET_SEED as u64,
                );
                self.connections.send_to(id, existing_packet);
                self.interest
                    .entry(id)
                    .or_default()
                    .insert(other_id, EntityType::Player);

                // they see the new player (bots have nobody home)
                if !is_bot_id(other_id, &self.config) {
                    let their_packet = AddEntityPacket::new(
                        id,
                        EntityType::Player,
                        x,
                        y,
                        1,
                        name.to_string(),
                        false,
                        team,
                        barrels.clone(),
                        Vec::new(),
                        PACKET_SEED as u64,
                    );
                    self.connections.send_to(other_id, their_packet);
                    self.interest
                        .entry(other_id)
                        .or_default()
                        .insert(id, EntityType::Player);
                }
            }
        }
    }

    fn tick_shape_orbits(&mut self, dt: f32) {
        let mut entities = self.scripting.entities_mut();
        let Entities {
            shapes, positions, ..
        } = &mut *entities;

        shapes.tick(dt, positions);
    }

    fn prune_damage_log(&mut self) {
        if self.tick_count % 50 != 0 {
            return;
        }

        let window = (self.config.player.assist_window * TICK_RATE as f32) as u64;
        let tick = self.tick_count;
        let entities = self.scripting.entities_mut();

        self.damage_log.retain(|victim, entries| {
            entries.retain(|e| tick.saturating_sub(e.2) <= window);
            entities.is_alive(*victim) && !entries.is_empty()
        });
    }

    fn tick_auto_spin(&mut self, dt: f32) {
        let step = self.config.player.auto_spin_speed * dt;
        let mut entities = self.scripting.entities_mut();

        for &eid in self.players.values() {
            if let Some(t) = entities.tanks.get_mut(eid) {
                if *t.auto_spin {
                    *t.aim = (*t.aim + step).rem_euclid(std::f32::consts::TAU);
                }
            }
        }
    }

    fn regen_health(&mut self) {
        if self.tick_count % 10 != 0 {
            return;
        }

        let mut entities = self.scripting.entities_mut();
        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }

            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            let (max_health, regen) = match entities.tanks.get(id) {
                Some(t) => (
                    *t.max_health,
                    (*t.max_health as f32
                        * (self.config.player.stat_regen_base
                            + self.config.player.stat_regen_per * t.stat_levels[0] as f32))
                        as u32,
                ),

                None => continue,
            };

            let Some(health) = entities.health_mut(id) else {
                continue;
            };

            if *health > 0 && *health < max_health {
                *health = (*health + regen.max(1)).min(max_health);
            }
        }
    }

    fn clamp_shape_positions(&mut self) {
        let arena = self.scripting.arena();
        let mut entities = self.scripting.entities_mut();
        for center in entities.shapes.centers.iter_mut() {
            center.x = center
                .x
                .clamp(-self.config.world.map_bound, self.config.world.map_bound);
            center.y = center
                .y
                .clamp(-self.config.world.map_bound, self.config.world.map_bound);
        }

        if arena.walls().is_empty() {
            return;
        }

        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }

            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            let Some(radius) = entities.shapes.get(id).map(|s| shape_radius(*s.kind)) else {
                continue;
            };

            let pos = entities.positions[i];
            if let Some((out, _)) = arena.push_out(pos, radius) {
                entities.shapes.push_center(id, out - pos);
                entities.positions[i] = out;
            }
        }
    }

    fn rebuild_spatial_hash(&mut self) {
        self.spatial_hash.clear();

        let entities = self.scripting.entities_mut();
        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }

            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            let pos = entities.positions[i];
            self.spatial_hash
                .insert(HashEntity::Entity(id), pos.x, pos.y);
        }

        for (i, (_, pos, _, _, _, _)) in entities.bullets.iter().enumerate() {
            self.spatial_hash
                .insert(HashEntity::Bullet(i), pos.x, pos.y);
        }
    }

    fn rebuild_entity_hash(&mut self) {
        self.spatial_hash.clear();

        let entities = self.scripting.entities_mut();

        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }

            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            let pos = entities.positions[i];
            self.spatial_hash
                .insert(HashEntity::Entity(id), pos.x, pos.y);
        }
    }

    fn rebuild_bullet_hash(&mut self) {
        self.bullet_hash.clear();

        let entities = self.scripting.entities_mut();
        for (i, (_, pos, _, _, _, _)) in entities.bullets.iter().enumerate() {
            self.bullet_hash.insert(HashEntity::Bullet(i), pos.x, pos.y);
        }
    }

    fn broadcast_entity_updates(&mut self, dt: f32) {
        {
            let mut entities = self.scripting.entities_mut();
            let tank_jobs: Vec<(EntityId, Option<f32>, u32, f32, [u8; 8])> = self
                .players
                .par_iter()
                .filter_map(|(&_conn, &eid)| {
                    if !entities.is_alive(eid) {
                        return None;
                    }
                    let t = entities.tanks.get(eid)?;

                    Some((
                        eid,
                        *t.move_dir,
                        *t.level,
                        t.tank_type.speed,
                        *t.stat_levels,
                    ))
                })
                .collect();

            let arena = self.scripting.arena();

            for (eid, move_dir, level, speed_mult, stats) in tank_jobs {
                let max_speed = self.config.player.speed
                    * (1.0 + (level - 1) as f32 * 0.02)
                    * speed_mult
                    * stat_mult(&stats, 7, self.config.player.stat_move_per);

                step_tank_velocity(
                    &mut entities,
                    eid.index,
                    move_dir,
                    max_speed,
                    dt,
                    &self.config,
                );
                clamp_tank_position(&mut entities, eid.index, &self.config);

                let radius = self.config.player.radius * level_scale(level);
                let pos = entities.positions[eid.index];

                if let Some((out, normal)) = arena.push_out(pos, radius) {
                    entities.positions[eid.index] = out;

                    let Entities {
                        velocities,
                        recoil_velocities,
                        ..
                    } = &mut *entities;

                    for vel in [
                        &mut velocities[eid.index],
                        &mut recoil_velocities[eid.index],
                    ] {
                        let into = vel.dot(normal);
                        if into < 0.0 {
                            *vel -= normal * into;
                        }
                    }
                }
            }
        }

        let (dead_bullets, fresh_origins) = {
            let mut entities = self.scripting.entities_mut();
            let dead = entities.bullets.take_graveyard();
            let fresh = entities.bullets.fresh_origins();
            entities.bullets.mark_announced();
            (dead, fresh)
        };

        let mut viewers: Vec<(u32, Vec2)> = self
            .spectators
            .iter()
            .map(|(&id, cam)| (id, cam.pos))
            .collect();
        {
            let entities = self.scripting.entities_mut();
            for (&conn_id, &eid) in self.players.iter() {
                if is_bot_id(conn_id, &self.config) {
                    continue;
                }

                if !entities.is_alive(eid) {
                    continue;
                }

                viewers.push((conn_id, entities.positions[eid.index]));
            }
        }

        if viewers.is_empty() {
            return;
        }

        let entities = self.scripting.entities_mut();
        let slot_count = entities.alive.len();

        let conn_by_slot = &mut self.buf_conn_by_slot;
        conn_by_slot.clear();
        conn_by_slot.resize(slot_count, u32::MAX);

        for (&conn, &eid) in self.players.iter() {
            if eid.index < slot_count {
                conn_by_slot[eid.index] = conn;
            }
        }

        let mut all_updates: Vec<UpdateEntityPacketData> = Vec::with_capacity(slot_count);
        let slot_net_id = &mut self.buf_slot_net_id;
        let slot_is_player = &mut self.buf_slot_is_player;
        let slot_update = &mut self.buf_slot_update;

        slot_net_id.clear();
        slot_net_id.resize(slot_count, u32::MAX);
        slot_is_player.clear();
        slot_is_player.resize(slot_count, false);
        slot_update.clear();
        slot_update.resize(slot_count, usize::MAX);

        for i in 0..slot_count {
            if !entities.alive[i] {
                continue;
            }

            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            let health = entities.get(id).map(|e| *e.health).unwrap_or(0);

            if conn_by_slot[i] != u32::MAX {
                let Some(tank) = entities.tanks.get(id) else {
                    continue;
                };

                let conn = conn_by_slot[i];
                let max_health = *tank.max_health;

                slot_net_id[i] = conn;
                slot_is_player[i] = true;
                slot_update[i] = all_updates.len();

                all_updates.push(UpdateEntityPacketData {
                    id: conn,
                    entity_type: EntityType::Player,
                    x: entities.positions[i].x,
                    y: entities.positions[i].y,
                    rot: *tank.aim,
                    scale: level_scale(*tank.level),
                    kind: 0,
                    health,
                    max_health,
                    team: *tank.team,
                    turrets: self
                        .turrets
                        .get(&id)
                        .map(|t| t.aims.clone())
                        .unwrap_or_default(),
                });
            } else if let Some(shape) = entities.shapes.get(id) {
                let net_id = shape_net_id(i, entities.generations[i]);
                let max_health = shape_max_health(*shape.kind);

                slot_net_id[i] = net_id;
                slot_is_player[i] = false;
                slot_update[i] = all_updates.len();

                all_updates.push(UpdateEntityPacketData {
                    id: net_id,
                    entity_type: EntityType::Shape,
                    x: entities.positions[i].x,
                    y: entities.positions[i].y,
                    rot: *shape.rotation,
                    scale: 1.0,
                    kind: shape_kind_id(*shape.kind),
                    health,
                    max_health,
                    team: NO_TEAM,
                    turrets: Vec::new(),
                });
            }
        }

        let mut bullet_net_ids: Vec<u32> = Vec::with_capacity(entities.bullets.len());
        let mut bullet_update: Vec<usize> = Vec::with_capacity(entities.bullets.len());
        let mut bullet_origin_update: Vec<Option<usize>> =
            Vec::with_capacity(entities.bullets.len());

        for (i, (net_id, pos, radius, recoil, team, trap)) in entities.bullets.iter().enumerate() {
            let update = |p: Vec2| UpdateEntityPacketData {
                id: net_id,
                entity_type: if trap {
                    EntityType::Trap
                } else {
                    EntityType::Bullet
                },
                x: p.x,
                y: p.y,
                rot: 0.0,
                scale: radius,
                kind: recoil,
                health: 1,
                max_health: 1,
                team,
                turrets: Vec::new(),
            };

            bullet_net_ids.push(net_id);

            match fresh_origins.get(i).copied().flatten() {
                Some(origin) => {
                    bullet_origin_update.push(Some(all_updates.len()));
                    all_updates.push(update(origin));
                }
                None => bullet_origin_update.push(None),
            }

            bullet_update.push(all_updates.len());
            all_updates.push(update(*pos));
        }

        let drone_updates: Vec<(Vec2, UpdateEntityPacketData)> = entities
            .drones
            .iter_net()
            .map(
                |(net_id, pos, facing, radius, recoil, health, max_health, team)| {
                    (
                        pos,
                        UpdateEntityPacketData {
                            id: net_id,
                            entity_type: EntityType::Drone,
                            x: pos.x,
                            y: pos.y,
                            rot: facing,
                            scale: radius,
                            kind: recoil,
                            health: health.ceil() as u32,
                            max_health: max_health.ceil() as u32,
                            team,
                            turrets: Vec::new(),
                        },
                    )
                },
            )
            .collect();

        let dead_updates: Vec<(Vec2, Option<UpdateEntityPacketData>, UpdateEntityPacketData)> =
            dead_bullets
                .iter()
                .map(|d| {
                    let update = |p: Vec2, health: u32| UpdateEntityPacketData {
                        id: d.net_id,
                        entity_type: if d.trap {
                            EntityType::Trap
                        } else {
                            EntityType::Bullet
                        },
                        x: p.x,
                        y: p.y,
                        rot: 0.0,
                        scale: d.radius,
                        kind: d.recoil,
                        health,
                        max_health: 1,
                        team: d.team,
                        turrets: Vec::new(),
                    };

                    (d.pos, d.origin.map(|o| update(o, 1)), update(d.pos, 0))
                })
                .collect();

        let enter_sq = self.config.world.view_enter_rad.powi(2);
        let exit_sq = self.config.world.view_exit_rad.powi(2);

        let entity_seen = &mut self.buf_entity_seen;
        let bullet_seen = &mut self.buf_bullet_seen;

        entity_seen.clear();
        entity_seen.resize(slot_count, false);
        bullet_seen.clear();
        bullet_seen.resize(bullet_net_ids.len(), false);

        let mut seen_ids: HashSet<u32> = HashSet::new();
        let mut nearby: Vec<HashEntity> = Vec::with_capacity(512);

        for (viewer_conn, viewer_pos) in viewers {
            let mut viewer_updates: Vec<UpdateEntityPacketData> = Vec::new();

            entity_seen.fill(false);
            bullet_seen.fill(false);
            seen_ids.clear();

            self.spatial_hash.get_nearby_into(
                &mut nearby,
                viewer_pos.x,
                viewer_pos.y,
                self.config.world.view_exit_rad,
            );

            {
                let visible = self.interest.entry(viewer_conn).or_default();

                for &candidate in nearby.iter() {
                    match candidate {
                        HashEntity::Entity(eid) => {
                            let slot = eid.index;

                            if slot >= slot_count || !entities.is_alive(eid) {
                                continue;
                            }

                            let net_id = slot_net_id[slot];
                            if net_id == u32::MAX {
                                continue;
                            }

                            let pos = entities.positions[slot];
                            let dist_sq = pos.distance_squared(viewer_pos);
                            let in_enter = dist_sq <= enter_sq;
                            let was_visible = visible.contains_key(&net_id);

                            if !in_enter && !(was_visible && dist_sq <= exit_sq) {
                                continue;
                            }

                            if entity_seen[slot] {
                                continue;
                            }

                            entity_seen[slot] = true;
                            seen_ids.insert(net_id);

                            if !was_visible {
                                let packet = if slot_is_player[slot] {
                                    let Some(tank) = entities.tanks.get(eid) else {
                                        continue;
                                    };

                                    AddEntityPacket::new(
                                        net_id,
                                        EntityType::Player,
                                        pos.x,
                                        pos.y,
                                        *tank.level,
                                        tank.name.to_string(),
                                        net_id == viewer_conn,
                                        *tank.team,
                                        tank.barrels.clone(),
                                        turret_mounts(tank.tank_type, &self.config),
                                        PACKET_SEED as u64,
                                    )
                                } else {
                                    let Some(shape) = entities.shapes.get(eid) else {
                                        continue;
                                    };

                                    AddEntityPacket::new(
                                        net_id,
                                        EntityType::Shape,
                                        pos.x,
                                        pos.y,
                                        shape_kind_id(*shape.kind),
                                        String::new(),
                                        false,
                                        NO_TEAM,
                                        vec![],
                                        vec![],
                                        PACKET_SEED as u64,
                                    )
                                };

                                self.connections.send_to(viewer_conn, packet);

                                visible.insert(
                                    net_id,
                                    if slot_is_player[slot] {
                                        EntityType::Player
                                    } else {
                                        EntityType::Shape
                                    },
                                );
                            }

                            let idx = slot_update[slot];
                            if idx != usize::MAX {
                                viewer_updates.push(all_updates[idx].clone());
                            }
                        }
                        HashEntity::Bullet(bullet_index) => {
                            if bullet_index >= bullet_net_ids.len() {
                                continue;
                            }

                            let pos = entities.bullets.position(bullet_index);
                            if pos.distance_squared(viewer_pos) > exit_sq {
                                continue;
                            }

                            if bullet_seen[bullet_index] {
                                continue;
                            }

                            bullet_seen[bullet_index] = true;

                            if let Some(origin) = bullet_origin_update[bullet_index] {
                                viewer_updates.push(all_updates[origin].clone());
                            }

                            viewer_updates.push(all_updates[bullet_update[bullet_index]].clone());
                        }
                    }
                }

                let mut exits: Vec<u32> = Vec::new();
                for &net_id in visible.keys() {
                    if !seen_ids.contains(&net_id) {
                        exits.push(net_id);
                    }
                }

                for net_id in exits {
                    if let Some(entity_type) = visible.remove(&net_id) {
                        self.connections.send_to(
                            viewer_conn,
                            RemoveEntityPacket::new(net_id, entity_type, PACKET_SEED as u64),
                        );
                    }
                }
            }

            for (pos, update) in drone_updates.iter() {
                if pos.distance_squared(viewer_pos) <= exit_sq {
                    viewer_updates.push(update.clone());
                }
            }

            for (pos, spawn, death) in dead_updates.iter() {
                if pos.distance_squared(viewer_pos) <= exit_sq {
                    if let Some(spawn) = spawn {
                        viewer_updates.push(spawn.clone());
                    }
                    viewer_updates.push(death.clone());
                }
            }

            self.connections.send_to(
                viewer_conn,
                UpdateEntityPacket::new(viewer_updates, PACKET_SEED as u64),
            );
        }
    }

    fn broadcast_player_stats(&mut self) {
        let entities = self.scripting.entities_mut();
        for (&conn_id, &entity_id) in self.players.iter() {
            if is_bot_id(conn_id, &self.config) {
                continue;
            }

            let Some(tank) = entities.tanks.get(entity_id) else {
                continue;
            };

            let health = entities.get(entity_id).map(|e| *e.health).unwrap_or(0);
            let packet = PlayerStatsPacket::new(
                *tank.level,
                *tank.score,
                tank.xp.0,
                tank.xp.1,
                health,
                *tank.max_health,
                PACKET_SEED as u64,
            );
            self.connections.send_to(conn_id, packet);

            let upgrades =
                PlayerUpgradesPacket::new(*tank.stat_levels, *tank.stat_points, PACKET_SEED as u64);
            self.connections.send_to(conn_id, upgrades);
        }
    }

    fn broadcast_leaderboard(&mut self) {
        let entities = self.scripting.entities_mut();
        let mut leaderboard: Vec<(String, u32)> = self
            .players
            .iter()
            .filter_map(|(_, &entity_id)| {
                let tank = entities.tanks.get(entity_id)?;
                Some((tank.name.to_string(), *tank.score))
            })
            .collect();

        leaderboard.sort_by(|a, b| b.1.cmp(&a.1));
        leaderboard.truncate(10);

        self.connections
            .broadcast(LeaderboardPacket::new(leaderboard, PACKET_SEED as u64));
    }

    fn simulate_combat(&mut self, dt: f32) {
        let sub_dt = dt / self.config.world.bullet_substeps as f32;

        let ids: Vec<EntityId> = self.players.values().copied().collect();
        let entity_to_conn = self.entity_to_conn();

        self.rebuild_entity_hash();
        self.pick_drone_targets();
        self.aim_turrets(dt);

        for _ in 0..self.config.world.bullet_substeps {
            self.fire_auto_weapons(sub_dt, &ids, &entity_to_conn);
            self.fire_turrets(sub_dt, &entity_to_conn);
            self.scripting
                .entities_mut()
                .bullets
                .tick(sub_dt, self.config.trap.drag);
            self.update_drones(sub_dt, &entity_to_conn);
            self.rebuild_bullet_hash();
            self.resolve_bullet_collisions(&entity_to_conn);
            self.rebuild_bullet_hash();
            self.resolve_drone_collisions(sub_dt, &entity_to_conn);
        }
    }

    fn aim_turrets(&mut self, dt: f32) {
        let cfg = &self.config.turret;
        let mut nearby: Vec<HashEntity> = Vec::with_capacity(64);
        let mut armed: HashSet<EntityId> = HashSet::new();
        let entities = self.scripting.entities_mut();

        for &eid in self.players.values() {
            let Some(t) = entities.tanks.get(eid) else {
                continue;
            };
            let Some(layout) = turret_layout(t.tank_type, &self.config) else {
                continue;
            };

            armed.insert(eid);

            let count = layout.count as usize;
            let scale = level_scale(*t.level);
            let body = self.config.player.radius * scale;
            let range = cfg.range * scale;
            let pos = entities.positions[eid.index];
            let aim = *t.aim;

            let state = self.turrets.entry(eid).or_insert_with(|| TurretState {
                aims: Vec::new(),
                timers: Vec::new(),
                targets: Vec::new(),
            });

            if state.aims.len() != count {
                state.aims = vec![aim; count];
                state.timers = vec![0.0; count];
                state.targets = vec![None; count];
            }

            for i in 0..count {
                let mount =
                    pos + Vec2::from_angle(aim + mount_angle(i, count)) * layout.distance * body;

                self.spatial_hash
                    .get_nearby_into(&mut nearby, mount.x, mount.y, range);

                let mut best_tank: Option<(f32, Vec2)> = None;
                let mut best_shape: Option<(u32, f32, Vec2)> = None;

                for &candidate in nearby.iter() {
                    let HashEntity::Entity(id) = candidate else {
                        continue;
                    };

                    if id == eid || !entities.is_alive(id) {
                        continue;
                    }

                    let p = entities.positions[id.index];
                    let d = p.distance(mount);
                    if d > range {
                        continue;
                    }

                    if entities.tanks.get(id).is_some() {
                        if !entities.tanks.allied(eid, id) && best_tank.is_none_or(|(bd, _)| d < bd)
                        {
                            best_tank = Some((d, p));
                        }
                    } else if let Some(shape) = entities.shapes.get(id) {
                        let xp = *shape.xp_reward;
                        let better =
                            best_shape.is_none_or(|(bx, bd, _)| xp > bx || (xp == bx && d < bd));
                        if better {
                            best_shape = Some((xp, d, p));
                        }
                    }
                }

                let target = best_tank.map(|(_, p)| p).or(best_shape.map(|(_, _, p)| p));
                state.targets[i] = target;

                if let Some(target) = target {
                    let to = target - mount;
                    let want = to.y.atan2(to.x);
                    state.aims[i] = turn_toward(state.aims[i], want, cfg.turn_speed * dt);
                }
            }
        }

        self.turrets.retain(|id, _| armed.contains(id));
    }

    fn fire_turrets(&mut self, dt: f32, entity_to_conn: &HashMap<EntityId, u32>) {
        if self.turrets.is_empty() {
            return;
        }

        let cfg = &self.config.turret;
        let mut entities = self.scripting.entities_mut();

        for (&eid, state) in self.turrets.iter_mut() {
            if !entities.is_alive(eid) {
                continue;
            }

            let Some((level, stats, team, aim, barrel_count, distance)) =
                entities.tanks.get(eid).and_then(|t| {
                    turret_layout(t.tank_type, &self.config).map(|layout| {
                        (
                            *t.level,
                            *t.stat_levels,
                            *t.team,
                            *t.aim,
                            t.tank_type.barrels.len(),
                            layout.distance,
                        )
                    })
                })
            else {
                continue;
            };

            let scale = level_scale(level);
            let body = self.config.player.radius * scale;
            let pos = entities.positions[eid.index];
            let count = state.aims.len();

            let owner_conn = entity_to_conn.get(&eid).copied().unwrap_or(u32::MAX);
            let strength = if is_bot_id(owner_conn, &self.config) {
                self.config.bots.bullet_strength
            } else {
                1.0
            };

            let reload = cfg.reload / stat_mult(&stats, 6, self.config.player.stat_reload_per);
            let base_damage = self.config.bullet.base_health
                * (1.0 + self.config.bullet.dmg_per_level * (level - 1) as f32);
            let damage = ((base_damage
                * cfg.damage_mult
                * stat_mult(&stats, 5, self.config.player.stat_bullet_dmg_per)
                * strength)
                .round() as u32)
                .max(1);
            let speed = self.config.player.speed
                * self.config.bullet.speed_mult
                * cfg.bullet_speed_mult
                * stat_mult(&stats, 3, self.config.player.stat_bullet_speed_per);
            let health = self.config.bullet.base_health
                * cfg.bullet_health_mult
                * stat_mult(&stats, 4, self.config.player.stat_bullet_hp_per)
                * strength;
            let radius = cfg.bullet_radius * scale;

            for i in 0..count {
                state.timers[i] -= dt;
                if state.timers[i] > 0.0 {
                    continue;
                }

                let Some(target) = state.targets[i] else {
                    state.timers[i] = 0.0;
                    continue;
                };

                let mount = pos + Vec2::from_angle(aim + mount_angle(i, count)) * distance * body;
                let to = target - mount;
                let off = angle_diff(to.y.atan2(to.x), state.aims[i]);

                if off.abs() > cfg.aim_tolerance {
                    state.timers[i] = 0.0;
                    continue;
                }

                state.timers[i] += reload;

                let dir = Vec2::from_angle(state.aims[i]);

                entities.bullets.spawn(
                    mount + dir * cfg.muzzle * body * 0.5,
                    dir * speed,
                    damage,
                    self.config.bullet.lifetime,
                    eid,
                    radius,
                    health,
                    recoil_info(owner_conn, barrel_count + i, &self.config),
                    team,
                    false,
                );
            }
        }
    }

    fn pick_drone_targets(&mut self) {
        let range = self.config.drone.ai_range;
        let mut nearby: Vec<HashEntity> = Vec::with_capacity(64);

        let arena = self.scripting.arena();
        let mut entities = self.scripting.entities_mut();

        if entities.drones.len() == 0 {
            return;
        }

        let mut owners: Vec<EntityId> = (0..entities.drones.len())
            .map(|i| entities.drones.owner(i))
            .collect();
        owners.sort_unstable_by_key(|o| (o.index, o.generation));
        owners.dedup();

        let mut candidates: HashMap<EntityId, Vec<(EntityId, Vec2)>> = HashMap::new();

        for owner in owners {
            let Some(scale) = entities.tanks.get(owner).map(|t| level_scale(*t.level)) else {
                continue;
            };

            let center = entities.positions[owner.index];
            let reach = range * scale;

            self.spatial_hash
                .get_nearby_into(&mut nearby, center.x, center.y, reach);

            let mut list: Vec<(EntityId, Vec2)> = Vec::new();

            for &candidate in nearby.iter() {
                let HashEntity::Entity(id) = candidate else {
                    continue;
                };

                if id == owner || !entities.is_alive(id) || entities.tanks.allied(owner, id) {
                    continue;
                }

                if entities.shapes.get(id).is_none() && entities.tanks.get(id).is_none() {
                    continue;
                }

                let pos = entities.positions[id.index];
                if pos.distance_squared(center) > reach * reach
                    || list.iter().any(|(other, _)| *other == id)
                    || arena.segment_hits_wall(center, pos, 0.0)
                {
                    continue;
                }

                list.push((id, pos));
            }

            candidates.insert(owner, list);
        }

        for i in 0..entities.drones.len() {
            let owner = entities.drones.owner(i);
            let pos = entities.drones.position(i);

            let target = candidates
                .get(&owner)
                .and_then(|list| {
                    list.iter().min_by(|a, b| {
                        a.1.distance_squared(pos)
                            .total_cmp(&b.1.distance_squared(pos))
                    })
                })
                .map(|(id, _)| *id);

            entities.drones.set_ai_target(i, target);
        }
    }

    fn update_drones(&mut self, dt: f32, entity_to_conn: &HashMap<EntityId, u32>) {
        let cfg = &self.config.drone;
        let arena = self.scripting.arena();
        let mut entities = self.scripting.entities_mut();

        for i in (0..entities.drones.len()).rev() {
            let still_valid = match entities.drones.zone_at(i) {
                Some(zone) => arena.zones().iter().any(|z| z.id == zone),
                None => {
                    let owner = entities.drones.owner(i);
                    let barrel = entities.drones.barrel_at(i);
                    entities.is_alive(owner)
                        && entities.tanks.get(owner).is_some_and(|t| {
                            t.tank_type
                                .barrels
                                .get(barrel)
                                .and_then(drone_cap)
                                .is_some()
                        })
                }
            };

            if !still_valid || entities.drones.is_expired(i) {
                entities.drones.remove(i);
            }
        }

        let intruders: Vec<(u32, Vec2, Vec2, f32, Vec<Vec2>)> = arena
            .zones()
            .iter()
            .filter(|z| z.team != NO_TEAM && z.spawn)
            .map(|z| {
                let margin = Vec2::splat(self.config.base.chase_margin);
                let (min, max) = (z.min - margin, z.max + margin);
                let enemies = self
                    .players
                    .values()
                    .filter(|&&eid| entities.is_alive(eid) && entities.tanks.team_of(eid) != z.team)
                    .map(|&eid| entities.positions[eid.index])
                    .filter(|p| p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y)
                    .collect();
                let half = (z.max - z.min) * 0.5;

                (z.id, z.min + half, half, half.min_element(), enemies)
            })
            .collect();

        for i in 0..entities.drones.len() {
            if let Some(zone) = entities.drones.zone_at(i) {
                let Some((_, center, _, size, enemies)) =
                    intruders.iter().find(|(id, ..)| *id == zone)
                else {
                    continue;
                };

                let pos = entities.drones.position(i);
                let goal = enemies
                    .iter()
                    .min_by(|a, b| a.distance_squared(pos).total_cmp(&b.distance_squared(pos)))
                    .map(|&p| DroneGoal::Chase(p))
                    .unwrap_or(DroneGoal::Idle {
                        owner: *center,
                        owner_radius: size * 0.25,
                    });

                entities.drones.set_goal(i, goal);
                continue;
            }

            let owner = entities.drones.owner(i);
            let owner_pos = entities.positions[owner.index];

            let Some((firing, repel, aim, scale)) = entities
                .tanks
                .get(owner)
                .map(|t| (*t.auto_fire, *t.repel, *t.aim, level_scale(*t.level)))
            else {
                continue;
            };

            let cursor = entity_to_conn
                .get(&owner)
                .and_then(|conn| self.mouse_offsets.get(conn))
                .map(|offset| owner_pos + *offset);

            let ai_target = entities
                .drones
                .ai_target(i)
                .filter(|&t| entities.is_alive(t))
                .map(|t| entities.positions[t.index]);

            let goal = match (repel, firing, cursor, ai_target) {
                (true, _, Some(cursor), _) => DroneGoal::Repel(cursor),
                (_, true, Some(cursor), _) => DroneGoal::Chase(cursor),
                (_, true, None, _) => {
                    DroneGoal::Chase(owner_pos + Vec2::from_angle(aim) * cfg.bot_reach * scale)
                }
                (_, false, _, Some(target)) => DroneGoal::Chase(target),
                _ => DroneGoal::Idle {
                    owner: owner_pos,
                    owner_radius: self.config.player.radius * scale,
                },
            };

            entities.drones.set_goal(i, goal);
        }

        entities.drones.tick(dt, cfg);
        entities.drones.resolve_walls(&arena);
    }

    fn resolve_drone_collisions(&mut self, dt: f32, entity_to_conn: &HashMap<EntityId, u32>) {
        let mut nearby: Vec<HashEntity> = Vec::with_capacity(64);

        let mut contacts: Vec<(usize, EntityId, Vec2, bool)> = Vec::new();
        let mut bullet_contacts: Vec<(usize, usize)> = Vec::new();

        {
            let entities = self.scripting.entities_mut();

            for (i, pos, _damage, radius, owner) in entities.drones.iter_indexed() {
                let team = entities.drones.team_at(i);

                self.spatial_hash
                    .get_nearby_into(&mut nearby, pos.x, pos.y, 40.0 + radius);

                for &candidate in nearby.iter() {
                    let HashEntity::Entity(target_id) = candidate else {
                        continue;
                    };

                    if target_id == owner || contacts.iter().any(|c| c.0 == i && c.1 == target_id) {
                        continue;
                    }

                    let Some(target) = entities.get(target_id) else {
                        continue;
                    };

                    let Some((target_radius, is_tank)) =
                        entity_collision_radius(&entities, target_id, &self.config)
                    else {
                        continue;
                    };

                    if is_tank && teams_allied(team, entities.tanks.team_of(target_id)) {
                        continue;
                    }

                    let delta = pos - *target.position;
                    let dist = delta.length();
                    if dist > target_radius + radius {
                        continue;
                    }

                    let dir = if dist > 1e-4 { delta / dist } else { Vec2::X };
                    contacts.push((i, target_id, dir, is_tank));
                }

                self.bullet_hash
                    .get_nearby_into(&mut nearby, pos.x, pos.y, radius + 12.0);

                for &candidate in nearby.iter() {
                    let HashEntity::Bullet(j) = candidate else {
                        continue;
                    };

                    if j >= entities.bullets.len()
                        || entities.bullets.owner(j) == owner
                        || teams_allied(team, entities.bullets.team_at(j))
                    {
                        continue;
                    }

                    if pos.distance(entities.bullets.position(j))
                        <= radius + entities.bullets.radius_at(j)
                    {
                        bullet_contacts.push((i, j));
                    }
                }
            }
        }

        let cfg = &self.config.drone;
        let mut killed: Vec<(EntityId, Option<EntityId>)> = Vec::new();

        {
            let mut entities = self.scripting.entities_mut();

            for (i, j) in entities.drones.resolve_pairs(dt, cfg.drone_push) {
                let (key_i, key_j) = (
                    HitKey::Drone(entities.drones.net_id(i)),
                    HitKey::Drone(entities.drones.net_id(j)),
                );

                if !entities.drones.can_hit(i, key_j) {
                    continue;
                }

                entities.drones.record_hit(i, key_j, cfg.hit_cooldown);
                entities.drones.record_hit(j, key_i, cfg.hit_cooldown);

                let (dmg_i, dmg_j) = (
                    entities.drones.damage_at(i) as f32,
                    entities.drones.damage_at(j) as f32,
                );
                entities.drones.damage(i, dmg_j);
                entities.drones.damage(j, dmg_i);
            }

            let mut spent_bullets: Vec<usize> = Vec::new();
            for &(i, j) in &bullet_contacts {
                if spent_bullets.contains(&j) || entities.drones.is_expired(i) {
                    continue;
                }

                let bullet_hp = entities.bullets.health(j);
                let drone_hp = entities.drones.health(i);

                entities.drones.damage(i, bullet_hp);
                if entities.bullets.damage(j, drone_hp) {
                    spent_bullets.push(j);
                }
            }

            spent_bullets.sort_unstable();
            for j in spent_bullets.into_iter().rev() {
                entities.bullets.remove(j);
            }

            for &(i, target_id, dir, is_tank) in &contacts {
                entities.drones.push(i, dir, cfg.contact_push, dt);

                if is_tank {
                    entities.recoil_velocities[target_id.index] -= dir * cfg.target_push * dt;
                } else {
                    entities
                        .shapes
                        .push_center(target_id, -dir * cfg.shape_push * dt);
                }

                let key = HitKey::Entity(target_id);
                if entities.drones.is_expired(i) || !entities.drones.can_hit(i, key) {
                    continue;
                }

                let owner = entities.drones.owner(i);
                let damage = entities.drones.damage_at(i);
                let taken =
                    entity_contact_dps(&entities, target_id, &self.config) * cfg.hit_cooldown;

                entities.drones.record_hit(i, key, cfg.hit_cooldown);
                entities.drones.damage(i, taken);

                if let Some(&t_conn) = entity_to_conn.get(&target_id) {
                    if is_bot_id(t_conn, &self.config) {
                        if let Some(&a_conn) = entity_to_conn.get(&owner) {
                            if let Some(brain) = self.bots.get_mut(&t_conn) {
                                brain.aggro_on = Some(a_conn);
                                brain.aggro_tick = self.tick_count;
                                brain.force_macro = true;
                            }
                        }
                    }
                }

                log_damage(
                    &mut self.damage_log,
                    &entities,
                    target_id,
                    owner,
                    damage,
                    self.tick_count,
                );

                let died = {
                    let Some(health) = entities.health_mut(target_id) else {
                        continue;
                    };

                    let was_alive = *health > 0;
                    *health = health.saturating_sub(damage);
                    was_alive && *health == 0
                };

                if died {
                    killed.push((target_id, Some(owner)));
                }
            }

            entities.drones.remove_expired();
        }

        self.finish_kills(killed, entity_to_conn);
    }

    fn finish_kills(
        &mut self,
        killed: Vec<(EntityId, Option<EntityId>)>,
        entity_to_conn: &HashMap<EntityId, u32>,
    ) {
        if killed.is_empty() {
            return;
        }

        let mut level_ups: Vec<(u32, u32)> = Vec::new();
        let assist_ticks = (self.config.player.assist_window * TICK_RATE as f32) as u64;

        {
            let mut entities = self.scripting.entities_mut();

            for (target_id, killer) in killed {
                if !entities.is_alive(target_id) {
                    continue;
                }

                let victim_conn = entity_to_conn.get(&target_id).copied();
                let victim_pos = entities.positions[target_id.index];
                let killer_conn = killer.and_then(|k| entity_to_conn.get(&k).copied());
                let shape_kind = entities.shapes.get(target_id).map(|s| *s.kind);

                let assists = self.damage_log.remove(&target_id).unwrap_or_default();
                let tank_reward = entities
                    .tanks
                    .get(target_id)
                    .map(|t| *t.score as f32 * self.config.player.kill_score_frac);

                let rewards = match tank_reward {
                    Some(reward) => split_reward(
                        reward,
                        &assists,
                        killer,
                        &entities,
                        self.tick_count,
                        assist_ticks,
                    ),
                    None => match (killer, entities.shapes.get(target_id)) {
                        (Some(k), Some(shape)) => vec![(k, *shape.xp_reward)],
                        _ => Vec::new(),
                    },
                };

                for (attacker, xp) in rewards {
                    if let Some(level) = grant_xp(&mut entities, attacker, xp, &self.config) {
                        if let Some(&conn) = entity_to_conn.get(&attacker) {
                            level_ups.push((conn, level));
                        }
                    }
                }

                if let Some(conn) = victim_conn.filter(|&c| !is_bot_id(c, &self.config)) {
                    let alive_ticks = self
                        .spawn_ticks
                        .get(&conn)
                        .map(|&at| self.tick_count.saturating_sub(at))
                        .unwrap_or(0);

                    self.connections.send_to(
                        conn,
                        death_packet(&entities, target_id, killer, (alive_ticks / 10) as u32),
                    );
                }

                if entities.tanks.get(target_id).is_some() {
                    if let Some(t) = killer.and_then(|k| entities.tanks.get_mut(k)) {
                        *t.kills += 1;
                    }
                }

                broadcast_despawn(&self.connections, &mut entities, entity_to_conn, target_id);

                if let Some(conn_id) = victim_conn {
                    self.players.remove(&conn_id);
                    forget_entity(&mut self.interest, conn_id);

                    if is_bot_id(conn_id, &self.config) {
                        self.bots.remove(&conn_id);
                        self.bot_respawns
                            .push((conn_id, self.tick_count + self.config.bots.respawn_ticks));
                    }

                    self.lua_events.push(GameEvents::PlayerDeath {
                        id: conn_id,
                        killer: killer_conn,
                    });

                    if !is_bot_id(conn_id, &self.config) {
                        let hold = (self.config.spectate.death_cam * TICK_RATE as f32) as u64;
                        self.spectators.insert(
                            conn_id,
                            SpectatorCam::at(victim_pos, self.tick_count + hold),
                        );
                    }
                } else {
                    forget_entity(
                        &mut self.interest,
                        shape_net_id(target_id.index, target_id.generation),
                    );

                    if let (Some(kind), Some(conn)) = (shape_kind, killer_conn) {
                        self.lua_events.push(GameEvents::ShapeKill {
                            id: conn,
                            kind: shape_kind_name(kind),
                        });
                    }
                }
            }
        }

        for (id, level) in level_ups {
            self.lua_events.push(GameEvents::LevelUp { id, level });
        }
    }

    fn resolve_entity_collisions(&mut self) {
        let dt = 0.1;

        let mut nearby: Vec<HashEntity> = Vec::with_capacity(64);

        let mut entities = self.scripting.entities_mut();

        let collidable = &mut self.buf_collidable;
        collidable.clear();
        collidable.resize(entities.alive.len(), None);

        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }

            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            if let Some(t) = entities.tanks.get(id) {
                let radius = self.config.player.radius * level_scale(*t.level);
                let dps = self.config.player.body_dps
                    * stat_mult(t.stat_levels, 2, self.config.player.stat_body_dmg_per);

                collidable[i] = Some((entities.positions[i], radius, radius * radius, true, dps));
            } else if let Some(s) = entities.shapes.get(id) {
                let radius = shape_radius(*s.kind);
                let dps = match *s.kind {
                    ShapeKind::Square => self.config.player.square_contact_dps,
                    ShapeKind::Triangle => self.config.player.triangle_contact_dps,
                    ShapeKind::Pentagon => self.config.player.pentagon_contact_dps,
                };

                collidable[i] = Some((
                    entities.positions[i],
                    radius,
                    radius * radius * 0.5,
                    false,
                    dps,
                ));
            }
        }

        let mut hits: Vec<(EntityId, u32, Option<EntityId>)> = Vec::new();

        for i in 0..collidable.len() {
            let Some((pos1, r1, m1, is_tank1, dps1)) = collidable[i] else {
                continue;
            };
            let id1 = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            self.spatial_hash.get_nearby_into(
                &mut nearby,
                pos1.x,
                pos1.y,
                self.config.world.collision_query_rad,
            );

            for &candidate in nearby.iter() {
                let HashEntity::Entity(id2) = candidate else {
                    continue;
                };

                if id2.index <= i {
                    continue;
                }

                let Some((pos2, r2, m2, is_tank2, dps2)) = collidable[id2.index] else {
                    continue;
                };

                let delta = pos2 - pos1;
                let dist = delta.length();
                let min_dist = r1 + r2;

                if dist >= min_dist {
                    continue;
                }

                let dir = if dist > 1e-4 {
                    delta / dist
                } else {
                    Vec2::new(1.0, 0.0)
                };
                let overlap = min_dist - dist;

                let inv1 = 1.0 / m1;
                let inv2 = 1.0 / m2;
                let corr = (overlap - self.config.player.collision_slop).max(0.0)
                    * self.config.player.collision_correction
                    / (inv1 + inv2);

                if is_tank1 {
                    entities.positions[i] -= dir * corr * inv1;

                    let v = entities.velocities[i];
                    let into = v.dot(dir);
                    if into > 0.0 {
                        entities.velocities[i] = v - dir * into;
                    }
                } else {
                    entities.shapes.push_center(id1, -dir * corr * inv1);
                }

                if is_tank2 {
                    entities.positions[id2.index] += dir * corr * inv2;
                    let v = entities.velocities[id2.index];
                    let into = v.dot(dir);
                    if into < 0.0 {
                        entities.velocities[id2.index] = v - dir * into;
                    }
                } else {
                    entities.shapes.push_center(id2, dir * corr * inv2);
                }

                let dmg1 = ((dps1 * dt).round() as u32).max(1);
                let dmg2 = ((dps2 * dt).round() as u32).max(1);
                match (is_tank1, is_tank2) {
                    (true, true) if entities.tanks.allied(id1, id2) => {}
                    (true, true) => {
                        hits.push((id1, dmg2, Some(id2)));
                        hits.push((id2, dmg1, Some(id1)));
                    }
                    (true, false) => {
                        hits.push((id1, dmg2, Some(id2)));
                        hits.push((id2, dmg1, Some(id1)));
                    }
                    (false, true) => {
                        hits.push((id1, dmg2, Some(id2)));
                        hits.push((id2, dmg1, Some(id1)));
                    }
                    (false, false) => {
                        // shapes never damage each other
                        // maybe later tho
                    }
                }
            }
        }

        drop(entities);

        if hits.is_empty() {
            return;
        }

        let entity_to_conn = self.entity_to_conn();
        let mut killed: Vec<(EntityId, Option<EntityId>)> = Vec::new();
        let mut entities = self.scripting.entities_mut();

        for (target_id, damage, attacker) in hits {
            if let Some(attacker) = attacker {
                log_damage(
                    &mut self.damage_log,
                    &entities,
                    target_id,
                    attacker,
                    damage,
                    self.tick_count,
                );
            }

            let died = {
                let Some(health) = entities.health_mut(target_id) else {
                    continue;
                };

                let was_alive = *health > 0;
                *health = health.saturating_sub(damage);
                was_alive && *health == 0
            };

            // holy nested code
            if let Some(&t_conn) = entity_to_conn.get(&target_id) {
                if is_bot_id(t_conn, &self.config) {
                    if let Some(attacker) = attacker {
                        if let Some(&a_conn) = entity_to_conn.get(&attacker) {
                            if let Some(brain) = self.bots.get_mut(&t_conn) {
                                brain.aggro_on = Some(a_conn);
                                brain.aggro_tick = self.tick_count;
                                brain.force_macro = true;
                            }
                        }
                    }
                }
            }

            if died {
                killed.push((target_id, attacker));
            }
        }

        drop(entities);
        self.finish_kills(killed, &entity_to_conn);
    }

    fn resolve_bullet_collisions(&mut self, entity_to_conn: &HashMap<EntityId, u32>) {
        let mut nearby: Vec<HashEntity> = Vec::with_capacity(64);

        let entity_hits: Vec<(usize, EntityId, u32, EntityId)> = {
            let entities = self.scripting.entities_mut();
            let mut hits = Vec::new();

            for (bullet_index, pos, damage, bullet_radius, owner) in entities.bullets.iter_indexed()
            {
                self.spatial_hash
                    .get_nearby_into(&mut nearby, pos.x, pos.y, 40.0 + bullet_radius);

                for &candidate in nearby.iter() {
                    let HashEntity::Entity(target_id) = candidate else {
                        continue;
                    };

                    if target_id == *owner {
                        continue;
                    }

                    if entities.bullets.has_hit(bullet_index, target_id) {
                        continue;
                    }

                    let Some(target) = entities.get(target_id) else {
                        continue;
                    };

                    let Some((target_radius, is_tank)) =
                        entity_collision_radius(&entities, target_id, &self.config)
                    else {
                        continue;
                    };

                    if is_tank
                        && teams_allied(
                            entities.bullets.team_at(bullet_index),
                            entities.tanks.team_of(target_id),
                        )
                    {
                        continue;
                    }

                    if pos.distance(*target.position) > target_radius + bullet_radius {
                        continue;
                    }

                    hits.push((bullet_index, target_id, *damage, *owner));

                    break;
                }
            }

            hits
        };

        let mut killed: Vec<(EntityId, Option<EntityId>)> = Vec::new();
        let spent = &mut self.buf_spent_flags;

        {
            let arena = self.scripting.arena();
            let entities = self.scripting.entities_mut();

            spent.clear();
            spent.resize(entities.bullets.len(), false);

            if !arena.walls().is_empty() {
                for (i, pos, _, radius, _) in entities.bullets.iter_indexed() {
                    if arena.circle_hits_wall(*pos, radius) {
                        spent[i] = true;
                    }
                }
            }

            let bases: Vec<_> = arena.zones().iter().filter(|z| z.team != NO_TEAM).collect();

            if !bases.is_empty() {
                for (i, pos, _, _, _) in entities.bullets.iter_indexed() {
                    let team = entities.bullets.team_at(i);
                    if bases.iter().any(|z| z.team != team && z.contains(*pos)) {
                        spent[i] = true;
                    }
                }
            }
        }

        if !entity_hits.is_empty() {
            let mut entities = self.scripting.entities_mut();

            for (bullet_index, target_id, damage, owner) in &entity_hits {
                entities.bullets.record_hit(*bullet_index, *target_id);

                // bots remember who shot them (players & other bots)
                if let Some(&t_conn) = entity_to_conn.get(target_id) {
                    if is_bot_id(t_conn, &self.config) {
                        if let Some(&a_conn) = entity_to_conn.get(owner) {
                            if let Some(brain) = self.bots.get_mut(&t_conn) {
                                brain.aggro_on = Some(a_conn);
                                brain.aggro_tick = self.tick_count;
                                brain.force_macro = true;
                            }
                        }
                    }
                }

                let cost = entities.get(*target_id).map(|e| *e.health).unwrap_or(0) as f32;

                if entities.bullets.damage(*bullet_index, cost) {
                    spent[*bullet_index] = true;
                }

                log_damage(
                    &mut self.damage_log,
                    &entities,
                    *target_id,
                    *owner,
                    *damage,
                    self.tick_count,
                );

                let died = {
                    let Some(health) = entities.health_mut(*target_id) else {
                        continue;
                    };

                    let was_alive = *health > 0;
                    *health = health.saturating_sub(*damage);
                    was_alive && *health == 0
                };

                if died {
                    killed.push((*target_id, Some(*owner)));
                }
            }
        }

        let bb_pairs: Vec<(usize, usize)> = {
            let entities = self.scripting.entities_mut();
            let mut pairs = Vec::new();

            for (i, pos, _damage, radius, _owner) in entities.bullets.iter_indexed() {
                if spent[i] {
                    continue;
                }

                self.bullet_hash
                    .get_nearby_into(&mut nearby, pos.x, pos.y, radius + 12.0);

                for &candidate in nearby.iter() {
                    let HashEntity::Bullet(j) = candidate else {
                        continue;
                    };

                    if j <= i || spent[j] {
                        continue;
                    }

                    if entities.bullets.owner(i) == entities.bullets.owner(j)
                        || teams_allied(entities.bullets.team_at(i), entities.bullets.team_at(j))
                    {
                        continue;
                    }

                    if pos.distance(entities.bullets.position(j))
                        > radius + entities.bullets.radius_at(j)
                    {
                        continue;
                    }

                    pairs.push((i, j));
                }
            }

            pairs
        };

        if !bb_pairs.is_empty() {
            let mut entities = self.scripting.entities_mut();

            for (i, j) in bb_pairs {
                if spent[i] || spent[j] {
                    continue;
                }

                let hp_i = entities.bullets.health(i);
                let hp_j = entities.bullets.health(j);
                let died_i = entities.bullets.damage(i, hp_j);
                let died_j = entities.bullets.damage(j, hp_i);

                if died_i {
                    spent[i] = true;
                }

                if died_j {
                    spent[j] = true;
                }
            }
        }

        {
            let mut entities = self.scripting.entities_mut();

            for i in (0..entities.bullets.len()).rev() {
                if spent[i] {
                    entities.bullets.remove(i);
                }
            }
        }

        self.finish_kills(killed, entity_to_conn);
    }

    fn tick_base_drones(&mut self) {
        let cfg = &self.config.base;
        if cfg.drone_count == 0 {
            return;
        }

        let bases: Vec<_> = self
            .scripting
            .arena()
            .zones()
            .iter()
            .filter(|z| z.team != NO_TEAM && z.spawn)
            .copied()
            .collect();

        let mut entities = self.scripting.entities_mut();

        for zone in bases {
            let have = entities.drones.count_in_zone(zone.id);
            if have >= cfg.drone_count {
                continue;
            }

            let due = self.base_drone_respawn.entry(zone.id).or_insert(0);
            if have > 0 && self.tick_count < *due {
                continue;
            }

            *due = self.tick_count + (cfg.respawn_seconds * TICK_RATE as f32) as u64;

            let center = (zone.min + zone.max) * 0.5;
            let missing = if have == 0 { cfg.drone_count } else { 1 };

            for _ in 0..missing {
                entities.drones.spawn(DroneSpawn {
                    position: center,
                    heading: self.rnd.generate::<f32>() * std::f32::consts::TAU,
                    speed: 0.0,
                    max_speed: cfg.drone_speed,
                    damage: cfg.drone_damage,
                    owner: BASE_OWNER,
                    team: zone.team,
                    barrel: 0,
                    radius: cfg.drone_radius,
                    health: cfg.drone_health,
                    recoil: 0,
                    lifetime: None,
                    zone: Some(zone.id),
                });
            }
        }
    }

    fn tick_shape_spawns(&mut self) {
        if self.players.is_empty() {
            return;
        }

        let arena = self.scripting.arena();
        let max_shapes = arena.max_shapes.unwrap_or(self.config.world.max_shapes);
        let bound = Vec2::splat(self.config.world.map_bound);
        let open = |p: Vec2| {
            !(p.x.abs() < 200.0 && p.y.abs() < 200.0)
                && arena.shapes_allowed(p)
                && !arena.circle_hits_wall(p, self.config.world.shape_clearance)
        };

        let mut entities = self.scripting.entities_mut();
        let mut failures = 0;

        while entities.shapes.len() < max_shapes && failures < 32 {
            let center = arena.random_open_point(
                &mut self.rnd,
                -bound,
                bound,
                self.config.world.shape_clearance,
                open,
            );
            if !open(center) {
                failures += 1;
                continue;
            }

            let (kind, rot_speed, xp) = random_shape_kind(self.rnd.generate::<f32>());
            let health = shape_max_health(kind);
            let orbit_radius = self.rnd.generate::<f32>() * 50.0 + 20.0;
            let orbit_angle = self.rnd.generate::<f32>() * std::f32::consts::TAU;
            let orbit_dir = if self.rnd.generate::<bool>() {
                1.0
            } else {
                -1.0
            };
            let orbit_speed = (self.rnd.generate::<f32>() * 0.4 + 0.1) * orbit_dir;

            entities.spawn_shape(
                center,
                kind,
                health,
                rot_speed,
                xp,
                orbit_radius,
                orbit_angle,
                orbit_speed,
            );
        }
    }

    fn fire_auto_weapons(
        &mut self,
        dt: f32,
        ids: &[EntityId],
        entity_to_conn: &HashMap<EntityId, u32>,
    ) {
        let mut entities = self.scripting.entities_mut();
        let Entities {
            tanks,
            bullets,
            drones,
            positions,
            alive,
            recoil_velocities,
            ..
        } = &mut *entities;

        for &id in ids {
            if !alive.get(id.index).copied().unwrap_or(false) {
                continue;
            }
            let position = *positions.get(id.index).unwrap_or(&Vec2::ZERO);

            let Some(t) = tanks.get_mut(id) else {
                continue;
            };

            if !t.tank_type.flags.can_shoot {
                continue;
            }

            let auto_fire = *t.auto_fire;
            let team = *t.team;
            let aim = *t.aim;
            let level = *t.level;
            let tank_scale = level_scale(level);
            let stats = *t.stat_levels;
            let reload_time =
                *t.reload_time / stat_mult(&stats, 6, self.config.player.stat_reload_per);
            let damage_mult = stat_mult(&stats, 5, self.config.player.stat_bullet_dmg_per);
            let speed_mult = stat_mult(&stats, 3, self.config.player.stat_bullet_speed_per);
            let hp_mult = stat_mult(&stats, 4, self.config.player.stat_bullet_hp_per);

            let base_damage = self.config.bullet.base_health
                * (1.0 + self.config.bullet.dmg_per_level * (level - 1) as f32);
            let base_speed = self.config.player.speed * self.config.bullet.speed_mult;

            let owner_conn = entity_to_conn.get(&id).copied().unwrap_or(u32::MAX);
            let strength = if is_bot_id(owner_conn, &self.config) {
                self.config.bots.bullet_strength
            } else {
                1.0
            };

            for (i, barrel) in t.tank_type.barrels.iter().enumerate() {
                let drone_cap = drone_cap(barrel);
                let needs_trigger = drone_cap.is_none_or(|cap| cap == u32::MAX);
                let cycle = (reload_time * barrel.reload).max(0.05);
                let phase = (cycle * barrel.delay).max(0.0);

                while t.barrel_timers.len() <= i {
                    t.barrel_timers.push(phase);
                }

                if needs_trigger && !auto_fire {
                    let timer = &mut t.barrel_timers[i];
                    *timer -= dt;

                    if *timer < phase {
                        *timer = phase;
                    }

                    continue;
                }

                let timer = &mut t.barrel_timers[i];
                *timer -= dt;

                if *timer > 0.0 {
                    continue;
                }

                if let Some(cap) = drone_cap {
                    if drones.count_for(id, i) >= cap as usize {
                        *timer = 0.0;
                        continue;
                    }
                }

                *timer += cycle;

                let barrel_angle = barrel.angle.to_radians();
                let muzzle_local = (Vec2::new(barrel.x, barrel.y)
                    + Vec2::from_angle(barrel_angle) * barrel.length)
                    * tank_scale;
                let world_angle = aim + barrel_angle;
                let muzzle_world = position + Vec2::from_angle(aim).rotate(muzzle_local);

                let radius = barrel.width * 0.5 * barrel.bullet.size_ratio * tank_scale;

                let is_trap = barrel.bullet.bullet_type == "trap";
                let damage = ((base_damage * barrel.bullet.damage * damage_mult * strength).round()
                    as u32)
                    .max(1);
                let speed = base_speed
                    * barrel.bullet.speed
                    * speed_mult
                    * if is_trap {
                        self.config.trap.speed_mult
                    } else {
                        1.0
                    };
                let lifetime = barrel.bullet.life_length
                    * if is_trap {
                        self.config.trap.lifetime
                    } else {
                        self.config.bullet.lifetime
                    };
                let health =
                    self.config.bullet.base_health * barrel.bullet.health * hp_mult * strength;

                let spawn_pos = muzzle_world - Vec2::from_angle(world_angle) * radius;

                if let Some(cap) = drone_cap {
                    let dcfg = &self.config.drone;
                    let max_speed = self.config.player.speed
                        * dcfg.speed_mult
                        * (1.0 + dcfg.speed_per_stat * stats[3] as f32)
                        * barrel.bullet.speed;

                    drones.spawn(DroneSpawn {
                        position: muzzle_world,
                        heading: world_angle,
                        speed: max_speed * dcfg.spawn_speed_frac,
                        max_speed,
                        damage,
                        owner: id,
                        team,
                        barrel: i,
                        radius,
                        health,
                        recoil: recoil_info(owner_conn, i, &self.config),
                        lifetime: (cap == u32::MAX)
                            .then_some(dcfg.uncapped_lifetime * barrel.bullet.life_length),
                        zone: None,
                    });

                    continue;
                }

                let recoil_info = recoil_info(owner_conn, i, &self.config);
                let scatter = (self.rnd.generate::<f32>() - 0.5)
                    * barrel.bullet.scatter_rate
                    * self.config.bullet.scatter_degrees.to_radians();

                bullets.spawn(
                    spawn_pos,
                    Vec2::from_angle(world_angle + scatter) * speed,
                    damage,
                    lifetime,
                    id,
                    radius,
                    health,
                    recoil_info,
                    team,
                    is_trap,
                );

                recoil_velocities[id.index] -= Vec2::from_angle(world_angle)
                    * (barrel.recoil * self.config.bullet.recoil_impulse);
                let recoil = recoil_velocities[id.index];
                if recoil.length_squared() > self.config.bullet.recoil_max.powi(2) {
                    recoil_velocities[id.index] =
                        recoil.normalize_or_zero() * self.config.bullet.recoil_max;
                }
            }
        }
    }
}

const BASE_OWNER: EntityId = EntityId {
    index: usize::MAX,
    generation: u32::MAX,
};

fn ease_in_out_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) * 0.5
    }
}

fn turret_layout<'a>(def: &Tank, config: &'a Config) -> Option<&'a TurretLayout> {
    config
        .turret
        .layouts
        .iter()
        .find(|l| l.addon != 0 && (l.addon == def.post_addon || l.addon == def.pre_addon))
}

pub(crate) fn turret_mounts(def: &Tank, config: &Config) -> Vec<TurretDef> {
    turret_layout(def, config)
        .map(|layout| {
            let count = layout.count as usize;
            (0..count)
                .map(|i| TurretDef {
                    angle: mount_angle(i, count).to_degrees(),
                    distance: layout.distance,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mount_angle(i: usize, count: usize) -> f32 {
    i as f32 * std::f32::consts::TAU / count.max(1) as f32
}

fn angle_diff(a: f32, b: f32) -> f32 {
    (a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn turn_toward(current: f32, target: f32, max_step: f32) -> f32 {
    let diff = angle_diff(target, current);
    current + diff.clamp(-max_step, max_step)
}

fn drone_cap(barrel: &crate::fs::tank_defs::Barrel) -> Option<u32> {
    if !barrel.is_trapezoid || barrel.bullet.bullet_type != "drone" {
        return None;
    }

    barrel.drone_count.filter(|&count| count > 0)
}

fn recoil_info(owner_conn: u32, barrel: usize, config: &Config) -> u32 {
    if owner_conn == u32::MAX {
        0
    } else if is_bot_id(owner_conn, config) {
        0x8000_0000 | (((owner_conn - config.bots.id_base) & 0x7f_ffff) << 8) | (barrel as u32 + 1)
    } else {
        (owner_conn << 8) | (barrel as u32 + 1)
    }
}

fn default_barrels() -> Vec<BarrelDef> {
    vec![BarrelDef {
        x: 0.0,
        y: 0.0,
        angle: 0.0,
        width: 18.0,
        length: 40.0,
        is_trapezoid: false,
        trap: false,
    }]
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn find_def(tree: &TankTree, id: u32) -> Option<(u32, &Tank)> {
    tree.iter().enumerate().find_map(|(tier, row)| {
        row.iter()
            .find(|tank| tank.id == id)
            .map(|tank| (tier as u32, tank))
    })
}

pub(crate) fn stat_mult(levels: &[u8; 8], idx: usize, per: f32) -> f32 {
    1.0 + per * levels[idx] as f32
}

fn stat_points_for_level(level: u32) -> u32 {
    if (2..=28).contains(&level) {
        1
    } else if (30..=45).contains(&level) && level % 3 == 0 {
        1
    } else {
        0
    }
}

fn shape_net_id(index: usize, generation: u32) -> u32 {
    0x80000000 | ((generation & 0xfff) << 19) | ((index as u32) & 0x7ffff)
}

fn shape_max_health(kind: ShapeKind) -> u32 {
    match kind {
        ShapeKind::Square => 10,
        ShapeKind::Triangle => 30,
        ShapeKind::Pentagon => 100,
    }
}

fn shape_radius(kind: ShapeKind) -> f32 {
    match kind {
        ShapeKind::Square => 16.0,
        ShapeKind::Triangle => 16.0,
        ShapeKind::Pentagon => 26.0,
    }
}

fn shape_kind_id(kind: ShapeKind) -> u32 {
    match kind {
        ShapeKind::Square => 1,
        ShapeKind::Triangle => 2,
        ShapeKind::Pentagon => 3,
    }
}

fn random_shape_kind(roll: f32) -> (ShapeKind, f32, u32) {
    if roll < 0.75 {
        (ShapeKind::Square, 0.35, 10)
    } else if roll < 0.95 {
        (ShapeKind::Triangle, 0.5, 25)
    } else {
        (ShapeKind::Pentagon, 0.22, 130)
    }
}

fn forget_entity(interest: &mut HashMap<u32, HashMap<u32, EntityType>>, net_id: u32) {
    for visible in interest.values_mut() {
        visible.remove(&net_id);
    }
}

#[allow(dead_code)]
fn entity_max_health(entities: &Entities, id: EntityId) -> u32 {
    entities
        .tanks
        .get(id)
        .map(|t| *t.max_health)
        .or_else(|| entities.shapes.get(id).map(|s| shape_max_health(*s.kind)))
        .unwrap_or(0)
}

fn entity_collision_radius(
    entities: &Entities,
    id: EntityId,
    config: &Config,
) -> Option<(f32, bool)> {
    if let Some(tank) = entities.tanks.get(id) {
        Some((config.player.radius * level_scale(*tank.level), true))
    } else if let Some(shape) = entities.shapes.get(id) {
        Some((shape_radius(*shape.kind), false))
    } else {
        None
    }
}

fn entity_contact_dps(entities: &Entities, id: EntityId, config: &Config) -> f32 {
    if let Some(t) = entities.tanks.get(id) {
        config.player.body_dps * stat_mult(t.stat_levels, 2, config.player.stat_body_dmg_per)
    } else if let Some(s) = entities.shapes.get(id) {
        match *s.kind {
            ShapeKind::Square => config.player.square_contact_dps,
            ShapeKind::Triangle => config.player.triangle_contact_dps,
            ShapeKind::Pentagon => config.player.pentagon_contact_dps,
        }
    } else {
        0.0
    }
}

fn grant_xp(
    entities: &mut Entities,
    target: EntityId,
    amount: u32,
    config: &Config,
) -> Option<u32> {
    let t = entities.tanks.get_mut(target)?;

    t.xp.0 = t.xp.0.saturating_add(amount);
    *t.score = t.score.saturating_add(amount);

    let start_level = *t.level;
    while *t.level < config.world.max_level && t.xp.0 >= t.xp.1 {
        t.xp.0 -= t.xp.1;
        *t.level += 1;
        // t.xp.1 = 250 + (*t.level - 1).pow(2) * 30;
        t.xp.1 = 1;
        *t.stat_points += stat_points_for_level(*t.level);
    }

    let level = *t.level;
    if level == start_level {
        return None;
    }

    refresh_max_health(entities, target, config);

    Some(level)
}

pub(crate) fn tank_max_health(def: &Tank, level: u32, health_stat: u8, config: &Config) -> u32 {
    (def.max_health as f32 * config.player.health_mult
        + config.player.health_per_level * level.saturating_sub(1) as f32
        + config.player.stat_max_health_bonus as f32 * health_stat as f32)
        .round()
        .max(1.0) as u32
}

pub(crate) fn refresh_max_health(entities: &mut Entities, id: EntityId, config: &Config) {
    let Some(t) = entities.tanks.get_mut(id) else {
        return;
    };

    let old_max = *t.max_health;
    let new_max = tank_max_health(t.tank_type, *t.level, t.stat_levels[1], config);
    *t.max_health = new_max;

    if let Some(h) = entities.health_mut(id) {
        if new_max > old_max {
            *h = h.saturating_add(new_max - old_max);
        }
        *h = (*h).min(new_max);
    }
}

fn total_stat_points(level: u32) -> u32 {
    (2..=level).map(stat_points_for_level).sum()
}

fn teams_allied(a: u8, b: u8) -> bool {
    a != NO_TEAM && a == b
}

fn death_packet(
    entities: &Entities,
    victim: EntityId,
    killer: Option<EntityId>,
    time_alive: u32,
) -> DeathPacket {
    let (score, level, kills, tank) = entities
        .tanks
        .get(victim)
        .map(|t| (*t.score, *t.level, *t.kills, t.tank_type.name.clone()))
        .unwrap_or_default();

    let (name, kind, killer_tank, team, sides, barrels) =
        if let Some(t) = killer.and_then(|k| entities.tanks.get(k)) {
            let name = if t.name.trim().is_empty() {
                "Unnamed".to_string()
            } else {
                t.name.to_string()
            };

            (
                name,
                KILLER_TANK,
                t.tank_type.name.clone(),
                *t.team,
                t.tank_type.sides,
                t.barrels.clone(),
            )
        } else if killer == Some(BASE_OWNER) {
            (
                "a Base Drone".to_string(),
                KILLER_ARENA,
                String::new(),
                NO_TEAM,
                0,
                Vec::new(),
            )
        } else if let Some(shape) = killer.and_then(|k| entities.shapes.get(k)) {
            let (name, kind, sides) = match *shape.kind {
                ShapeKind::Square => ("a Square", KILLER_SQUARE, 4),
                ShapeKind::Triangle => ("a Triangle", KILLER_TRIANGLE, 3),
                ShapeKind::Pentagon => ("a Pentagon", KILLER_PENTAGON, 5),
            };

            (
                name.to_string(),
                kind,
                String::new(),
                NO_TEAM,
                sides,
                Vec::new(),
            )
        } else {
            (
                "the Arena".to_string(),
                KILLER_ARENA,
                String::new(),
                NO_TEAM,
                0,
                Vec::new(),
            )
        };

    DeathPacket::new(
        name,
        kind,
        killer_tank,
        team,
        sides,
        barrels,
        score,
        level,
        kills,
        time_alive,
        tank,
        PACKET_SEED as u64,
    )
}

fn log_damage(
    log: &mut DamageLog,
    entities: &Entities,
    victim: EntityId,
    attacker: EntityId,
    amount: u32,
    tick: u64,
) {
    if victim == attacker
        || entities.tanks.get(victim).is_none()
        || entities.tanks.get(attacker).is_none()
    {
        return;
    }

    let dealt = entities
        .get(victim)
        .map(|e| amount.min(*e.health))
        .unwrap_or(0) as f32;

    if dealt <= 0.0 {
        return;
    }

    let entries = log.entry(victim).or_default();
    match entries.iter_mut().find(|e| e.0 == attacker) {
        Some(entry) => {
            entry.1 += dealt;
            entry.2 = tick;
        }
        None => entries.push((attacker, dealt, tick)),
    }
}

fn split_reward(
    reward: f32,
    assists: &[(EntityId, f32, u64)],
    killer: Option<EntityId>,
    entities: &Entities,
    tick: u64,
    window: u64,
) -> Vec<(EntityId, u32)> {
    let shares: Vec<(EntityId, f32)> = assists
        .iter()
        .filter(|(attacker, dealt, at)| {
            *dealt > 0.0
                && tick.saturating_sub(*at) <= window
                && entities.tanks.get(*attacker).is_some()
        })
        .map(|(attacker, dealt, _)| (*attacker, *dealt))
        .collect();

    if shares.is_empty() {
        return killer
            .filter(|k| entities.tanks.get(*k).is_some())
            .map(|k| vec![(k, reward.round() as u32)])
            .unwrap_or_default();
    }

    let total: f32 = shares.iter().map(|(_, dealt)| dealt).sum();

    shares
        .into_iter()
        .map(|(attacker, dealt)| (attacker, (reward * dealt / total).round() as u32))
        .collect()
}

fn system_message(text: String) -> ChatMessagePacket {
    ChatMessagePacket::new(
        NO_SENDER,
        0,
        0,
        now_ms() / 1000,
        String::new(),
        text,
        PACKET_SEED as u64,
    )
}

fn shape_kind_name(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Square => "square",
        ShapeKind::Triangle => "triangle",
        ShapeKind::Pentagon => "pentagon",
    }
}

fn step_tank_velocity(
    entities: &mut Entities,
    index: usize,
    move_dir: Option<f32>,
    max_speed: f32,
    dt: f32,
    config: &Config,
) {
    let desired = match move_dir {
        Some(dir) => Vec2::new(dir.cos(), dir.sin()) * max_speed,
        None => Vec2::ZERO,
    };

    let vel = entities.velocities[index];
    let delta = desired - vel;
    let max_step = config.player.acceleration * dt;

    let new_vel = if delta.length_squared() > max_step * max_step {
        vel + delta.normalize_or_zero() * max_step
    } else {
        desired
    };

    entities.velocities[index] = new_vel;
    entities.positions[index] += (new_vel + entities.recoil_velocities[index]) * dt;
    entities.recoil_velocities[index] /= 1.0 + config.bullet.recoil_decay * dt;
}

fn clamp_tank_position(entities: &mut Entities, index: usize, config: &Config) {
    let pos = &mut entities.positions[index];

    pos.x = pos.x.clamp(-config.world.map_bound, config.world.map_bound);
    pos.y = pos.y.clamp(-config.world.map_bound, config.world.map_bound);
}

fn broadcast_despawn(
    connections: &Connections,
    entities: &mut Entities,
    entity_to_conn: &HashMap<EntityId, u32>,
    target: EntityId,
) {
    let (net_id, entity_type) = if let Some(&conn_id) = entity_to_conn.get(&target) {
        (conn_id, EntityType::Player)
    } else if entities.shapes.get(target).is_some() {
        (
            shape_net_id(target.index, target.generation),
            EntityType::Shape,
        )
    } else {
        return;
    };

    entities.despawn(target);
    connections.broadcast(RemoveEntityPacket::new(
        net_id,
        entity_type,
        PACKET_SEED as u64,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(plugins: &[&str]) -> GameState {
        state_with_config(plugins, |_| {})
    }

    fn state_with_config(plugins: &[&str], tweak: impl FnOnce(&mut Config)) -> GameState {
        let mut config: Config = toml::from_str(include_str!("../fs/data/config.toml")).unwrap();
        config.game.plugins = plugins.iter().map(|p| p.to_string()).collect();
        tweak(&mut config);

        let (_send, recv) = tokio::sync::mpsc::unbounded_channel();
        GameState::new(recv, Connections::new(), Arc::new(config)).unwrap()
    }

    fn run_ticks(state: &mut GameState, ticks: u32) {
        let dt = 0.1;
        for _ in 0..ticks {
            state.tick_count += 1;
            state.drain_events();
            state.tick_bot_spawns();
            state.tick_shape_orbits(dt);
            state.regen_health();
            state.apply_base_zones(dt);
            state.rebuild_spatial_hash();
            if state.tick_count % NAV_REBUILD_INTERVAL == 0 {
                rebuild_nav_grid(
                    &mut state.scripting,
                    &mut state.nav_danger,
                    &mut state.nav_version,
                    &state.config,
                );
            }
            state.tick_bot_ai();
            state.resolve_entity_collisions();
            state.clamp_shape_positions();
            state.broadcast_entity_updates(dt);
            state.broadcast_arena();
            state.simulate_combat(dt);
            state.sync_lua();
            state.scripting.tick();
            state.flush_lua_events();
            state.apply_lua_actions();
            state.tick_shape_spawns();
        }
    }

    fn player_pos(state: &GameState, id: u32) -> Vec2 {
        let eid = state.players[&id];
        state.scripting.entities_mut().positions[eid.index]
    }

    #[test]
    fn maze_plugin_builds_walls_and_keeps_tanks_out() {
        let mut state = state_with(&["maze"]);

        assert_eq!(state.config.world.map_bound, 3200.0);
        assert!(state.scripting.arena().walls().len() > 20);

        state.handle_game_events(&GameEvents::PlayerSpawn {
            id: 1,
            name: "tester".into(),
        });

        run_ticks(&mut state, 60);

        let arena = state.scripting.arena();
        let entities = state.scripting.entities_mut();
        for &eid in state.players.values() {
            if entities.is_alive(eid) {
                let pos = entities.positions[eid.index];
                assert!(
                    !arena.circle_hits_wall(pos, state.config.player.radius * 0.5),
                    "tank stuck in a wall at {pos}"
                );
            }
        }
    }

    #[test]
    fn four_tdm_plugin_sets_up_teams_and_bases() {
        let mut state = state_with(&["4tdm"]);

        assert_eq!(state.scripting.arena().team_count, 4);
        assert_eq!(state.scripting.arena().zones().len(), 4);

        for id in 1..=4 {
            state.handle_game_events(&GameEvents::PlayerSpawn {
                id,
                name: format!("p{id}"),
            });
        }

        let teams: HashSet<u8> = (1..=4)
            .map(|id| {
                let eid = state.players[&id];
                state.scripting.entities_mut().tanks.team_of(eid)
            })
            .collect();
        assert_eq!(teams.len(), 4);

        for id in 1..=4 {
            let pos = player_pos(&state, id);
            let eid = state.players[&id];
            let team = state.scripting.entities_mut().tanks.team_of(eid);
            let arena = state.scripting.arena();
            let zone = arena.zone_at(pos).expect("spawned outside every base");
            assert_eq!(zone.team, team);
        }

        run_ticks(&mut state, 40);
    }

    #[test]
    fn lua_api_drives_players() {
        let mut state = state_with(&[]);

        state.handle_game_events(&GameEvents::PlayerSpawn {
            id: 7,
            name: "lua".into(),
        });
        state.sync_lua();

        state
            .scripting
            .lua
            .load(
                r#"
                local p = players.get(7)
                assert(p and p.name == "lua")
                p:teleport(123, -45)
                p.level = 15
                p:setTank("twin")
                p:message("hi")
                commands.register("ping", function(player, args)
                    player:addXp(5)
                    chat.send(player.id, "pong " .. (args[1] or ""))
                end)
            "#,
            )
            .exec()
            .unwrap();

        state.apply_lua_actions();

        let pos = player_pos(&state, 7);
        assert!((pos.x - 123.0).abs() < 1e-3 && (pos.y + 45.0).abs() < 1e-3);

        {
            let eid = state.players[&7];
            let entities = state.scripting.entities_mut();
            let tank = entities.tanks.get(eid).unwrap();
            assert_eq!(*tank.level, 15);
            assert_eq!(tank.tank_type.name, "Twin");
            assert_eq!(*tank.stat_points, total_stat_points(15));
            assert!(*tank.max_health > 100);
        }

        assert!(state.scripting.run_command(7, "/ping hello").unwrap());
        assert!(!state.scripting.run_command(7, "/nope").unwrap());
        state.apply_lua_actions();
    }

    #[test]
    fn idle_drones_hunt_nearby_shapes() {
        let mut state = state_with_config(&[], |c| {
            c.bots.count = 0;
            c.world.max_shapes = 0;
        });

        state.handle_game_events(&GameEvents::PlayerSpawn {
            id: 3,
            name: "overseer".into(),
        });
        state.sync_lua();
        state
            .scripting
            .lua
            .load("players.get(3):setTank('overseer')")
            .exec()
            .unwrap();
        state.apply_lua_actions();

        let shape = {
            let pos = player_pos(&state, 3);
            let mut entities = state.scripting.entities_mut();
            entities.spawn_shape(
                pos + Vec2::new(200.0, 0.0),
                ShapeKind::Pentagon,
                10_000,
                0.0,
                10,
                0.0,
                0.0,
                0.0,
            )
        };

        run_ticks(&mut state, 80);

        let entities = state.scripting.entities_mut();
        assert!(entities.drones.len() > 0, "overseer never launched drones");
        assert!(
            entities.get(shape).map(|s| *s.health).unwrap_or(0) < 10_000,
            "idle drones ignored a pentagon right next to their owner"
        );
    }

    #[test]
    fn bot_finds_its_way_through_the_maze() {
        let mut state = state_with_config(&["maze"], |c| c.bots.count = 1);
        state.scripting.arena().max_shapes = Some(0);

        run_ticks(&mut state, 1);

        let bot_id = state.config.bots.id_base;
        let start = player_pos(&state, bot_id);

        let goal = {
            let arena = state.scripting.arena();
            let bound = Vec2::splat(arena.bound - 200.0);
            let mut goal = start;
            while goal.distance(start) < 2500.0 {
                goal = arena.random_open_point(&mut state.rnd, -bound, bound, 60.0, |_| true);
            }
            goal
        };

        assert!(state.scripting.arena().segment_hits_wall(start, goal, 0.0));

        let mut closest = f32::MAX;
        for _ in 0..1500 {
            if let Some(brain) = state.bots.get_mut(&bot_id) {
                brain.target = None;
                brain.wander_target = goal;
                brain.next_wander_tick = u64::MAX;
            }

            run_ticks(&mut state, 1);

            if !state.players.contains_key(&bot_id) {
                break;
            }

            closest = closest.min(player_pos(&state, bot_id).distance(goal));
            if closest < 150.0 {
                break;
            }
        }

        assert!(
            closest < 150.0,
            "bot got stuck {closest} units away from a goal {} units away",
            start.distance(goal)
        );
    }

    #[test]
    #[ignore]
    fn maze_tick_budget() {
        let mut state = state_with(&["maze"]);

        run_ticks(&mut state, 5);

        let started = std::time::Instant::now();
        let mut worst = std::time::Duration::ZERO;
        for _ in 0..200 {
            let t = std::time::Instant::now();
            run_ticks(&mut state, 1);
            worst = worst.max(t.elapsed());
        }

        let avg = started.elapsed() / 200;
        println!(
            "maze: avg tick {avg:?}, worst {worst:?}, {} shapes",
            state.scripting.entities_mut().shapes.len()
        );
        assert!(avg.as_millis() < 50, "average tick {avg:?} is too slow");
    }

    #[test]
    fn killing_a_tank_pays_eighty_percent_of_its_score() {
        let mut state = state_with_config(&[], |c| c.bots.count = 1);
        run_ticks(&mut state, 1);

        state.handle_game_events(&GameEvents::PlayerSpawn {
            id: 1,
            name: "killer".into(),
        });

        let bot = state.players[&state.config.bots.id_base];
        let killer = state.players[&1];

        if let Some(t) = state.scripting.entities_mut().tanks.get_mut(bot) {
            *t.score = 1000;
        }

        let entity_to_conn = state.entity_to_conn();
        state.finish_kills(vec![(bot, Some(killer))], &entity_to_conn);

        let entities = state.scripting.entities_mut();
        assert_eq!(*entities.tanks.get(killer).unwrap().score, 800);
        assert!(!entities.is_alive(bot));
    }

    #[test]
    fn spectator_hotspot_finds_the_fight() {
        let mut state = state_with_config(&[], |c| {
            c.bots.count = 0;
            c.world.max_shapes = 0;
        });

        let fight = Vec2::new(1500.0, -1200.0);
        for id in 1..=4 {
            state.handle_game_events(&GameEvents::PlayerSpawn {
                id,
                name: format!("p{id}"),
            });
            let eid = state.players[&id];
            state.scripting.entities_mut().positions[eid.index] =
                fight + Vec2::new(id as f32 * 60.0, 0.0);
        }

        state.find_hotspot();

        assert!(
            state.hotspot.distance(fight) < 400.0,
            "hotspot {} is nowhere near the fight",
            state.hotspot
        );
    }

    fn spawn_at(state: &mut GameState, id: u32, pos: Vec2) -> EntityId {
        state.handle_game_events(&GameEvents::PlayerSpawn {
            id,
            name: format!("p{id}"),
        });
        let eid = state.players[&id];
        state.scripting.entities_mut().positions[eid.index] = pos;
        eid
    }

    fn set_tank(state: &mut GameState, id: u32, tank: &str) {
        state.sync_lua();
        state
            .scripting
            .lua
            .load(format!("players.get({id}):setTank('{tank}')"))
            .exec()
            .unwrap();
        state.apply_lua_actions();
    }

    fn quiet_state(plugins: &[&str]) -> GameState {
        state_with_config(plugins, |c| {
            c.bots.count = 0;
            c.world.max_shapes = 0;
        })
    }

    #[test]
    fn auto_turrets_prefer_tanks_then_the_best_shape() {
        let mut state = quiet_state(&[]);
        spawn_at(&mut state, 1, Vec2::ZERO);
        set_tank(&mut state, 1, "auto_gunner");

        let (square, pentagon) = {
            let mut entities = state.scripting.entities_mut();
            let square = entities.spawn_shape(
                Vec2::new(120.0, 0.0),
                ShapeKind::Square,
                10,
                0.0,
                10,
                0.0,
                0.0,
                0.0,
            );
            let pentagon = entities.spawn_shape(
                Vec2::new(0.0, 300.0),
                ShapeKind::Pentagon,
                100,
                0.0,
                130,
                0.0,
                0.0,
                0.0,
            );
            (square, pentagon)
        };
        let _ = square;

        state.rebuild_entity_hash();
        state.aim_turrets(0.1);

        let eid = state.players[&1];
        let pentagon_pos = state.scripting.entities_mut().positions[pentagon.index];
        assert_eq!(state.turrets[&eid].targets[0], Some(pentagon_pos));

        let enemy = spawn_at(&mut state, 2, Vec2::new(-250.0, 0.0));
        let enemy_pos = state.scripting.entities_mut().positions[enemy.index];

        state.rebuild_entity_hash();
        state.aim_turrets(0.1);
        assert_eq!(state.turrets[&eid].targets[0], Some(enemy_pos));

        let before = state.scripting.entities_mut().bullets.len();
        for _ in 0..30 {
            state.aim_turrets(0.1);
            let entity_to_conn = state.entity_to_conn();
            state.fire_turrets(0.1, &entity_to_conn);
        }
        assert!(state.scripting.entities_mut().bullets.len() > before);
    }

    #[test]
    fn base_drones_guard_bases_and_bullets_die_inside() {
        let mut state = quiet_state(&["4tdm"]);
        state.tick_base_drones();

        let zones = state.scripting.arena().zones().len();
        assert_eq!(
            state.scripting.entities_mut().drones.len(),
            zones * state.config.base.drone_count
        );

        let intruder = spawn_at(&mut state, 1, Vec2::ZERO);
        let (enemy_base, team) = {
            let arena = state.scripting.arena();
            let entities = state.scripting.entities_mut();
            let mine = entities.tanks.team_of(intruder);
            let z = arena.zones().iter().find(|z| z.team != mine).unwrap();
            ((z.min + z.max) * 0.5, mine)
        };

        state.scripting.entities_mut().bullets.spawn(
            enemy_base,
            Vec2::ZERO,
            5,
            5.0,
            intruder,
            5.0,
            10.0,
            0,
            team,
            false,
        );
        let entity_to_conn = state.entity_to_conn();
        state.rebuild_bullet_hash();
        state.resolve_bullet_collisions(&entity_to_conn);
        assert_eq!(state.scripting.entities_mut().bullets.len(), 0);

        state.scripting.entities_mut().positions[intruder.index] = enemy_base;
        state.config = Arc::new({
            let mut c = (*state.config).clone();
            c.world.base_push = 0.0;
            c
        });
        let mut alive = true;
        for _ in 0..80 {
            if let Some(h) = state.scripting.entities_mut().health_mut(intruder) {
                *h = (*h).max(1);
            }
            run_ticks(&mut state, 1);
            if !state.players.contains_key(&1) {
                alive = false;
                break;
            }
            state.scripting.entities_mut().positions[intruder.index] = enemy_base;
        }
        assert!(!alive, "intruder survived inside an enemy base");
    }

    #[test]
    fn assists_split_the_kill_reward_by_damage() {
        let mut state = quiet_state(&[]);
        let a = spawn_at(&mut state, 1, Vec2::new(-1000.0, 0.0));
        let b = spawn_at(&mut state, 2, Vec2::new(1000.0, 0.0));
        let victim = spawn_at(&mut state, 3, Vec2::new(0.0, 1000.0));

        if let Some(t) = state.scripting.entities_mut().tanks.get_mut(victim) {
            *t.score = 1000;
        }

        {
            let entities = state.scripting.entities_mut();
            log_damage(
                &mut state.damage_log,
                &entities,
                victim,
                a,
                30,
                state.tick_count,
            );
            log_damage(
                &mut state.damage_log,
                &entities,
                victim,
                b,
                70,
                state.tick_count,
            );
        }

        let entity_to_conn = state.entity_to_conn();
        state.finish_kills(vec![(victim, Some(a))], &entity_to_conn);

        let entities = state.scripting.entities_mut();
        assert_eq!(*entities.tanks.get(a).unwrap().score, 240);
        assert_eq!(*entities.tanks.get(b).unwrap().score, 560);
    }

    #[test]
    fn machine_gun_bullets_spread() {
        let mut state = quiet_state(&[]);
        let eid = spawn_at(&mut state, 1, Vec2::ZERO);
        set_tank(&mut state, 1, "machine_gun");

        if let Some(t) = state.scripting.entities_mut().tanks.get_mut(eid) {
            *t.auto_fire = true;
            *t.aim = 0.0;
        }

        let entity_to_conn = state.entity_to_conn();
        for _ in 0..200 {
            state.fire_auto_weapons(0.05, &[eid], &entity_to_conn);
        }

        let entities = state.scripting.entities_mut();
        let angles: Vec<f32> = (0..entities.bullets.len())
            .map(|i| {
                let v = entities.bullets.velocity_at(i);
                v.y.atan2(v.x).to_degrees()
            })
            .collect();

        let spread = angles.iter().cloned().fold(f32::MIN, f32::max)
            - angles.iter().cloned().fold(f32::MAX, f32::min);
        assert!(angles.len() > 10);
        assert!(spread > 10.0 && spread <= 30.0, "spread {spread}");
    }

    #[test]
    fn traps_survive_inside_their_own_base() {
        let mut state = quiet_state(&["4tdm"]);
        let eid = spawn_at(&mut state, 1, Vec2::ZERO);
        let home = {
            let arena = state.scripting.arena();
            let entities = state.scripting.entities_mut();
            let mine = entities.tanks.team_of(eid);
            let z = arena.zones().iter().find(|z| z.team == mine).unwrap();
            (z.min + z.max) * 0.5
        };
        state.scripting.entities_mut().positions[eid.index] = home;
        set_tank(&mut state, 1, "trapper");

        if let Some(t) = state.scripting.entities_mut().tanks.get_mut(eid) {
            *t.auto_fire = true;
        }

        run_ticks(&mut state, 30);

        let entities = state.scripting.entities_mut();
        let traps = entities.bullets.iter().filter(|b| b.5).count();
        assert!(traps > 0, "no traps alive inside the home base");
    }

    #[test]
    fn spectator_travel_eases_quadratically() {
        assert_eq!(ease_in_out_quad(0.0), 0.0);
        assert!((ease_in_out_quad(0.25) - 0.125).abs() < 1e-6);
        assert!((ease_in_out_quad(0.5) - 0.5).abs() < 1e-6);
        assert!((ease_in_out_quad(0.75) - 0.875).abs() < 1e-6);
        assert_eq!(ease_in_out_quad(1.0), 1.0);
    }

    #[test]
    fn wait_counts_seconds_not_ticks() {
        let mut state = state_with(&[]);

        state
            .scripting
            .lua
            .load(
                r#"
                done = false
                task.spawn(function()
                    wait(1)
                    done = true
                end)
            "#,
            )
            .exec()
            .unwrap();
        state.scripting.start_spawned();

        for _ in 0..9 {
            state.scripting.tick();
        }
        assert!(!state.scripting.lua.globals().get::<bool>("done").unwrap());

        state.scripting.tick();
        assert!(state.scripting.lua.globals().get::<bool>("done").unwrap());
    }
}
