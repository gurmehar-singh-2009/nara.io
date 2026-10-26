use glam::Vec2;

use crate::{entities::entity::EntityId, fs::load_config::DroneConfig, game::arena::Arena};

#[derive(Debug, Clone, Copy)]
pub enum DroneGoal {
    Chase(Vec2),
    Repel(Vec2),
    Idle { owner: Vec2, owner_radius: f32 },
}

pub struct DroneSpawn {
    pub position: Vec2,
    pub heading: f32,
    pub speed: f32,
    pub max_speed: f32,
    pub damage: u32,
    pub owner: EntityId,
    pub team: u8,
    pub barrel: usize,
    pub radius: f32,
    pub health: f32,
    pub recoil: u32,
    pub lifetime: Option<f32>,
    pub zone: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKey {
    Entity(EntityId),
    Drone(u32),
}

#[derive(Debug, Clone)]
pub struct Drones {
    positions: Vec<Vec2>,
    velocities: Vec<Vec2>,
    headings: Vec<f32>,
    resting: Vec<bool>,
    goals: Vec<DroneGoal>,
    ai_targets: Vec<Option<EntityId>>,
    max_speeds: Vec<f32>,
    damages: Vec<u32>,
    radii: Vec<f32>,
    healths: Vec<f32>,
    max_healths: Vec<f32>,
    recoils: Vec<u32>,
    lifetimes: Vec<Option<f32>>,
    hit_cooldowns: Vec<Vec<(HitKey, f32)>>,
    owners: Vec<EntityId>,
    teams: Vec<u8>,
    zones: Vec<Option<u32>>,
    barrels: Vec<usize>,
    net_ids: Vec<u32>,
    next_net_id: u32,
    sweep_order: Vec<usize>,
}

impl Drones {
    pub fn new() -> Self {
        Self {
            positions: vec![],
            velocities: vec![],
            headings: vec![],
            resting: vec![],
            goals: vec![],
            ai_targets: vec![],
            max_speeds: vec![],
            damages: vec![],
            radii: vec![],
            healths: vec![],
            max_healths: vec![],
            recoils: vec![],
            lifetimes: vec![],
            hit_cooldowns: vec![],
            owners: vec![],
            teams: vec![],
            zones: vec![],
            barrels: vec![],
            net_ids: vec![],
            next_net_id: 0x60000000,
            sweep_order: vec![],
        }
    }

