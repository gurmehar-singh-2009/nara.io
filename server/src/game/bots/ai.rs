use std::collections::HashMap;

use glam::Vec2;
use nanorand::Rng;

use super::{
    combat::{DuelCtx, DuelState, minimax_decide},
    navigation::{NAV_CELLS, NAV_LEN, nav_world_to_cell, nearest_passable_cell, string_pull},
    perception::{
        Enemy, escape_direction, fire_line_clear, intercept_angle, is_bot_id, scan_enemies,
        scan_positions, tank_power,
    },
    steering::context_steer,
};
use crate::{
    entities::{
        entity::{Entities, EntityId},
        shape::ShapeKind,
        spatial_hash::{HashEntity, SpatialHash},
    },
    fs::load_config::Config,
    game::{bots::BotBrain, game_state::stat_mult},
};

pub fn tick_ai(
    brains: &mut HashMap<u32, BotBrain>,
    players: &HashMap<u32, EntityId>,
    spatial_hash: &SpatialHash,
    nav_danger: &[f32],
    rnd: &mut nanorand::WyRand,
    entities: &mut Entities,
    nav_version: u64,
    tick_count: u64,
    player_speed: f32,
    nearby: &mut Vec<HashEntity>,
    config: &Config,
) -> Vec<(u32, EntityId)> {
    let mut upgrade_candidates: Vec<(u32, EntityId)> = Vec::new();
    let bot_ids: Vec<u32> = brains.keys().copied().collect();

    let mut enemies: Vec<Enemy> = Vec::with_capacity(config.bots.max_tracked_enemies);
    let mut brawl: Vec<(EntityId, Vec2, f32)> = Vec::with_capacity(16);
    let mut flee_positions: Vec<Vec2> = Vec::with_capacity(16);

    let bullet_range = player_speed * config.bullet.speed_mult * config.bullet.lifetime;
    let bullet_speed = player_speed * config.bullet.speed_mult;
    let fire_range = (bullet_range * 0.8).min(config.bots.vision);
    let base_move_speed = player_speed;

    for bot_id in bot_ids {
        let Some(brain) = brains.get_mut(&bot_id) else {
            continue;
        };

        let eid = brain.entity;
        if !entities.is_alive(eid) {
            continue;
        }

        let (my_level, my_stats, my_reload, my_def_speed, my_max, my_can_shoot) = {
            let Some(t) = entities.tanks.get(eid) else {
                continue;
            };

            (
                *t.level,
                *t.stat_levels,
                (*t.reload_time).max(0.15),
                t.tank_type.speed,
                *t.max_health,
                t.tank_type.flags.can_shoot,
            )
        };

        let ram_build = !my_can_shoot;

        let pos = entities.positions[eid.index];
        let my_hp = entities.get(eid).map(|e| *e.health).unwrap_or(0) as f32;
        let my_hp_frac = if my_max > 0 {
            my_hp / my_max as f32
        } else {
            0.0
        };
        let my_speed = base_move_speed
            * (1.0 + (my_level - 1) as f32 * 0.02)
            * my_def_speed
            * stat_mult(&my_stats, 7, config.player.stat_move_per);
        let my_power = my_level as f32 * (0.35 + 0.65 * my_hp_frac);

        if let Some(target) = brain.target {
            if entities.is_alive(target) {
                let tpos = entities.positions[target.index];

                if brain.target_last_tick + 1 == tick_count {
                    let v = (tpos - brain.target_last_pos) / 0.1;

                    brain.target_vel = if v.length() > 600.0 {
                        v.normalize_or_zero() * 600.0
                    } else {
                        v
                    };
                }

                brain.target_last_pos = tpos;
                brain.target_last_tick = tick_count;
            }
        }

        let staggered =
            (bot_id.wrapping_add(tick_count as u32)) % config.bots.macro_interval as u32 == 0;
        let target_dead = brain
            .target
            .map(|te| !entities.is_alive(te))
            .unwrap_or(false);

        if staggered || target_dead || brain.force_macro {
            brain.force_macro = false;

            let old_target = brain.target;
            let was_fleeing = brain.flee_from.is_some();

            brain.target = None;
            brain.target_is_tank = false;
            brain.target_is_bot = false;
            brain.target_vel = Vec2::ZERO;
            brain.farm_ram = false;

            scan_enemies(
                entities,
                players,
                eid,
                pos,
                config.bots.engage_vision,
                config.bots.noise_power_estimate,
                rnd,
                &mut enemies,
                config,
            );

            // retaliation
            let mut chosen: Option<(EntityId, bool, bool)> = None;
            if let Some(aggro_conn) = brain.aggro_on {
                let fresh =
                    tick_count.saturating_sub(brain.aggro_tick) <= config.bots.aggro_timeout_ticks;

                if !fresh {
                    brain.aggro_on = None;
                } else if let Some(&ae) = players.get(&aggro_conn) {
                    let est = enemies
                        .iter()
                        .find(|e| e.entity == ae)
                        .map(|e| e.power)
                        .unwrap_or_else(|| tank_power(entities, ae));

                    if entities.is_alive(ae)
                        && pos.distance(entities.positions[ae.index]) < 1600.0
                        && my_power >= est * 0.55
                    {
                        chosen = Some((ae, true, is_bot_id(aggro_conn, config)));
                    }
                }
            }

            if chosen.is_none() {
                let i_am_fragile = my_hp_frac < config.bots.low_health_frac;
                let mut threat_power = 0.0f32;
                let mut n_close = 0usize;

                for e in enemies.iter() {
                    if e.dist < config.bots.threat_vision {
                        threat_power += e.power;
                        n_close += 1;
                    }
                }

                let outnumbered =
                    n_close >= 2 && threat_power > my_power * config.bots.outnumber_ratio;
                if outnumbered || (i_am_fragile && n_close >= 1 && threat_power > my_power) {
                    // run through the biggest gap in their encirclement
                    flee_positions.clear();

                    for e in enemies.iter() {
                        if e.dist < 900.0 {
                            flee_positions.push(e.pos);
                        }
                    }

                    brain.flee_dir = escape_direction(pos, &flee_positions);
                    brain.flee_from = Some(enemies[0].pos);
                } else {
                    // pick a duel target
                    let mut best: Option<(f32, EntityId, bool)> = None; // (score, entity, is_bot)

                    for e in enemies.iter() {
                        let mut power = e.power;

                        if Some(e.entity) == old_target {
                            power *= 0.85; // stickiness
                        }

                        let rank_d = if e.is_bot {
                            e.dist * config.bots.vs_bot_bias
                        } else {
                            e.dist
                        };

                        let finishing_blow = my_hp_frac > 0.5 && e.hp_frac < 0.3;
                        if my_power >= power * config.bots.engage_power_ratio || finishing_blow {
                            // weakest first, then nearest
                            let score = power + rank_d * 0.5;

                            if best.map_or(true, |(bs, _, _)| score < bs) {
                                best = Some((score, e.entity, e.is_bot));
                            }
                        } else if e.dist < config.bots.threat_vision
                            && power > my_power * 1.6
                            && brain.flee_from.is_none()
                        {
                            // a single scarier enemy
                            brain.flee_dir = (pos - e.pos).normalize_or_zero();
                            brain.flee_from = Some(e.pos);
                        }
                    }

                    if let Some((_, pe, is_bot)) = best {
                        chosen = Some((pe, true, is_bot));
                    }
                }
            }

            if chosen.is_none() && brain.flee_from.is_none() {
                spatial_hash.get_nearby_into(nearby, pos.x, pos.y, config.bots.vision);
                let mut best_shape: Option<(f32, EntityId, bool)> = None;

                for &candidate in nearby.iter() {
                    let HashEntity::Entity(se) = candidate else {
                        continue;
                    };

                    if !entities.is_alive(se) {
                        continue;
                    }

                    let Some(shape) = entities.shapes.get(se) else {
                        continue;
                    };
                    let spos = entities.positions[se.index];
                    let d = pos.distance(spos);

                    if d > config.bots.vision {
                        continue;
                    }

                    let score = *shape.xp_reward as f32 / (d + 100.0);
                    if best_shape.map_or(true, |(bs, _, _)| score > bs) {
                        let ram = *shape.kind != ShapeKind::Pentagon;
                        best_shape = Some((score, se, ram));
                    }
                }

                if let Some((_, se, ram)) = best_shape {
                    chosen = Some((se, false, false));
                    brain.farm_ram = ram;
                }
            }

            if let Some((te, is_tank, is_bot)) = chosen {
                brain.target = Some(te);
                brain.target_is_tank = is_tank;
                brain.target_is_bot = is_bot;
                brain.target_last_pos = entities.positions[te.index];
                brain.target_last_tick = tick_count;
                brain.target_vel = Vec2::ZERO;
                brain.flee_from = None;
            }

            // some human-like delay, maybe makes it better idk
            // before it was super op and too efficient...
            let situation_changed =
                brain.target != old_target || brain.flee_from.is_some() != was_fleeing;

            if situation_changed {
                brain.react_until_tick = tick_count
                    + config.bots.reaction_min
                    + (rnd.generate::<u32>() % config.bots.reaction_span as u32) as u64;

                brain.combat_move = None;
                brain.combat_fire = false;
            }
        }

        let reacting = tick_count < brain.react_until_tick;
        let mut desired = Vec2::ZERO;
        let mut hold = false;
        let mut ignore_obs: Option<EntityId> = None;
        let mut want_fire = false;
        let mut aim_at: Option<Vec2> = None;
        let aim_vel = brain.target_vel;
        let mut combat = false;
        let mut far_approach = false;

        let fleeing = brain.flee_from.is_some();
        let combat_mode = brain.target.is_some() && brain.target_is_tank;
        brawl.clear();

        if fleeing || combat_mode {
            let vision = if fleeing { 850.0 } else { 600.0 };

            scan_positions(entities, players, eid, pos, vision, &mut brawl);
        }

        if let Some(flee_pos) = brain.flee_from {
            let dist = pos.distance(flee_pos);

            if dist > 1300.0 {
                brain.flee_from = None;
            } else {
                if !brawl.is_empty() {
                    flee_positions.clear();

                    for &(_, p, _) in brawl.iter() {
                        flee_positions.push(p);
                    }

                    brain.flee_dir = escape_direction(pos, &flee_positions);
                    brain.flee_from = Some(brawl[0].1); // nearest chaser
                }

                let aim_target = brain.flee_from.unwrap_or(flee_pos);
                let aim_dist = pos.distance(aim_target);
                desired = if brain.flee_dir.length_squared() > 1e-4 {
                    brain.flee_dir
                } else {
                    pos - flee_pos
                };

                aim_at = Some(aim_target);
                want_fire = aim_dist < fire_range
                    && fire_line_clear(entities, spatial_hash, nearby, pos, aim_target, 12.0, None);
            }
        }

        if brain.flee_from.is_none() {
            if let Some(target) = brain.target {
                if !entities.is_alive(target) {
                    brain.target = None;
                }
            }

            if let Some(target) = brain.target {
                let tpos = entities.positions[target.index];
                let dist = pos.distance(tpos);
                aim_at = Some(tpos);

                if brain.target_is_tank {
                    combat = true;

                    let mut flank_push = Vec2::ZERO;

                    for &(oe, opos, odist) in brawl.iter() {
                        if oe == target || odist > config.bots.flank_radius {
                            continue;
                        }

                        let push = (pos - opos).normalize_or_zero();
                        flank_push += push * (1.0 - odist / config.bots.flank_radius);
                    }

                    let decide_due = (bot_id.wrapping_add(tick_count as u32))
                        % config.bots.decide_interval as u32
                        == 0;

                    if decide_due && !reacting && dist < config.bots.duel_max_dist {
                        let foe_level = entities
                            .tanks
                            .get(target)
                            .map(|ft| *ft.level)
                            .unwrap_or(my_level);
                        let foe_max = entities
                            .tanks
                            .get(target)
                            .map(|ft| *ft.max_health)
                            .unwrap_or(100)
                            .max(1);
                        let foe_hp = entities.get(target).map(|e| *e.health).unwrap_or(0) as f32;
                        let foe_power = tank_power(entities, target);

                        let foe_strength = if brain.target_is_bot {
                            config.bots.bullet_strength
                        } else {
                            1.0
                        };
                        let mut support_dps = 0.0f32;
                        let mut counted = 0usize;

                        for &(oe, _opos, odist) in brawl.iter() {
                            if oe == target || odist > 550.0 {
                                continue;
                            }

                            let lvl = entities.tanks.get(oe).map(|t| *t.level).unwrap_or(my_level);
                            support_dps += config.bullet.base_dmg
                                * (1.0
                                    + config.player.stat_bullet_dmg_per
                                        * lvl.saturating_sub(1) as f32)
                                / 0.45
                                * config.bots.support_dps_frac
                                * foe_strength;

                            counted += 1;

                            if counted >= 4 {
                                break;
                            }
                        }

                        support_dps *= 1.0
                            + (rnd.generate::<f32>() * 2.0 - 1.0)
                                * config.bots.noise_power_estimate;

                        let los_mult = if fire_line_clear(
                            entities,
                            spatial_hash,
                            nearby,
                            pos,
                            tpos,
                            12.0,
                            Some(target),
                        ) {
                            1.0
                        } else {
                            0.1
                        };

                        let me_dps = config.bullet.base_dmg
                            * (1.0 + config.bullet.dmg_per_level * (my_level - 1) as f32)
                            * config.bots.bullet_strength
                            / my_reload
                            * los_mult;
                        let foe_dps = (config.bullet.base_dmg
                            * (1.0 + config.bullet.dmg_per_level * (foe_level - 1) as f32)
                            / 0.45
                            + support_dps)
                            * los_mult;
                        let me_body = config.player.body_dps
                            * stat_mult(&my_stats, 2, config.player.stat_body_dmg_per);
                        let foe_body = config.player.body_dps * 1.5;

                        let aggressive = my_power >= foe_power * (1.0 + counted as f32 * 0.3);
                        let kite = (bullet_range * config.bots.kite_range_frac).min(520.0);
                        let ideal_dist = if ram_build {
                            config.bots.duel_ram_dist
                        } else if aggressive {
                            kite * 0.7
                        } else {
                            kite
                        };

                        let mut threats: Vec<(Vec2, Vec2, f32)> = Vec::new();
                        spatial_hash.get_nearby_into(nearby, pos.x, pos.y, 420.0);

                        for &cand in nearby.iter() {
                            let HashEntity::Bullet(bi) = cand else {
                                continue;
                            };

                            if bi >= entities.bullets.len() {
                                continue;
                            }

                            if entities.bullets.owner(bi) == eid {
                                continue;
                            }

                            let bpos = entities.bullets.position(bi);
                            let bvel = entities.bullets.velocity_at(bi);

                            if bvel.length_squared() < 2500.0 {
                                continue;
                            }

                            if (pos - bpos).dot(bvel) <= 0.0 {
                                continue;
                            }

                            threats.push((bpos, bvel, entities.bullets.damage_at(bi) as f32));

                            if threats.len() >= 12 {
                                break;
                            }
                        }

                        let ctx = DuelCtx {
                            me_speed: my_speed,
                            me_dps,
                            me_max: my_max as f32,
                            me_body,
                            foe_speed: base_move_speed * 1.15,
                            foe_dps,
                            foe_max: foe_max as f32,
                            foe_body,
                            bullet_range,
                            ideal_dist,
                            threats,
                        };
                        let s0 = DuelState {
                            me: pos,
                            me_hp: my_hp,
                            foe: tpos,
                            foe_hp,
                            t: 0.0,
                        };
                        let (mv, fire) = minimax_decide(s0, &ctx, config);

                        brain.combat_move = mv;
                        brain.combat_fire = fire;
                    }

                    if dist > config.bots.duel_max_dist || brain.combat_move.is_none() {
                        far_approach = true;
                        desired = tpos - pos + flank_push;

                        want_fire = dist < fire_range
                            && !reacting
                            && fire_line_clear(
                                entities,
                                spatial_hash,
                                nearby,
                                pos,
                                tpos,
                                12.0,
                                Some(target),
                            );
                    } else {
                        match brain.combat_move {
                            Some(a) => desired = Vec2::from_angle(a) + flank_push * 0.8,
                            None => {
                                if flank_push.length_squared() > 0.25 {
                                    // shoved off our position by flankers
                                    desired = flank_push;
                                } else {
                                    hold = true;
                                }
                            }
                        }

                        want_fire = brain.combat_fire
                            && dist < fire_range * 1.15
                            && !reacting
                            && fire_line_clear(
                                entities,
                                spatial_hash,
                                nearby,
                                pos,
                                tpos,
                                12.0,
                                Some(target),
                            );
                    }

                    ignore_obs = if ram_build && my_power >= tank_power(entities, target) {
                        Some(target)
                    } else {
                        None
                    };
                } else {
                    // farming
                    let stop_at = if brain.farm_ram {
                        30.0
                    } else {
                        fire_range * 0.75
                    };
                    if dist > stop_at {
                        desired = tpos - pos;
                    } else {
                        hold = true;
                    }
                    want_fire = dist < fire_range
                        && fire_line_clear(
                            entities,
                            spatial_hash,
                            nearby,
                            pos,
                            tpos,
                            12.0,
                            Some(target),
                        );
                    if brain.farm_ram {
                        ignore_obs = Some(target);
                    }
                }
            } else {
                // wander
                if (brain.wander_target - pos).length_squared() < 100.0 * 100.0
                    || tick_count >= brain.next_wander_tick
                {
                    let bound = config.world.map_bound - 150.0;
                    brain.wander_target = Vec2::new(
                        (rnd.generate::<f32>() * 2.0 - 1.0) * bound,
                        (rnd.generate::<f32>() * 2.0 - 1.0) * bound,
                    );

                    brain.next_wander_tick = tick_count + config.bots.wander_interval;
                }

                desired = brain.wander_target - pos;
            }
        }

        let goal = if let Some(t) = brain.target {
            entities.positions[t.index]
        } else if brain.flee_from.is_none() {
            brain.wander_target
        } else {
            pos
        };

        if brain.flee_from.is_none() {
            let far = pos.distance(goal) > 650.0;

            if far {
                let (gx, gy) = nav_world_to_cell(goal, config);
                let (gx, gy) = nearest_passable_cell(nav_danger, gx, gy);
                let goal_flat = gy * NAV_CELLS + gx;
                let (mx, my) = nav_world_to_cell(pos, config);
                let my_flat = my * NAV_CELLS + mx;

                let need_init =
                    brain.planner.goal().is_none() || goal.distance(brain.path_goal) > 300.0;
                let mut replanned = false;

                if need_init {
                    brain.planner.initialize(nav_danger, my_flat, goal_flat);
                    brain.nav_version_seen = nav_version;
                    brain.path_goal = goal;
                    replanned = true;
                } else {
                    if brain.nav_version_seen != nav_version {
                        brain.planner.update_costs(nav_danger);
                        brain.nav_version_seen = nav_version;
                        replanned = true;
                    }

                    brain.planner.update_start(my_flat);
                }

                // O(1) when already converged
                brain.planner.plan();

                if replanned || tick_count >= brain.next_repath_tick {
                    let cells = brain.planner.extract_path(NAV_LEN);

                    if cells.is_empty() {
                        brain.path = vec![goal];
                        brain.next_repath_tick = tick_count + 5;
                    } else {
                        brain.path = string_pull(nav_danger, pos, &cells, goal, config);
                        brain.next_repath_tick = tick_count + config.bots.repath_interval;
                    }
                }
            } else {
                brain.path.clear();
            }
        } else {
            brain.path.clear();
        }

        // follow the waypoints unless the minimax is in close control
        if !brain.path.is_empty() && brain.flee_from.is_none() && (!combat || far_approach) {
            if pos.distance(brain.path[0]) < 60.0 {
                brain.path.remove(0);
            }

            if let Some(wp) = brain.path.first() {
                desired = *wp - pos;
            }
        }

        let move_dir = if hold {
            brain.move_vec = brain.move_vec * 0.6;
            None
        } else {
            if desired.length_squared() > 1e-6 {
                let jitter = (rnd.generate::<f32>() * 2.0 - 1.0) * config.bots.noise_steer_jitter;

                desired = Vec2::from_angle(jitter).rotate(desired);
            }

            let steer_angle =
                context_steer(entities, spatial_hash, nearby, pos, desired, ignore_obs)
                    .unwrap_or(0.0);
            let target = Vec2::from_angle(steer_angle);
            let smoothed = brain.move_vec.lerp(target, config.bots.steer_smooth);

            brain.move_vec = if smoothed.length_squared() < 0.01 {
                target
            } else {
                smoothed.normalize_or_zero()
            };

            Some(brain.move_vec.y.atan2(brain.move_vec.x))
        };

        let aim = if let Some(aim_point) = aim_at {
            let lead_vel = aim_vel * brain.skill;
            let base = intercept_angle(pos, aim_point, lead_vel, bullet_speed);

            base + brain.aim_phase.sin() * config.bots.noise_aim_wobble * (1.2 - brain.skill)
                + (rnd.generate::<f32>() * 2.0 - 1.0) * config.bots.noise_aim_scatter
        } else if let Some(a) = move_dir {
            a
        } else {
            0.0
        };

        brain.aim_phase += 0.55;

        if want_fire && rnd.generate::<u32>() % 100 < config.bots.noise_hesitate_chance {
            want_fire = false;
        }

        if let Some(t) = entities.tanks.get_mut(eid) {
            *t.aim = aim;
            *t.move_dir = move_dir;
            *t.auto_fire = want_fire;
        }

        if rnd.generate::<u32>() % 20 == 0 {
            upgrade_candidates.push((bot_id, eid));
        }
    }

    upgrade_candidates
}
