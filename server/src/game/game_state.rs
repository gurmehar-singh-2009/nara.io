use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use glam::Vec2;
use nanorand::Rng;
use paris::{error, info};
use rustrict::CensorStr;
use shared::packets::{
    PACKET_SEED, TankOption, TankTreePacket,
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
    TankTree { tree: TankTree },
    ChatMessage { id: u32, channel: u8, text: String },
}

const TANK_RADIUS: f32 = 20.0;
const BULLET_RADIUS: f32 = 8.0;
const BULLET_LIFETIME: f32 = 1.5;
const MAX_LEVEL: u32 = 45;

const MAP_BOUND: f32 = 2500.0;
const MAX_SHAPES: usize = 1500;
const BULLET_SUBSTEPS: u32 = 5;
const ENTITY_COLLISION_QUERY_RADIUS: f32 = 80.0;

const RECOIL_SCALE: f32 = 10.0;

const TANK_RELOAD_TICKS: u64 = 10;

const MIN_UPGRADE_INTERVAL_MS: u64 = 250;
const MIN_AIM_SAMPLES: usize = 16;
const MAX_HUMAN_ANGULAR_VELOCITY: f32 = 75.0;
const MAX_HUMAN_ANGULAR_JERK: f32 = 10_000.0;
const MAX_SIGN_INVERSION_RATIO: f32 = 0.90;
const MULTIBOX_MIN_SAMPLES: usize = 16;
const MULTIBOX_THRESHOLD_RAD: f32 = 0.05;
const MULTIBOX_SIMILARITY_FLAG: f32 = 0.97;
const SUSPICION_DECAY: u32 = 3;

