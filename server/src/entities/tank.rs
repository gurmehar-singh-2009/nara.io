use std::sync::Arc;

use shared::packets::client_bound::BarrelDef;

use crate::{
    entities::entity::EntityId,
    fs::tank_defs::{Tank, TankTree},
};

pub type Xp = (u32, u32);

#[derive(Debug, Clone)]
pub struct Tanks {
    ids: Vec<EntityId>,
    names: Vec<String>,
    xp: Vec<Xp>,
    aim: Vec<f32>,
    movement_dirs: Vec<Option<f32>>,
    auto_fire: Vec<bool>,
    levels: Vec<u32>,
    tank_types: Vec<Arc<Tank>>,
    max_healths: Vec<u32>,
    bullet_damages: Vec<u32>,
    bullet_speeds: Vec<f32>,
    reload_times: Vec<f32>,
    barrels: Vec<Vec<BarrelDef>>,
    barrel_timers: Vec<Vec<f32>>,
    upgrade_offered: Vec<Option<u32>>,
    sparse: Vec<Option<usize>>,
}

pub struct TankRef<'a> {
    pub name: &'a str,
    pub xp: &'a Xp,
    pub aim: &'a f32,
    pub move_dir: &'a Option<f32>,
    pub auto_fire: &'a bool,
    pub level: &'a u32,
    pub tank_type: &'a Tank,
    pub max_health: &'a u32,
    pub bullet_damage: &'a u32,
    pub bullet_speed: &'a f32,
    pub reload_time: &'a f32,
    pub barrels: &'a Vec<BarrelDef>,
}

pub struct TankMut<'a> {
    pub name: &'a mut String,
    pub xp: &'a mut Xp,
    pub aim: &'a mut f32,
    pub move_dir: &'a mut Option<f32>,
    pub auto_fire: &'a mut bool,
    // pub reload_timer: &'a mut f32,
    pub level: &'a mut u32,
    // we dont need tank type as mutable EVER
    pub tank_type: &'a Tank,
    pub max_health: &'a mut u32,
    pub bullet_damage: &'a mut u32,
    pub bullet_speed: &'a mut f32,
    pub reload_time: &'a mut f32,
    pub barrels: &'a mut Vec<BarrelDef>,
    pub barrel_timers: &'a mut Vec<f32>,
}

fn initial_timers(reload_time: f32, def: &Tank) -> Vec<f32> {
    def.barrels
        .iter()
        .map(|b| ((reload_time * b.reload).max(0.05) * b.delay).max(0.0))
        .collect()
}

impl Tanks {
    pub fn new(max_tanks: usize) -> Self {
        Self {
            ids: Vec::with_capacity(max_tanks),
            names: Vec::with_capacity(max_tanks),
            xp: Vec::with_capacity(max_tanks),
            aim: Vec::with_capacity(max_tanks),
            movement_dirs: Vec::with_capacity(max_tanks),
            auto_fire: Vec::with_capacity(max_tanks),
            levels: Vec::with_capacity(max_tanks),
            tank_types: Vec::with_capacity(max_tanks),
            max_healths: Vec::with_capacity(max_tanks),
            bullet_damages: Vec::with_capacity(max_tanks),
            bullet_speeds: Vec::with_capacity(max_tanks),
            reload_times: Vec::with_capacity(max_tanks),
            barrels: Vec::with_capacity(max_tanks),
            barrel_timers: Vec::with_capacity(max_tanks),
            upgrade_offered: Vec::with_capacity(max_tanks),
            sparse: Vec::with_capacity(max_tanks),
        }
    }

    fn ensure_sparse_capacity(&mut self, index: usize) {
        if index >= self.sparse.len() {
            self.sparse.resize(index + 1, None);
        }
    }

    pub fn insert(&mut self, id: EntityId, name: String, tank_tree: Arc<TankTree>) {
        self.ensure_sparse_capacity(id.index);

        if self.sparse[id.index].is_some() {
            return;
        }

        let slot = self.ids.len();
        let basic_tank = tank_tree[0][0].clone();
        let basic_barrels = barrel_defs(&basic_tank);
        let timers = initial_timers(0.5, &basic_tank);

        self.ids.push(id);
        self.names.push(name);
        self.xp.push((0, 250));
        self.aim.push(0.0);
        self.movement_dirs.push(None);
        self.auto_fire.push(false);
        self.levels.push(1);
        self.max_healths.push(basic_tank.max_health);
        self.bullet_damages.push(2);
        self.bullet_speeds.push(100.0);
        self.reload_times.push(0.5);
        self.tank_types.push(Arc::new(basic_tank));
        self.barrels.push(basic_barrels);
        self.barrel_timers.push(timers);
        self.upgrade_offered.push(None);

        self.sparse[id.index] = Some(slot);
    }

