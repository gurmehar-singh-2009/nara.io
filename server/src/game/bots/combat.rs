use glam::Vec2;

use crate::fs::load_config::Config;

const MINIMAX_DEPTH: i32 = 3;
const MINIMAX_PLY_STEP: f32 = 0.35;

pub struct DuelCtx {
    pub me_speed: f32,
    pub me_dps: f32,
    pub me_max: f32,
    pub me_body: f32,
    pub foe_speed: f32,
    pub foe_dps: f32,
    pub foe_max: f32,
    pub foe_body: f32,
    pub bullet_range: f32,
    pub ideal_dist: f32,
    pub threats: Vec<(Vec2, Vec2, f32)>,
}

#[derive(Clone, Copy)]
pub struct DuelState {
    pub me: Vec2,
    pub me_hp: f32,
    pub foe: Vec2,
    pub foe_hp: f32,
    pub t: f32,
}

fn duel_advance(
    mut s: DuelState,
    ctx: &DuelCtx,
    actor_is_me: bool,
    move_dir: Option<Vec2>,
    fire: bool,
    config: &Config,
) -> DuelState {
    s.t += MINIMAX_PLY_STEP;

    {
        let (pos, speed) = if actor_is_me {
            (&mut s.me, ctx.me_speed)
        } else {
            (&mut s.foe, ctx.foe_speed)
        };

        if let Some(dir) = move_dir {
            *pos += dir * speed * MINIMAX_PLY_STEP;
            pos.x = pos.x.clamp(-config.world.map_bound, config.world.map_bound);
            pos.y = pos.y.clamp(-config.world.map_bound, config.world.map_bound);
        }
    }

    if actor_is_me {
        for (bp, bv, dmg) in &ctx.threats {
            let b = *bp + *bv * s.t;

            if b.distance(s.me) < 50.0 {
                s.me_hp -= *dmg;
            }
        }
    }

    let dist = s.me.distance(s.foe);
    if dist < config.bots.duel_ram_dist {
        s.me_hp -= ctx.foe_body * MINIMAX_PLY_STEP;
        s.foe_hp -= ctx.me_body * MINIMAX_PLY_STEP;
    }

    if fire && dist < ctx.bullet_range {
        let acc = ((ctx.bullet_range - dist) / ctx.bullet_range).clamp(0.0, 1.0);
        let acc = acc * acc;

        if actor_is_me {
            s.foe_hp -= ctx.me_dps * MINIMAX_PLY_STEP * acc;
        } else {
            s.me_hp -= ctx.foe_dps * MINIMAX_PLY_STEP * acc;
        }
    }

    s
}

fn duel_terminal(s: &DuelState) -> Option<f32> {
    if s.foe_hp <= 0.0 {
        Some(1.0e6 + s.me_hp.max(0.0))
    } else if s.me_hp <= 0.0 {
        Some(-1.0e6 - s.foe_hp.max(0.0))
    } else {
        None
    }
}

fn duel_eval(s: &DuelState, ctx: &DuelCtx, config: &Config) -> f32 {
    if let Some(v) = duel_terminal(s) {
        return v;
    }
    let dist = s.me.distance(s.foe);

    let mut v = 260.0 * (s.me_hp / ctx.me_max - s.foe_hp / ctx.foe_max);
    v -= 6.0 * (dist - ctx.ideal_dist).abs();

    for (bp, bv, dmg) in &ctx.threats {
        let b = *bp + *bv * s.t;
        let d = b.distance(s.me);

        if d < 150.0 {
            v -= (1.0 - d / 150.0) * *dmg * 1.5;
        }
    }

    // stay away from map corners
    let edge_x = (s.me.x.abs() - (config.world.map_bound - 300.0)).max(0.0);
    let edge_y = (s.me.y.abs() - (config.world.map_bound - 300.0)).max(0.0);
    v -= 0.06 * (edge_x + edge_y);

    v
}

fn duel_search(
    s: DuelState,
    ctx: &DuelCtx,
    depth: i32,
    mut alpha: f32,
    mut beta: f32,
    my_turn: bool,
    config: &Config,
) -> f32 {
    if let Some(v) = duel_terminal(&s) {
        return v;
    }

    if depth <= 0 {
        return duel_eval(&s, ctx, config);
    }

    let mut best = if my_turn { f32::MIN } else { f32::MAX };
    'actions: for mi in 0..9usize {
        let mv = if mi == 8 {
            None
        } else {
            Some(Vec2::from_angle(mi as f32 * std::f32::consts::FRAC_PI_4))
        };

        for &fire in &[false, true] {
            let ns = duel_advance(s, ctx, my_turn, mv, fire, config);
            let v = duel_search(ns, ctx, depth - 1, alpha, beta, !my_turn, config);

            if my_turn {
                if v > best {
                    best = v;
                }

                if best > alpha {
                    alpha = best;
                }

                if alpha >= beta {
                    break 'actions;
                }
            } else {
                if v < best {
                    best = v;
                }

                if best < beta {
                    beta = best;
                }

                if alpha >= beta {
                    break 'actions;
                }
            }
        }
    }

    best
}

pub fn minimax_decide(s: DuelState, ctx: &DuelCtx, config: &Config) -> (Option<f32>, bool) {
    let mut best_val = f32::MIN;
    let mut best = (None, false);
    let mut alpha = f32::MIN;

    for mi in 0..9usize {
        let (angle, mv) = if mi == 8 {
            (None, None)
        } else {
            let a = mi as f32 * std::f32::consts::FRAC_PI_4;
            (Some(a), Some(Vec2::from_angle(a)))
        };

        for &fire in &[false, true] {
            let ns = duel_advance(s, ctx, true, mv, fire, config);
            let v = duel_search(ns, ctx, MINIMAX_DEPTH - 1, alpha, f32::MAX, false, config);

            if v > best_val {
                best_val = v;
                best = (angle, fire);
            }

            if best_val > alpha {
                alpha = best_val;
            }
        }
    }

    best
}
