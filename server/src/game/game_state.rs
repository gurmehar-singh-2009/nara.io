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
    PACKET_SEED, PlayerUpgradesPacket, TankOption, TankTreePacket,
    client_bound::{
        AddEntityPacket, BarrelDef, EntityType, LeaderboardPacket, PlayerStatsPacket,
        RemoveEntityPacket, UpdateEntityPacket, UpdateEntityPacketData,
    },
    level_scale,
    server_bound::ChatMessagePacket,
};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    anti_cheat::{PlayerWatch, SUSPICION_FLAG_THRESHOLD, multibox_anti, upgrade_anti},
    entities::{
        connections::Connections,
        entity::{Entities, EntityId},
        shape::ShapeKind,
        spatial_hash::{HashEntity, SpatialHash},
        tank::barrel_defs,
    },
    fs::{
        load_config::Config,
        tank_defs::{Tank, TankTree},
    },
    game::bots::{
        BotBrain, NAV_LEN, NAV_REBUILD_INTERVAL, is_bot_id, rebuild_nav_grid, spend_stat_points,
        tick_ai, tick_spawns,
    },
    scripting::scripting::Scripting,
};

#[derive(Debug)]
pub enum GameEvents {
    PlayerSpawn { id: u32, name: String },
    PlayerDisconnect { id: u32 },
    PlayerMovement { id: u32, dir: Option<f32> },
    PlayerAutoFire { id: u32, enabled: bool },
    PlayerAim { id: u32, dir: f32 },
    TankSelect { id: u32, tank_id: u32 },
    StatUpgrade { id: u32, stat: u8 },
    TankTree { tree: TankTree },
    ChatMessage { id: u32, channel: u8, text: String },
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
        let mut scripting = Scripting::new(Entities::new())?;

        let mut loaded = false;
        for dir in ["content", "../content", "server/content"] {
            if std::path::Path::new(dir).is_dir() {
                if let Err(err) = scripting.load_scripts(dir) {
                    error!("Lua script load error ({dir}): {err}");
                }

                loaded = true;

                break;
            }
        }

        if !loaded {
            error!("content/ not found!! maybe run from the repo root or adjust path?");
        }

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

            self.drain_events();
            self.tick_bot_spawns();
            self.reload_tanks();

            phase_us[0] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.tick_shape_orbits(dt);
            self.regen_health();
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
            self.broadcast_entity_updates(dt);
            self.broadcast_player_stats();
            self.send_tank_upgrade_offers();
            self.run_anti_cheat_pass();

            if self.tick_count % 10 == 0 {
                self.broadcast_leaderboard();
            }

            phase_us[4] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.simulate_combat(dt);
            phase_us[5] += t.elapsed().as_secs_f64() * 1.0e6;

            let t = std::time::Instant::now();
            self.scripting.scheduler.on_tick();
            self.tick_shape_spawns();
            phase_us[6] += t.elapsed().as_secs_f64() * 1.0e6;

            let elapsed = tick_start.elapsed();
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
        tick_spawns(
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
    }

    fn tick_bot_ai(&mut self) {
        if self.bots.is_empty() {
            return;
        }

        let upgrade_candidates = tick_ai(
            &mut self.bots,
            &self.players,
            &self.spatial_hash,
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
                spend_stat_points(
                    &mut self.scripting.entities_mut(),
                    &mut self.rnd,
                    eid,
                    0,
                    &self.config,
                );
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
            if let Err(err) = self.scripting.dispatch_event(&msg) {
                error!("Lua event error: {err}");
            }
        }
    }