    pub fn remove(&mut self, id: EntityId) -> bool {
        let Some(slot) = self.sparse.get(id.index).copied().flatten() else {
            return false;
        };

        if self.ids.get(slot).copied() != Some(id) {
            return false;
        }

        let last = self.ids.len() - 1;

        self.ids.swap(slot, last);
        self.names.swap(slot, last);
        self.xp.swap(slot, last);
        self.aim.swap(slot, last);
        self.movement_dirs.swap(slot, last);
        self.auto_fire.swap(slot, last);
        self.levels.swap(slot, last);
        self.tank_types.swap(slot, last);
        self.max_healths.swap(slot, last);
        self.bullet_damages.swap(slot, last);
        self.bullet_speeds.swap(slot, last);
        self.reload_times.swap(slot, last);
        self.barrels.swap(slot, last);
        self.barrel_timers.swap(slot, last);
        self.upgrade_offered.swap(slot, last);

        self.ids.pop();
        self.names.pop();
        self.xp.pop();
        self.aim.pop();
        self.movement_dirs.pop();
        self.auto_fire.pop();
        self.levels.pop();
        self.tank_types.pop();
        self.max_healths.pop();
        self.bullet_damages.pop();
        self.bullet_speeds.pop();
        self.reload_times.pop();
        self.barrels.pop();
        self.barrel_timers.pop();
        self.upgrade_offered.pop();

        self.sparse[id.index] = None;

        if slot != last {
            let moved_id = self.ids[slot];
            self.sparse[moved_id.index] = Some(slot);
        }

        true
    }

    pub fn get(&self, id: EntityId) -> Option<TankRef<'_>> {
        let slot = self.sparse.get(id.index).copied().flatten()?;

        if self.ids.get(slot).copied() != Some(id) {
            return None;
        }

        Some(TankRef {
            name: &self.names[slot],
            xp: &self.xp[slot],
            aim: &self.aim[slot],
            move_dir: &self.movement_dirs[slot],
            auto_fire: &self.auto_fire[slot],
            // reload_timer: &self.reload_timers[slot],
            level: &self.levels[slot],
            tank_type: &self.tank_types[slot],
            max_health: &self.max_healths[slot],
            bullet_damage: &self.bullet_damages[slot],
            bullet_speed: &self.bullet_speeds[slot],
            reload_time: &self.reload_times[slot],
            barrels: &self.barrels[slot],
        })
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<TankMut<'_>> {
        let slot = self.sparse.get(id.index).copied().flatten()?;

        if self.ids.get(slot).copied() != Some(id) {
            return None;
        }

        Some(TankMut {
            name: &mut self.names[slot],
            xp: &mut self.xp[slot],
            aim: &mut self.aim[slot],
            move_dir: &mut self.movement_dirs[slot],
            auto_fire: &mut self.auto_fire[slot],
            // reload_timer: &mut self.reload_timers[slot],
            level: &mut self.levels[slot],
            tank_type: &self.tank_types[slot],
            max_health: &mut self.max_healths[slot],
            bullet_damage: &mut self.bullet_damages[slot],
            bullet_speed: &mut self.bullet_speeds[slot],
            reload_time: &mut self.reload_times[slot],
            barrels: &mut self.barrels[slot],
            barrel_timers: &mut self.barrel_timers[slot],
        })
    }

    pub fn set_movement_dir(&mut self, id: EntityId, dir: Option<f32>) -> bool {
        let Some(slot) = self.sparse.get(id.index).copied().flatten() else {
            return false;
        };

        if self.ids.get(slot).copied() != Some(id) {
            return false;
        }

        self.movement_dirs[slot] = dir;

        true
    }

    pub fn set_auto_fire(&mut self, id: EntityId, enabled: bool) -> bool {
        let Some(slot) = self.sparse.get(id.index).copied().flatten() else {
            return false;
        };

        if self.ids.get(slot).copied() != Some(id) {
            return false;
        }

        self.auto_fire[slot] = enabled;

        true
    }

    pub fn offered_for(&self, id: EntityId) -> Option<u32> {
        let Some(slot) = self.sparse.get(id.index).copied().flatten() else {
            return None;
        };

        if self.ids.get(slot).copied() != Some(id) {
            return None;
        }

        self.upgrade_offered[slot]
    }

    pub fn set_offered(&mut self, id: EntityId, tank_id: u32) -> bool {
        let Some(slot) = self.sparse.get(id.index).copied().flatten() else {
            return false;
        };

        if self.ids.get(slot).copied() != Some(id) {
            return false;
        }

        self.upgrade_offered[slot] = Some(tank_id);

        true
    }

    pub fn reset_offers(&mut self) {
        for offered in self.upgrade_offered.iter_mut() {
            *offered = None;
        }
    }

    pub fn apply_def(&mut self, id: EntityId, def: &Tank) -> bool {
        let Some(slot) = self.sparse.get(id.index).copied().flatten() else {
            return false;
        };

        if self.ids.get(slot).copied() != Some(id) {
            return false;
        }

        self.tank_types[slot] = Arc::new(def.clone());
        self.max_healths[slot] = def.max_health;
        self.barrels[slot] = barrel_defs(def);
        self.barrel_timers[slot] = initial_timers(self.reload_times[slot], def);
        self.upgrade_offered[slot] = None;

        true
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

pub fn barrel_defs(def: &Tank) -> Vec<BarrelDef> {
    def.barrels
        .iter()
        .map(|b| BarrelDef {
            x: b.x,
            y: b.y,
            angle: b.angle,
            width: b.width,
            length: b.length,
        })
        .collect()
}
