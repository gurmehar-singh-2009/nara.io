#![allow(dead_code)] // figure out later

use glam::Vec2;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::entities::entity::EntityId;

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
    net_ids: Vec<u32>,
    next_net_id: u32,
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
            net_ids: vec![],
            next_net_id: 0x40000000,
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
        self.positions.swap_remove(i);
        self.velocities.swap_remove(i);
        self.lifetimes.swap_remove(i);
        self.damages.swap_remove(i);
        self.radii.swap_remove(i);
        self.healths.swap_remove(i);
        self.recoils.swap_remove(i);
        self.hit_targets.swap_remove(i);
        self.owners.swap_remove(i);
        self.net_ids.swap_remove(i);
    }

    pub fn tick(&mut self, dt: f32) {
        (
            &mut self.positions,
            &mut self.velocities,
            &mut self.lifetimes,
        )
            .into_par_iter()
            .for_each(|(pos, vel, life)| {
                *pos += *vel * dt;
                *life -= dt;
            });

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

    pub fn radius_at(&self, i: usize) -> f32 {
        self.radii.get(i).copied().unwrap_or(0.0)
    }

    pub fn owner(&self, i: usize) -> EntityId {
        self.owners.get(i).copied().unwrap_or(EntityId {
            index: usize::MAX,
            generation: 0,
        })
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

    pub fn iter(&self) -> impl Iterator<Item = (u32, &Vec2, f32, u32)> {
        self.positions
            .iter()
            .zip(self.net_ids.iter())
            .zip(self.radii.iter())
            .zip(self.recoils.iter())
            .map(|(((pos, id), radius), recoil)| (*id, pos, *radius, *recoil))
    }
}
