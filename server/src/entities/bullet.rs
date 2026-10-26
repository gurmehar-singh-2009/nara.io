#![allow(dead_code)] // figure out later

use glam::Vec2;

use crate::entities::entity::EntityId;

#[derive(Debug, Clone, Copy)]
pub struct DeadBullet {
    pub net_id: u32,
    pub origin: Option<Vec2>,
    pub pos: Vec2,
    pub radius: f32,
    pub recoil: u32,
    pub team: u8,
    pub trap: bool,
}

#[derive(Debug, Clone)]
pub struct Bullets {
    positions: Vec<Vec2>,
    velocities: Vec<Vec2>,
    lifetimes: Vec<f32>,
    damages: Vec<u32>,
    radii: Vec<f32>,
    healths: Vec<f32>,
    recoils: Vec<u32>,
    hit_targets: Vec<Vec<EntityId>>,
    owners: Vec<EntityId>,
    teams: Vec<u8>,
    traps: Vec<bool>,
    origins: Vec<Vec2>,
    announced: Vec<bool>,
    net_ids: Vec<u32>,
    next_net_id: u32,
    graveyard: Vec<DeadBullet>,
}

impl Bullets {
    pub fn new() -> Self {
        Self {
            positions: vec![],
            velocities: vec![],
            lifetimes: vec![],
            damages: vec![],
            radii: vec![],
            healths: vec![],
            recoils: vec![],
            hit_targets: vec![],
            owners: vec![],
            teams: vec![],
            traps: vec![],
            origins: vec![],
            announced: vec![],
            net_ids: vec![],
            next_net_id: 0x40000000,
            graveyard: vec![],
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        position: Vec2,
        velocity: Vec2,
        damage: u32,
        lifetime: f32,
        owner: EntityId,
        radius: f32,
        health: f32,
        recoil: u32,
        team: u8,
        trap: bool,
    ) -> u32 {
        let id = self.next_net_id;
        self.next_net_id += 1;

        self.positions.push(position);
        self.velocities.push(velocity);
        self.lifetimes.push(lifetime);
        self.damages.push(damage);
        self.radii.push(radius);
        self.healths.push(health);
        self.recoils.push(recoil);
        self.hit_targets.push(Vec::new());
        self.owners.push(owner);
        self.teams.push(team);
        self.traps.push(trap);
        self.origins.push(position);
        self.announced.push(false);
        self.net_ids.push(id);

        id
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn remove(&mut self, i: usize) {
        self.graveyard.push(DeadBullet {
            net_id: self.net_ids[i],
            origin: (!self.announced[i]).then_some(self.origins[i]),
            pos: self.positions[i],
            radius: self.radii[i],
            recoil: self.recoils[i],
            team: self.teams[i],
            trap: self.traps[i],
        });

        self.positions.swap_remove(i);
        self.velocities.swap_remove(i);
        self.lifetimes.swap_remove(i);
        self.damages.swap_remove(i);
        self.radii.swap_remove(i);
        self.healths.swap_remove(i);
        self.recoils.swap_remove(i);
        self.hit_targets.swap_remove(i);
        self.owners.swap_remove(i);
        self.teams.swap_remove(i);
        self.traps.swap_remove(i);
        self.origins.swap_remove(i);
        self.announced.swap_remove(i);
        self.net_ids.swap_remove(i);
    }

    pub fn tick(&mut self, dt: f32, trap_drag: f32) {
        let damping = (-trap_drag * dt).exp();

        for i in 0..self.positions.len() {
            if self.traps[i] {
                self.velocities[i] *= damping;
            }

            self.positions[i] += self.velocities[i] * dt;
            self.lifetimes[i] -= dt;
        }

        let mut i = 0;
        while i < self.lifetimes.len() {
            if self.lifetimes[i] <= 0.0 {
                self.remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn health(&self, i: usize) -> f32 {
        self.healths.get(i).copied().unwrap_or(0.0)
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

    pub fn has_hit(&self, i: usize, target: EntityId) -> bool {
        self.hit_targets
            .get(i)
            .is_some_and(|list| list.contains(&target))
    }

    pub fn record_hit(&mut self, i: usize, target: EntityId) {
        if let Some(list) = self.hit_targets.get_mut(i) {
            if !list.contains(&target) {
                list.push(target);
            }
        }
    }

    pub fn position(&self, i: usize) -> Vec2 {
        self.positions.get(i).copied().unwrap_or(Vec2::ZERO)
    }

    pub fn velocity_at(&self, i: usize) -> Vec2 {
        self.velocities.get(i).copied().unwrap_or(Vec2::ZERO)
    }

    pub fn damage_at(&self, i: usize) -> u32 {
        self.damages.get(i).copied().unwrap_or(0)
    }

    pub fn radius_at(&self, i: usize) -> f32 {
        self.radii.get(i).copied().unwrap_or(0.0)
    }

    pub fn owner(&self, i: usize) -> EntityId {
        self.owners.get(i).copied().unwrap_or(EntityId {
            index: usize::MAX,
            generation: 0,
        })
    }

    pub fn fresh_origins(&self) -> Vec<Option<Vec2>> {
        self.origins
            .iter()
            .zip(self.announced.iter())
            .map(|(&origin, &announced)| (!announced).then_some(origin))
            .collect()
    }

    pub fn mark_announced(&mut self) {
        self.announced.fill(true);
    }

    pub fn take_graveyard(&mut self) -> Vec<DeadBullet> {
        std::mem::take(&mut self.graveyard)
    }

    pub fn team_at(&self, i: usize) -> u8 {
        self.teams
            .get(i)
            .copied()
            .unwrap_or(shared::packets::NO_TEAM)
    }

    pub fn iter_indexed(&self) -> impl Iterator<Item = (usize, &Vec2, &u32, f32, &EntityId)> {
        self.positions
            .iter()
            .zip(self.damages.iter())
            .zip(self.radii.iter())
            .zip(self.owners.iter())
            .enumerate()
            .map(|(i, (((pos, dmg), radius), owner))| (i, pos, dmg, *radius, owner))
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &Vec2, f32, u32, u8, bool)> {
        self.positions
            .iter()
            .zip(self.net_ids.iter())
            .zip(self.radii.iter())
            .zip(self.recoils.iter())
            .zip(self.teams.iter())
            .zip(self.traps.iter())
            .map(|(((((pos, id), radius), recoil), team), trap)| {
                (*id, pos, *radius, *recoil, *team, *trap)
            })
    }
}
