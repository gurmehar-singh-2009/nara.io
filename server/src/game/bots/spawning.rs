use std::collections::HashMap;

use glam::Vec2;
use nanorand::Rng;
use paris::info;

use super::{BotBrain, DStarLite};
use crate::{
    entities::{
        entity::{Entities, EntityId},
        tank::stat_max,
    },
    fs::load_config::Config,
    scripting::scripting::Scripting,
};

pub fn tick(
    scripting: &mut Scripting,
    config: &Config,
    players: &mut HashMap<u32, EntityId>,
    brains: &mut HashMap<u32, BotBrain>,
    respawns: &mut Vec<(u32, u64)>,
    rnd: &mut nanorand::WyRand,
    nav_danger: &[f32],
    nav_version: u64,
    tick_count: u64,
) -> Vec<(u32, EntityId)> {
    let mut spawned = Vec::new();

    if brains.is_empty() && respawns.is_empty() {
        let num_bots = config.bots.count;

        for i in 0..num_bots {
            let id = config.bots.id_base + i as u32;
            let eid = spawn(
                scripting,
                config,
                players,
                brains,
                rnd,
                nav_danger,
                nav_version,
                tick_count,
                id,
            );
            spawned.push((id, eid));
        }

        info!("spawned {num_bots} bots");

        return spawned;
    }

    let due: Vec<u32> = respawns
        .iter()
        .filter(|(_, at)| *at <= tick_count)
        .map(|(id, _)| *id)
        .collect();

    if due.is_empty() {
        return spawned;
    }

    respawns.retain(|(_, at)| *at > tick_count);

    for id in due {
        let eid = spawn(
            scripting,
            config,
            players,
            brains,
            rnd,
            nav_danger,
            nav_version,
            tick_count,
            id,
        );
        spawned.push((id, eid));
    }

    spawned
}

fn spawn(
    scripting: &mut Scripting,
    config: &Config,
    players: &mut HashMap<u32, EntityId>,
    brains: &mut HashMap<u32, BotBrain>,
    rnd: &mut nanorand::WyRand,
    nav_danger: &[f32],
    nav_version: u64,
    tick_count: u64,
    bot_id: u32,
) -> EntityId {
    let name = config.bots.names[rnd.generate::<u32>() as usize % config.bots.names.len()].clone();

    let bound = config.world.map_bound - 150.0;
    let wander = Vec2::new(
        (rnd.generate::<f32>() * 2.0 - 1.0) * bound,
        (rnd.generate::<f32>() * 2.0 - 1.0) * bound,
    );

    let entity_id = scripting.entities_mut().spawn_tank(
        Vec2::ZERO,
        Vec2::ZERO,
        config.player.default_health,
        name,
    );

    let planner = DStarLite::new(nav_danger);

    players.insert(bot_id, entity_id);
    brains.insert(
        bot_id,
        BotBrain {
            entity: entity_id,
            wander_target: wander,
            next_wander_tick: tick_count + config.bots.wander_interval,
            target: None,
            target_is_tank: false,
            target_is_bot: false,
            target_last_pos: Vec2::ZERO,
            target_last_tick: 0,
            target_vel: Vec2::ZERO,
            farm_ram: false,
            flee_dir: Vec2::ZERO,
            flee_from: None,
            aggro_on: None,
            aggro_tick: 0,
            force_macro: true,
            react_until_tick: 0,
            planner,
            nav_version_seen: nav_version,
            path: Vec::new(),
            path_goal: Vec2::ZERO,
            next_repath_tick: 0,
            combat_move: None,
            combat_fire: false,
            move_vec: Vec2::ZERO,
            skill: config.bots.skill_min + rnd.generate::<f32>() * config.bots.skill_span,
            aim_phase: rnd.generate::<f32>() * std::f32::consts::TAU,
        },
    );

    let bonus_points = rnd.generate::<u32>() % 9; // 0..=8
    spend_stat_points(
        &mut *scripting.entities_mut(),
        rnd,
        entity_id,
        bonus_points,
        config,
    );

    entity_id
}

pub fn spend_stat_points(
    entities: &mut Entities,
    rnd: &mut nanorand::WyRand,
    entity_id: EntityId,
    extra: u32,
    config: &Config,
) {
    let Some(t) = entities.tanks.get_mut(entity_id) else {
        return;
    };

    let mut remaining = *t.stat_points + extra;
    *t.stat_points = 0;

    let mut attempts = 0;
    while remaining > 0 && attempts < 512 {
        attempts += 1;
        let stat = (rnd.generate::<u8>() % 8) as usize;

        if t.stat_levels[stat] >= stat_max(t.tank_type, stat, config.player.stat_max_level) {
            continue;
        }

        t.stat_levels[stat] += 1;
        remaining -= 1;
    }
}