    pub fn spawn(&mut self, d: DroneSpawn) -> u32 {
        let id = self.next_net_id;
        self.next_net_id = 0x60000000 | (self.next_net_id.wrapping_add(1) & 0x1fff_ffff);

        self.positions.push(d.position);
        self.velocities.push(Vec2::from_angle(d.heading) * d.speed);
        self.headings.push(d.heading);
        self.resting.push(false);
        self.goals.push(DroneGoal::Chase(d.position));
        self.ai_targets.push(None);
        self.max_speeds.push(d.max_speed);
        self.damages.push(d.damage);
        self.radii.push(d.radius);
        self.healths.push(d.health);
        self.max_healths.push(d.health);
        self.recoils.push(d.recoil);
        self.lifetimes.push(d.lifetime);
        self.hit_cooldowns.push(Vec::new());
        self.owners.push(d.owner);
        self.teams.push(d.team);
        self.zones.push(d.zone);
        self.barrels.push(d.barrel);
        self.net_ids.push(id);

        id
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn remove(&mut self, i: usize) {
        self.positions.swap_remove(i);
        self.velocities.swap_remove(i);
        self.headings.swap_remove(i);
        self.resting.swap_remove(i);
        self.goals.swap_remove(i);
        self.ai_targets.swap_remove(i);
        self.max_speeds.swap_remove(i);
        self.damages.swap_remove(i);
        self.radii.swap_remove(i);
        self.healths.swap_remove(i);
        self.max_healths.swap_remove(i);
        self.recoils.swap_remove(i);
        self.lifetimes.swap_remove(i);
        self.hit_cooldowns.swap_remove(i);
        self.owners.swap_remove(i);
        self.teams.swap_remove(i);
        self.zones.swap_remove(i);
        self.barrels.swap_remove(i);
        self.net_ids.swap_remove(i);
    }

    pub fn remove_owned_by(&mut self, owner: EntityId) {
        for i in (0..self.len()).rev() {
            if self.owners[i] == owner {
                self.remove(i);
            }
        }
    }

    pub fn remove_expired(&mut self) {
        for i in (0..self.len()).rev() {
            if self.is_expired(i) {
                self.remove(i);
            }
        }
    }

    pub fn count_for(&self, owner: EntityId, barrel: usize) -> usize {
        self.owners
            .iter()
            .zip(self.barrels.iter())
            .filter(|&(o, b)| *o == owner && *b == barrel)
            .count()
    }

    pub fn set_goal(&mut self, i: usize, goal: DroneGoal) {
        self.goals[i] = goal;
    }

    pub fn ai_target(&self, i: usize) -> Option<EntityId> {
        self.ai_targets[i]
    }

    pub fn set_ai_target(&mut self, i: usize, target: Option<EntityId>) {
        self.ai_targets[i] = target;
    }

    pub fn tick(&mut self, dt: f32, cfg: &DroneConfig) {
        let rest_sq = cfg.rest_radius * cfg.rest_radius;
        let damping = (-cfg.drag * dt).exp();

        for i in 0..self.len() {
            let pos = self.positions[i];

            let accel_mult = match self.goals[i] {
                DroneGoal::Chase(target) => {
                    self.resting[i] = false;
                    let to = target - pos;
                    if to.length_squared() > 1e-6 {
                        self.headings[i] = to.y.atan2(to.x);
                    }
                    1.0
                }

                DroneGoal::Repel(from) => {
                    self.resting[i] = false;
                    let away = pos - from;
                    if away.length_squared() > 1e-6 {
                        self.headings[i] = away.y.atan2(away.x);
                    }
                    1.0
                }

                DroneGoal::Idle {
                    owner,
                    owner_radius,
                } => {
                    let delta = pos - owner;
                    let unit_dist = delta.length_squared() / rest_sq;

                    if unit_dist <= 1.0 && self.resting[i] {
                        self.headings[i] +=
                            (cfg.rest_turn_base + cfg.rest_turn_per * unit_dist) * dt;
                        cfg.rest_accel_mult
                    } else {
                        let around = delta.y.atan2(delta.x) + std::f32::consts::FRAC_PI_2;
                        let orbit =
                            owner + Vec2::from_angle(around) * owner_radius * cfg.orbit_radii;
                        let to = orbit - pos;

                        self.headings[i] = to.y.atan2(to.x);
                        self.resting[i] =
                            to.length_squared() <= (cfg.rest_enter_radii * owner_radius).powi(2);

                        if unit_dist < 0.5 {
                            cfg.return_slow_mult
                        } else {
                            1.0
                        }
                    }
                }
            };

            let accel = self.max_speeds[i] * cfg.drag * accel_mult;
            self.velocities[i] += Vec2::from_angle(self.headings[i]) * accel * dt;
            self.velocities[i] *= damping;
            self.positions[i] += self.velocities[i] * dt;

            if let Some(life) = self.lifetimes[i].as_mut() {
                *life -= dt;
            }

            let cooldowns = &mut self.hit_cooldowns[i];
            for (_, t) in cooldowns.iter_mut() {
                *t -= dt;
            }
            cooldowns.retain(|(_, t)| *t > 0.0);
        }
    }

    pub fn resolve_pairs(&mut self, dt: f32, push: f32) -> Vec<(usize, usize)> {
        let n = self.len();
        let mut enemy_pairs = Vec::new();
        if n < 2 {
            return enemy_pairs;
        }

        let mut order = std::mem::take(&mut self.sweep_order);
        order.clear();
        order.extend(0..n);
        order.sort_unstable_by(|&a, &b| self.positions[a].x.total_cmp(&self.positions[b].x));

        let max_radius = self.radii.iter().copied().fold(0.0, f32::max);

        for a in 0..n {
            let i = order[a];
            let reach = self.positions[i].x + self.radii[i] + max_radius;

            for &j in &order[a + 1..] {
                if self.positions[j].x > reach {
                    break;
                }

                let same_owner = self.owners[i] == self.owners[j];
                if !same_owner && self.allied(i, j) {
                    continue;
                }

                let delta = self.positions[j] - self.positions[i];
                let min_dist = self.radii[i] + self.radii[j];
                let dist_sq = delta.length_squared();
                if dist_sq >= min_dist * min_dist {
                    continue;
                }

                let dist = dist_sq.sqrt();
                let dir = if dist > 1e-4 { delta / dist } else { Vec2::X };
                let impulse = dir * push * dt;

                self.velocities[i] -= impulse;
                self.velocities[j] += impulse;

                if !same_owner {
                    enemy_pairs.push((i, j));
                }
            }
        }

        self.sweep_order = order;
        enemy_pairs
    }

    fn allied(&self, i: usize, j: usize) -> bool {
        self.teams[i] != shared::packets::NO_TEAM && self.teams[i] == self.teams[j]
    }

    pub fn resolve_walls(&mut self, arena: &Arena) {
        let bound = arena.bound;

        for i in 0..self.len() {
            let pos = &mut self.positions[i];
            let vel = &mut self.velocities[i];

            if pos.x.abs() > bound {
                pos.x = pos.x.clamp(-bound, bound);
                vel.x = 0.0;
            }
            if pos.y.abs() > bound {
                pos.y = pos.y.clamp(-bound, bound);
                vel.y = 0.0;
            }

            if let Some((out, normal)) = arena.push_out(*pos, self.radii[i]) {
                *pos = out;
                let into = vel.dot(normal);
                if into < 0.0 {
                    *vel -= normal * into;
                }
            }
        }
    }

    pub fn is_expired(&self, i: usize) -> bool {
        self.healths[i] <= 0.0 || self.lifetimes[i].is_some_and(|l| l <= 0.0)
    }

    pub fn push(&mut self, i: usize, dir: Vec2, accel: f32, dt: f32) {
        self.velocities[i] += dir * accel * dt;
    }

    pub fn damage(&mut self, i: usize, amount: f32) -> bool {
        match self.healths.get_mut(i) {
            Some(h) => {
                *h -= amount.max(0.0);
                if *h <= 0.0 {
                    *h = 0.0;
                    true
                } else {
                    false
                }
            }
            None => true,
        }
    }

    pub fn can_hit(&self, i: usize, key: HitKey) -> bool {
        !self.hit_cooldowns[i].iter().any(|(k, _)| *k == key)
    }

    pub fn record_hit(&mut self, i: usize, key: HitKey, cooldown: f32) {
        self.hit_cooldowns[i].push((key, cooldown));
    }

    pub fn owner(&self, i: usize) -> EntityId {
        self.owners[i]
    }

    pub fn zone_at(&self, i: usize) -> Option<u32> {
        self.zones[i]
    }

    pub fn count_in_zone(&self, zone: u32) -> usize {
        self.zones.iter().filter(|z| **z == Some(zone)).count()
    }

    pub fn team_at(&self, i: usize) -> u8 {
        self.teams[i]
    }

    pub fn position(&self, i: usize) -> Vec2 {
        self.positions[i]
    }

    pub fn barrel_at(&self, i: usize) -> usize {
        self.barrels[i]
    }

    pub fn net_id(&self, i: usize) -> u32 {
        self.net_ids[i]
    }

    pub fn health(&self, i: usize) -> f32 {
        self.healths[i]
    }

    pub fn damage_at(&self, i: usize) -> u32 {
        self.damages[i]
    }

    pub fn iter_indexed(&self) -> impl Iterator<Item = (usize, Vec2, u32, f32, EntityId)> + '_ {
        (0..self.len()).map(|i| {
            (
                i,
                self.positions[i],
                self.damages[i],
                self.radii[i],
                self.owners[i],
            )
        })
    }

    pub fn iter_net(&self) -> impl Iterator<Item = (u32, Vec2, f32, f32, u32, f32, f32, u8)> + '_ {
        (0..self.len()).map(|i| {
            (
                self.net_ids[i],
                self.positions[i],
                self.headings[i],
                self.radii[i],
                self.recoils[i],
                self.healths[i],
                self.max_healths[i],
                self.teams[i],
            )
        })
    }
}