const NUM_TEAMS: u8 = 2;
const CHAT_WINDOW_MS: u64 = 10_000;
const CHAT_BURST: usize = 5;
const CHAT_MIN_GAP_MS: u64 = 400;
const CHAT_MAX_CHARS: usize = 120;

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
        })
    }

    pub async fn game_loop(&mut self) {
        let tick_rate = Duration::from_millis(100);
        let dt = tick_rate.as_secs_f32();
        let mut interval = tokio::time::interval(tick_rate);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            self.tick_count += 1;
            self.drain_events();
            self.reload_tanks();
            self.tick_shape_orbits(dt);

            self.regen_health();
            self.rebuild_spatial_hash();
            self.resolve_entity_collisions();
            self.clamp_shape_positions();

            let entity_updates = self.compute_entity_updates(dt);
            self.connections
                .broadcast(UpdateEntityPacket::new(entity_updates, PACKET_SEED as u64));

            self.broadcast_player_stats();
            self.send_tank_upgrade_offers();
            self.run_anti_cheat_pass();
            if self.tick_count % 10 == 0 {
                self.broadcast_leaderboard();
            }

            self.rebuild_spatial_hash();
            self.simulate_bullets(dt);
            self.fire_auto_weapons(dt);

            self.scripting.scheduler.on_tick();
            self.tick_shape_spawns();

            interval.tick().await;
        }
    }

    fn reload_tanks(&mut self) {
        if self.tick_count % TANK_RELOAD_TICKS != 0 {
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
                if now.saturating_sub(last) < CHAT_MIN_GAP_MS {
                    return;
                }
            }
            times.push_back(now);
            while let Some(&front) = times.front() {
                if now.saturating_sub(front) > CHAT_WINDOW_MS {
                    times.pop_front();
                } else {
                    break;
                }
            }
            if times.len() > CHAT_BURST {
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
        if text.is_empty() || text.chars().count() > CHAT_MAX_CHARS {
            return;
        }
        let censored = text.censor().to_string();

        let team = self.teams.get(&conn_id).copied().unwrap_or(0);
        let packet = ChatMessagePacket::new(
            channel,
            team,
            now / 1000,
            name,
            censored,
            PACKET_SEED as u64,
        );

        if channel == 1 {
            let recipients: Vec<u32> = self
                .teams
                .iter()
                .filter(|&(_, &t)| t == team)
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
        let mut counts = [0usize; NUM_TEAMS as usize];
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
        let other_packet = AddEntityPacket::new(
            conn_id,
            EntityType::Player,
            x,
            y,
            level,
            name,
            false,
            barrels,
            PACKET_SEED as u64,
        );
        self.connections.send_to(conn_id, my_packet);
        self.connections
            .broadcast_with_exceptions(other_packet, &[conn_id]);
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
        if analysis.samples >= MIN_AIM_SAMPLES {
            if analysis.max_velocity > MAX_HUMAN_ANGULAR_VELOCITY {
                watch.tracker.add_score(10);
            }
            if analysis.max_jerk > MAX_HUMAN_ANGULAR_JERK {
                watch.tracker.add_score(6);
            }
            if analysis.sign_inversion > MAX_SIGN_INVERSION_RATIO {
                watch.tracker.add_score(4);
            }
        }
    }

    fn apply_tank_upgrade(&mut self, conn_id: u32, tank_id: u32) {
        let Some(&entity_id) = self.players.get(&conn_id) else {
            return;
        };
        let Some((tier, def)) = find_def(&self.tank_tree, tank_id) else {
            error!("player {conn_id} requested unknown tank {tank_id}");
            return;
        };

        let now = now_ms();
        {
            let watch = self.watch.entry(conn_id).or_default();
            if !upgrade_anti::check_upgrade_interval(
                watch.last_upgrade_ms,
                now,
                MIN_UPGRADE_INTERVAL_MS,
            ) {
                watch.tracker.add_score(25);
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
                watch.tracker.decay(SUSPICION_DECAY);
            }
        }

        if self.tick_count % 10 != 0 {
            return;
        }

        let series: Vec<(u32, Vec<f32>)> = self
            .watch
            .iter()
            .filter(|(_, watch)| watch.aim.len() >= MULTIBOX_MIN_SAMPLES)
            .map(|(id, watch)| (*id, watch.aim.angles()))
            .collect();

        for i in 0..series.len() {
            for j in (i + 1)..series.len() {
                let similarity = multibox_anti::calculate_input_similarity(
                    &series[i].1,
                    &series[j].1,
                    MULTIBOX_THRESHOLD_RAD,
                );
                if similarity >= MULTIBOX_SIMILARITY_FLAG {
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
        let world_size = self.config.world.size;
        let x = (self.rnd.generate::<f32>() * world_size) - world_size / 2.0;
        let y = (self.rnd.generate::<f32>() * world_size) - world_size / 2.0;

        let entity_id = self.scripting.entities_mut().spawn_tank(
            Vec2::new(x, y),
            Vec2::ZERO,
            self.config.player.default_health,
            name.to_string(),
        );
        self.players.insert(id, entity_id);

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
        let other_packet = AddEntityPacket::new(
            id,
            EntityType::Player,
            x,
            y,
            1,
            name.to_string(),
            false,
            barrels,
            PACKET_SEED as u64,
        );
        self.connections.send_to(id, my_packet);
        self.connections
            .broadcast_with_exceptions(other_packet, &[id]);

        let entities = self.scripting.entities_mut();
        for (&other_id, &other_entity_id) in self.players.iter() {
            if other_id == id {
                continue;
            }
            let Some(entity) = entities.get(other_entity_id) else {
                continue;
            };
            let Some(tank) = entities.tanks.get(other_entity_id) else {
                continue;
            };
            let existing_packet = AddEntityPacket::new(
                other_id,
                EntityType::Player,
                entity.position.x,
                entity.position.y,
                *tank.level,
                tank.name.to_string(),
                false,
                tank.barrels.clone(),
                PACKET_SEED as u64,
            );
            self.connections.send_to(id, existing_packet);
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
        let mut entities = self.scripting.entities_mut();
        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }
            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };
            let max_health = entity_max_health(&entities, id);
            let Some(health) = entities.health_mut(id) else {
                continue;
            };
            if *health > 0 && *health < max_health {
                // let regen_amount = (max_health / 10000).max(1);
                // *health = (*health + regen_amount).min(max_health);
            }
        }
    }

    fn clamp_shape_positions(&mut self) {
        let mut entities = self.scripting.entities_mut();
        for center in entities.shapes.centers.iter_mut() {
            center.x = center.x.clamp(-MAP_BOUND, MAP_BOUND);
            center.y = center.y.clamp(-MAP_BOUND, MAP_BOUND);
        }
    }

    fn compute_entity_updates(&mut self, dt: f32) -> Vec<UpdateEntityPacketData> {
        let entity_to_conn: HashMap<EntityId, u32> =
            self.players.iter().map(|(&k, &v)| (v, k)).collect();

        let mut entities = self.scripting.entities_mut();
        let mut updates = Vec::new();

        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }
            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };
            let tank_data = entities
                .tanks
                .get(id)
                .map(|t| (*t.move_dir, *t.aim, *t.level, t.tank_type.speed));
            let health = entities.get(id).map(|e| *e.health).unwrap_or(0);
            let max_health = entity_max_health(&entities, id);

            if let Some((move_dir, aim, level, speed_mult)) = tank_data {
                let current_max_speed =
                    self.config.player.speed * (1.0 + (level - 1) as f32 * 0.02) * speed_mult;
                step_tank_velocity(&mut entities, i, move_dir, current_max_speed, dt);
                clamp_tank_position(&mut entities, i);

                if let Some(conn_id) = entity_to_conn.get(&id).copied() {
                    let scale = level_scale(level);
                    updates.push(UpdateEntityPacketData {
                        id: conn_id,
                        entity_type: EntityType::Player,
                        x: entities.positions[i].x,
                        y: entities.positions[i].y,
                        rot: aim,
                        scale,
                        kind: 0,
                        health,
                        max_health,
                    });
                }
            } else if let Some(shape) = entities.shapes.get(id) {
                let net_id = 0x80000000 | (i as u32);
                let rot = *shape.rotation;
                let kind = shape_kind_id(*shape.kind);
                updates.push(UpdateEntityPacketData {
                    id: net_id,
                    entity_type: EntityType::Shape,
                    x: entities.positions[i].x,
                    y: entities.positions[i].y,
                    rot,
                    scale: 1.0,
                    kind,
                    health,
                    max_health,
                });
            }
        }

        for (net_id, pos) in entities.bullets.iter() {
            updates.push(UpdateEntityPacketData {
                id: net_id,
                entity_type: EntityType::Bullet,
                x: pos.x,
                y: pos.y,
                rot: 0.0,
                scale: 1.0,
                kind: 0,
                health: 1,
                max_health: 1,
            });
        }

        updates
    }

    fn broadcast_player_stats(&mut self) {
        let entities = self.scripting.entities_mut();
        for (&conn_id, &entity_id) in self.players.iter() {
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
    }

    fn simulate_bullets(&mut self, dt: f32) {
        let sub_dt = dt / BULLET_SUBSTEPS as f32;
        for _ in 0..BULLET_SUBSTEPS {
            self.scripting.entities_mut().bullets.tick(sub_dt);
            self.resolve_bullet_collisions();
        }
    }

    fn resolve_entity_collisions(&mut self) {
        let mut entities = self.scripting.entities_mut();

        let mut collidable: Vec<Option<(Vec2, f32, bool)>> = vec![None; entities.alive.len()];
        for i in 0..entities.alive.len() {
            if !entities.alive[i] {
                continue;
            }
            let id = EntityId {
                index: i,
                generation: entities.generations[i],
            };
            if let Some((radius, is_tank)) = entity_collision_radius(&entities, id) {
                collidable[i] = Some((entities.positions[i], radius, is_tank));
            }
        }

        let mut collision_hits: Vec<(EntityId, u32)> = Vec::new();

        for i in 0..collidable.len() {
            let Some((pos1, r1, is_tank1)) = collidable[i] else {
                continue;
            };
            let id1 = EntityId {
                index: i,
                generation: entities.generations[i],
            };

            let nearby =
                self.spatial_hash
                    .get_nearby(pos1.x, pos1.y, ENTITY_COLLISION_QUERY_RADIUS);
            for candidate in nearby {
                let HashEntity::Entity(id2) = candidate else {
                    continue;
                };
                if id2.index <= i {
                    continue;
                }
                let Some((pos2, r2, is_tank2)) = collidable[id2.index] else {
                    continue;
                };

                let delta = pos2 - pos1;
                let dist = delta.length();
                let min_dist = r1 + r2;
                if dist >= min_dist || dist <= 0.0 {
                    continue;
                }
                let push = min_dist - dist;
                let dir = delta / dist;

                match (is_tank1, is_tank2) {
                    (true, true) => {
                        entities.velocities[i] -= dir * push * 5.0;
                        entities.velocities[id2.index] += dir * push * 5.0;
                        collision_hits.push((id1, 2));
                        collision_hits.push((id2, 2));
                    }
                    (true, false) => {
                        entities.shapes.push_center(id2, dir * push * 0.5);
                        let vel = entities.velocities[i];
                        entities.velocities[i] -= dir * vel.dot(dir) * 2.0;
                        collision_hits.push((id1, 1));
                        collision_hits.push((id2, 15));
                    }
                    (false, true) => {
                        entities.shapes.push_center(id1, -dir * push * 0.5);
                        let vel = entities.velocities[id2.index];
                        entities.velocities[id2.index] -= -dir * vel.dot(-dir) * 2.0;
                        collision_hits.push((id1, 15));
                        collision_hits.push((id2, 1));
                    }
                    (false, false) => {
                        entities.shapes.push_center(id1, -dir * push * 0.25);
                        entities.shapes.push_center(id2, dir * push * 0.25);
                    }
                }
            }
        }
        drop(entities);

        if collision_hits.is_empty() {
            return;
        }

        let entity_to_conn: HashMap<EntityId, u32> =
            self.players.iter().map(|(&k, &v)| (v, k)).collect();
        let mut entities = self.scripting.entities_mut();

        for (target_id, damage) in collision_hits {
            let died = {
                let Some(health) = entities.health_mut(target_id) else {
                    continue;
                };
                let was_alive = *health > 0;
                *health = health.saturating_sub(damage);
                was_alive && *health == 0
            };
            if died {
                broadcast_despawn(&self.connections, &mut entities, &entity_to_conn, target_id);
            }
        }
    }

    fn resolve_bullet_collisions(&mut self) {
        let hits: Vec<(usize, EntityId, u32, EntityId)> = {
            let entities = self.scripting.entities_mut();
            let mut hits = Vec::new();
            for (bullet_index, pos, damage, owner) in entities.bullets.iter_indexed() {
                let nearby = self.spatial_hash.get_nearby(pos.x, pos.y, 40.0);
                for candidate in nearby {
                    let HashEntity::Entity(target_id) = candidate else {
                        continue;
                    };
                    if target_id == *owner {
                        continue;
                    }
                    let Some(target) = entities.get(target_id) else {
                        continue;
                    };
                    let Some((target_radius, _)) = entity_collision_radius(&entities, target_id)
                    else {
                        continue;
                    };
                    if pos.distance(*target.position) > target_radius + BULLET_RADIUS {
                        continue;
                    }
                    hits.push((bullet_index, target_id, *damage, *owner));
                    break;
                }
            }
            hits
        };

        if hits.is_empty() {
            return;
        }

        let mut entities = self.scripting.entities_mut();
        let entity_to_conn: HashMap<EntityId, u32> =
            self.players.iter().map(|(&k, &v)| (v, k)).collect();

        for (_, target_id, damage, owner) in &hits {
            let died = {
                let Some(health) = entities.health_mut(*target_id) else {
                    continue;
                };
                let was_alive = *health > 0;
                *health = health.saturating_sub(*damage);
                was_alive && *health == 0
            };
            if !died {
                continue;
            }

            let xp_gained = if let Some(tank) = entities.tanks.get(*target_id) {
                Some(tank.xp.0 / 2)
            } else {
                entities.shapes.get(*target_id).map(|s| *s.xp_reward)
            };
            if let Some(xp_gained) = xp_gained {
                grant_xp(&mut entities, *owner, xp_gained);
            }

            broadcast_despawn(
                &self.connections,
                &mut entities,
                &entity_to_conn,
                *target_id,
            );
        }

        let mut spent: Vec<usize> = hits.iter().map(|(b, _, _, _)| *b).collect();
        spent.sort_unstable_by(|a, b| b.cmp(a));
        spent.dedup();
        for i in spent {
            entities.bullets.remove(i);
        }
    }

    fn tick_shape_spawns(&mut self) {
        if self.players.is_empty() {
            return;
        }

        let mut entities = self.scripting.entities_mut();
        while entities.shapes.len() < MAX_SHAPES {
            let x = (self.rnd.generate::<f32>() * MAP_BOUND * 2.0) - MAP_BOUND;
            let y = (self.rnd.generate::<f32>() * MAP_BOUND * 2.0) - MAP_BOUND;
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

            let entity_id = entities.spawn_shape(
                center,
                kind,
                health,
                rot_speed,
                xp,
                orbit_radius,
                orbit_angle,
                orbit_speed,
            );

            let net_id = 0x80000000 | (entity_id.index as u32);
            let packet = AddEntityPacket::new(
                net_id,
                EntityType::Shape,
                center.x,
                center.y,
                shape_kind_id(kind),
                String::new(),
                false,
                vec![],
                PACKET_SEED as u64,
            );
            self.connections.broadcast(packet);
        }
    }

    fn fire_auto_weapons(&mut self, dt: f32) {
        let ids: Vec<EntityId> = self.players.values().copied().collect();

        let mut entities = self.scripting.entities_mut();
        let Entities {
            tanks,
            bullets,
            positions,
            velocities,
            alive,
            ..
        } = &mut *entities;

        for id in ids {
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
            let base_damage = *t.bullet_damage;
            let base_speed = *t.bullet_speed;
            let reload_time = *t.reload_time;

            for (i, barrel) in t.tank_type.barrels.iter().enumerate() {
                while t.barrel_timers.len() <= i {
                    t.barrel_timers.push(0.0);
                }

                let timer = &mut t.barrel_timers[i];
                *timer -= dt;
                if *timer > 0.0 {
                    continue;
                }

                if !auto_fire {
                    *timer = 0.0;
                    continue;
                }

                let cycle = (reload_time * barrel.reload).max(0.05);
                *timer += cycle;

                let barrel_angle = barrel.angle.to_radians();
                let muzzle_local =
                    Vec2::new(barrel.x, barrel.y) + Vec2::from_angle(barrel_angle) * barrel.length;
                let world_angle = aim + barrel_angle;
                let muzzle_world = position + Vec2::from_angle(aim).rotate(muzzle_local);

                let damage = ((base_damage as f32 * barrel.bullet.damage).round() as u32).max(1);
                let speed = base_speed * barrel.bullet.speed;
                let lifetime = BULLET_LIFETIME * barrel.bullet.life_length;

                bullets.spawn(
                    muzzle_world,
                    Vec2::from_angle(world_angle) * speed,
                    damage,
                    lifetime,
                    id,
                );

                velocities[id.index] -=
                    Vec2::from_angle(world_angle) * (barrel.recoil * RECOIL_SCALE);
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

fn shape_max_health(kind: ShapeKind) -> u32 {
    match kind {
        ShapeKind::Square => 10,
        ShapeKind::Triangle => 30,
        ShapeKind::Pentagon => 100,
    }
}

fn shape_radius(kind: ShapeKind) -> f32 {
    match kind {
        ShapeKind::Square => 15.0,
        ShapeKind::Triangle => 20.0,
        ShapeKind::Pentagon => 35.0,
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
        (ShapeKind::Square, 0.05, 10)
    } else if roll < 0.95 {
        (ShapeKind::Triangle, 0.07, 25)
    } else {
        (ShapeKind::Pentagon, 0.03, 130)
    }
}

fn entity_max_health(entities: &Entities, id: EntityId) -> u32 {
    entities
        .tanks
        .get(id)
        .map(|t| *t.max_health)
        .or_else(|| entities.shapes.get(id).map(|s| shape_max_health(*s.kind)))
        .unwrap_or(100)
}

fn entity_collision_radius(entities: &Entities, id: EntityId) -> Option<(f32, bool)> {
    if entities.tanks.get(id).is_some() {
        Some((TANK_RADIUS, true))
    } else {
        entities
            .shapes
            .get(id)
            .map(|s| (shape_radius(*s.kind), false))
    }
}

fn net_id_for(
    entities: &Entities,
    entity_to_conn: &HashMap<EntityId, u32>,
    id: EntityId,
) -> (u32, EntityType) {
    if entities.tanks.get(id).is_some() {
        (
            entity_to_conn.get(&id).copied().unwrap_or(0),
            EntityType::Player,
        )
    } else {
        (0x80000000 | (id.index as u32), EntityType::Shape)
    }
}

fn broadcast_despawn(
    connections: &Connections,
    entities: &mut Entities,
    entity_to_conn: &HashMap<EntityId, u32>,
    id: EntityId,
) {
    let (net_id, entity_type) = net_id_for(entities, entity_to_conn, id);
    connections.broadcast(RemoveEntityPacket::new(
        net_id,
        entity_type,
        PACKET_SEED as u64,
    ));
    entities.despawn(id);
}

fn grant_xp(entities: &mut Entities, owner: EntityId, xp_gained: u32) {
    let Some(owner_tank) = entities.tanks.get_mut(owner) else {
        return;
    };
    owner_tank.xp.0 = owner_tank.xp.0.saturating_add(xp_gained);
    let start_level = *owner_tank.level;
    while owner_tank.xp.0 >= owner_tank.xp.1 && *owner_tank.level < MAX_LEVEL {
        owner_tank.xp.0 -= owner_tank.xp.1;
        owner_tank.xp.1 = 1; //(owner_tank.xp.1 as f32 * 1.12).min(100000.0) as u32;
        *owner_tank.level += 1;
        *owner_tank.max_health += 10;
        *owner_tank.bullet_damage += 2;
        *owner_tank.bullet_speed += 10.0;
        *owner_tank.reload_time = (*owner_tank.reload_time * 0.98).max(0.1);
    }

    if *owner_tank.level == MAX_LEVEL && start_level < MAX_LEVEL {
        owner_tank.xp.0 = owner_tank.xp.0.min(owner_tank.xp.1.saturating_sub(1));
    }

    let levels_gained = *owner_tank.level - start_level;
    let new_max_health = *owner_tank.max_health;
    if levels_gained > 0 {
        if let Some(h) = entities.health_mut(owner) {
            *h = (*h + levels_gained * 10).min(new_max_health);
        }
    }
}

fn step_tank_velocity(
    entities: &mut Entities,
    i: usize,
    move_dir: Option<f32>,
    max_speed: f32,
    dt: f32,
) {
    if let Some(dir) = move_dir {
        let target = Vec2::from_angle(dir) * max_speed;
        let delta = target - entities.velocities[i];
        let dist = delta.length();
        let max_step = 800.0 * dt;
        entities.velocities[i] = if dist > max_step {
            entities.velocities[i] + delta / dist * max_step
        } else {
            target
        };
    } else {
        let speed = entities.velocities[i].length();
        let drop = 500.0 * dt;
        entities.velocities[i] = if speed > drop {
            let vel = entities.velocities[i];
            vel - vel / speed * drop
        } else {
            Vec2::ZERO
        };
    }

    entities.velocities[i] =
        entities.velocities[i].clamp(Vec2::splat(-max_speed), Vec2::splat(max_speed));
    entities.positions[i] += entities.velocities[i] * dt;
}

fn clamp_tank_position(entities: &mut Entities, i: usize) {
    let pos = &mut entities.positions[i];
    let vel = &mut entities.velocities[i];
    if pos.x < -MAP_BOUND {
        pos.x = -MAP_BOUND;
        vel.x = 0.0;
    }
    if pos.x > MAP_BOUND {
        pos.x = MAP_BOUND;
        vel.x = 0.0;
    }
    if pos.y < -MAP_BOUND {
        pos.y = -MAP_BOUND;
        vel.y = 0.0;
    }
    if pos.y > MAP_BOUND {
        pos.y = MAP_BOUND;
        vel.y = 0.0;
    }
}