    fn handle_game_events(&mut self, msg: &GameEvents) {
        match &msg {
            GameEvents::PlayerSpawn { id, name } => {
                self.spawn_player(*id, name);
                self.watch.entry(*id).or_default();
                self.teams.insert(*id, self.least_populated_team());
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

            GameEvents::PlayerAim { id, dir } => {
                self.with_live_entity(*id, |entities, eid| {
                    if let Some(tank) = entities.tanks.get_mut(eid) {
                        *tank.aim = *dir;
                    }
                });
                self.record_aim_sample(*id, *dir);
            }

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

        let mut leveled_max = None;
        {
            let mut entities = self.scripting.entities_mut();
            let Some(t) = entities.tanks.get_mut(entity_id) else {
                return;
            };

            if *t.stat_points == 0
                || t.stat_levels[stat as usize] >= self.config.player.stat_max_level
            {
                return;
            }

            t.stat_levels[stat as usize] += 1;
            *t.stat_points -= 1;

            if stat == 1 {
                *t.max_health += self.config.player.stat_max_health_bonus;
                leveled_max = Some(*t.max_health);
            }
        }

        if let Some(new_max) = leveled_max {
            let mut entities = self.scripting.entities_mut();

            if let Some(health) = entities.health_mut(entity_id) {
                *health = (*health + self.config.player.stat_max_health_bonus).min(new_max);
            }
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
        let censored = text.censor().to_string();

        let team = self.teams.get(&conn_id).unwrap_or(&0);
        let packet = ChatMessagePacket::new(
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
        let mut counts = [0usize; 2 as usize]; // UPDATE LATER, 2=NUM OF TEAMS

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

    fn broadcast_player_def(&mut self, conn_id: u32, entity_id: EntityId) {
        let (x, y, level, name, barrels) = {
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
            )
        };

        let my_packet = AddEntityPacket::new(
            conn_id,
            EntityType::Player,
            x,
            y,
            level,
            name.clone(),
            true,
            barrels.clone(),
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
                barrels.clone(),
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
        let world_size = self.config.world.map_bound;
        let x = (self.rnd.generate::<f32>() * world_size) - world_size / 2.0;
        let y = (self.rnd.generate::<f32>() * world_size) - world_size / 2.0;

        let entity_id = self.scripting.entities_mut().spawn_tank(
            Vec2::new(x, y),
            Vec2::ZERO,
            self.config.player.default_health,
            name.to_string(),
        );
        self.players.insert(id, entity_id);
        self.interest.insert(id, HashMap::new());

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
            barrels.clone(),
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
                    tank.barrels.clone(),
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
                        barrels.clone(),
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
        let mut entities = self.scripting.entities_mut();
        for center in entities.shapes.centers.iter_mut() {
            center.x = center
                .x
                .clamp(-self.config.world.map_bound, self.config.world.map_bound);
            center.y = center
                .y
                .clamp(-self.config.world.map_bound, self.config.world.map_bound);
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

        for (i, (_, pos, _, _)) in entities.bullets.iter().enumerate() {
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
        for (i, (_, pos, _, _)) in entities.bullets.iter().enumerate() {
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
            }
        }

        let mut viewers: Vec<(u32, Vec2)> = Vec::new();
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
                });
            }
        }

        let mut bullet_net_ids: Vec<u32> = Vec::with_capacity(entities.bullets.len());
        let mut bullet_update: Vec<usize> = Vec::with_capacity(entities.bullets.len());
        for (i, (net_id, pos, radius, recoil)) in entities.bullets.iter().enumerate() {
            bullet_net_ids.push(net_id);
            bullet_update.push(all_updates.len());

            all_updates.push(UpdateEntityPacketData {
                id: net_id,
                entity_type: EntityType::Bullet,
                x: pos.x,
                y: pos.y,
                rot: 0.0,
                scale: radius,
                kind: recoil,
                health: 1,
                max_health: 1,
            });
        }

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
                                        tank.barrels.clone(),
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
                Some((tank.name.to_string(), tank.xp.0))
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
        let entity_to_conn: HashMap<EntityId, u32> =
            self.players.iter().map(|(&k, &v)| (v, k)).collect();

        self.rebuild_entity_hash();

        for _ in 0..self.config.world.bullet_substeps {
            self.fire_auto_weapons(sub_dt, &ids, &entity_to_conn);
            self.scripting.entities_mut().bullets.tick(sub_dt);
            self.rebuild_bullet_hash();
            self.resolve_bullet_collisions(&entity_to_conn);
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
                    (true, true) => {
                        hits.push((id1, dmg2, Some(id2)));
                        hits.push((id2, dmg1, Some(id1)));
                    }
                    (true, false) => {
                        hits.push((id1, dmg2, None));
                        hits.push((id2, dmg1, Some(id1)));
                    }
                    (false, true) => {
                        hits.push((id1, dmg2, Some(id2)));
                        hits.push((id2, dmg1, None));
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

        let entity_to_conn: HashMap<EntityId, u32> =
            self.players.iter().map(|(&k, &v)| (v, k)).collect();
        let mut entities = self.scripting.entities_mut();

        for (target_id, damage, attacker) in hits {
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

            if !died {
                continue;
            }

            if let Some(attacker) = attacker {
                let xp_gained = if entities.tanks.get(target_id).is_some() {
                    entities.tanks.get(target_id).map(|t| t.xp.0 / 2)
                } else {
                    entities.shapes.get(target_id).map(|s| *s.xp_reward)
                };

                if let Some(xp) = xp_gained {
                    grant_xp(&mut entities, attacker, xp, &self.config);
                }
            }

            broadcast_despawn(&self.connections, &mut entities, &entity_to_conn, target_id);

            if let Some(&conn_id) = entity_to_conn.get(&target_id) {
                self.players.remove(&conn_id);
                forget_entity(&mut self.interest, conn_id);

                if is_bot_id(conn_id, &self.config) {
                    self.bots.remove(&conn_id);
                    self.bot_respawns
                        .push((conn_id, self.tick_count + self.config.bots.respawn_ticks));
                }
            } else {
                forget_entity(
                    &mut self.interest,
                    shape_net_id(target_id.index, target_id.generation),
                );
            }
        }
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

                    let Some((target_radius, _)) =
                        entity_collision_radius(&entities, target_id, &self.config)
                    else {
                        continue;
                    };

                    if pos.distance(*target.position) > target_radius + bullet_radius {
                        continue;
                    }

                    hits.push((bullet_index, target_id, *damage, *owner));

                    break;
                }
            }

            hits
        };

        let n_bullets = self.scripting.entities_mut().bullets.len();
        let spent = &mut self.buf_spent_flags;
        spent.clear();
        spent.resize(n_bullets, false);

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

                let died = {
                    let Some(health) = entities.health_mut(*target_id) else {
                        continue;
                    };

                    let was_alive = *health > 0;
                    *health = health.saturating_sub(*damage);
                    was_alive && *health == 0
                };

                if died {
                    let xp_gained = if let Some(tank) = entities.tanks.get(*target_id) {
                        Some(tank.xp.0 / 2)
                    } else {
                        entities.shapes.get(*target_id).map(|s| *s.xp_reward)
                    };

                    if let Some(xp_gained) = xp_gained {
                        grant_xp(&mut entities, *owner, xp_gained, &self.config);
                    }

                    broadcast_despawn(&self.connections, &mut entities, entity_to_conn, *target_id);

                    if let Some(&conn_id) = entity_to_conn.get(target_id) {
                        self.players.remove(&conn_id);
                        forget_entity(&mut self.interest, conn_id);

                        if is_bot_id(conn_id, &self.config) {
                            self.bots.remove(&conn_id);
                            self.bot_respawns
                                .push((conn_id, self.tick_count + self.config.bots.respawn_ticks));
                        }
                    } else {
                        forget_entity(
                            &mut self.interest,
                            shape_net_id(target_id.index, target_id.generation),
                        );
                    }
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

                    if entities.bullets.owner(i) == entities.bullets.owner(j) {
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
    }

    fn tick_shape_spawns(&mut self) {
        if self.players.is_empty() {
            return;
        }

        let mut entities = self.scripting.entities_mut();
        while entities.shapes.len() < self.config.world.max_shapes {
            let x = (self.rnd.generate::<f32>() * self.config.world.map_bound * 2.0)
                - self.config.world.map_bound;
            let y = (self.rnd.generate::<f32>() * self.config.world.map_bound * 2.0)
                - self.config.world.map_bound;
            if x.abs() < 200.0 && y.abs() < 200.0 {
                continue;
            }

            let (kind, rot_speed, xp) = random_shape_kind(self.rnd.generate::<f32>());
            let health = shape_max_health(kind);
            let center = Vec2::new(x, y);
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
                let cycle = (reload_time * barrel.reload).max(0.05);
                let phase = (cycle * barrel.delay).max(0.0);

                while t.barrel_timers.len() <= i {
                    t.barrel_timers.push(phase);
                }

                if !auto_fire {
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

                *timer += cycle;

                let barrel_angle = barrel.angle.to_radians();
                let muzzle_local = (Vec2::new(barrel.x, barrel.y)
                    + Vec2::from_angle(barrel_angle) * barrel.length)
                    * tank_scale;
                let world_angle = aim + barrel_angle;
                let muzzle_world = position + Vec2::from_angle(aim).rotate(muzzle_local);

                let radius = barrel.width * 0.5 * barrel.bullet.size_ratio * tank_scale;

                let damage = ((base_damage * barrel.bullet.damage * damage_mult * strength).round()
                    as u32)
                    .max(1);
                let speed = base_speed * barrel.bullet.speed * speed_mult;
                let lifetime = self.config.bullet.lifetime * barrel.bullet.life_length;
                let health =
                    self.config.bullet.base_health * barrel.bullet.health * hp_mult * strength;

                let spawn_pos = muzzle_world + Vec2::from_angle(world_angle) * radius;

                let recoil_info = if owner_conn == u32::MAX {
                    0
                } else if is_bot_id(owner_conn, &self.config) {
                    0x8000_0000
                        | (((owner_conn - self.config.bots.id_base) & 0x7f_ffff) << 8)
                        | (i as u32 + 1)
                } else {
                    (owner_conn << 8) | (i as u32 + 1)
                };

                bullets.spawn(
                    spawn_pos,
                    Vec2::from_angle(world_angle) * speed,
                    damage,
                    lifetime,
                    id,
                    radius,
                    health,
                    recoil_info,
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

fn default_barrels() -> Vec<BarrelDef> {
    vec![BarrelDef {
        x: 0.0,
        y: 0.0,
        angle: 0.0,
        width: 18.0,
        length: 40.0,
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

fn grant_xp(entities: &mut Entities, target: EntityId, amount: u32, config: &Config) {
    let Some(t) = entities.tanks.get_mut(target) else {
        return;
    };

    t.xp.0 = t.xp.0.saturating_add(amount);

    while *t.level < config.world.max_level && t.xp.0 >= t.xp.1 {
        t.xp.0 -= t.xp.1;
        *t.level += 1;
        // t.xp.1 = 250 + (*t.level - 1).pow(2) * 30;
        t.xp.1 = 1;
        *t.stat_points += stat_points_for_level(*t.level);
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
