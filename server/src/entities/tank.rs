use std::sync::Arc;

use shared::packets::{NO_TEAM, client_bound::BarrelDef};

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
    scores: Vec<u32>,
    kills: Vec<u32>,
    aim: Vec<f32>,
    movement_dirs: Vec<Option<f32>>,
    auto_fire: Vec<bool>,
    auto_spin: Vec<bool>,
    repel: Vec<bool>,
    teams: Vec<u8>,
    levels: Vec<u32>,
    tank_types: Vec<Arc<Tank>>,
    max_healths: Vec<u32>,
    reload_times: Vec<f32>,
    barrels: Vec<Vec<BarrelDef>>,
    barrel_timers: Vec<Vec<f32>>,
    stat_levels: Vec<[u8; 8]>,
    stat_points: Vec<u32>,
    upgrade_offered: Vec<Option<u32>>,
    sparse: Vec<Option<usize>>,
}

#[allow(dead_code)]
pub struct TankRef<'a> {
    pub name: &'a str,
    pub xp: &'a Xp,
    pub score: &'a u32,
    pub kills: &'a u32,
    pub aim: &'a f32,
    pub move_dir: &'a Option<f32>,
    pub auto_fire: &'a bool,
    pub auto_spin: &'a bool,
    pub repel: &'a bool,
    pub team: &'a u8,
    pub level: &'a u32,
    pub tank_type: &'a Tank,
    pub max_health: &'a u32,
    pub reload_time: &'a f32,
    pub barrels: &'a Vec<BarrelDef>,
    pub stat_levels: &'a [u8; 8],
    pub stat_points: &'a u32,
}

#[allow(dead_code)]
pub struct TankMut<'a> {
    pub name: &'a mut String,
    pub xp: &'a mut Xp,
    pub score: &'a mut u32,
    pub kills: &'a mut u32,
    pub aim: &'a mut f32,
    pub move_dir: &'a mut Option<f32>,
    pub auto_fire: &'a mut bool,
    pub auto_spin: &'a mut bool,
    pub repel: &'a mut bool,
    pub team: &'a mut u8,
    pub level: &'a mut u32,
    // we dont need tank type as mutable EVER
    pub tank_type: &'a Tank,
    pub max_health: &'a mut u32,
    pub reload_time: &'a mut f32,
    pub barrels: &'a mut Vec<BarrelDef>,
    pub barrel_timers: &'a mut Vec<f32>,
    pub stat_levels: &'a mut [u8; 8],
    pub stat_points: &'a mut u32,
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
            scores: Vec::with_capacity(max_tanks),
            kills: Vec::with_capacity(max_tanks),
            aim: Vec::with_capacity(max_tanks),
            movement_dirs: Vec::with_capacity(max_tanks),
            auto_fire: Vec::with_capacity(max_tanks),
            auto_spin: Vec::with_capacity(max_tanks),
            repel: Vec::with_capacity(max_tanks),
            teams: Vec::with_capacity(max_tanks),
            levels: Vec::with_capacity(max_tanks),
            tank_types: Vec::with_capacity(max_tanks),
            max_healths: Vec::with_capacity(max_tanks),
            reload_times: Vec::with_capacity(max_tanks),
            barrels: Vec::with_capacity(max_tanks),
            barrel_timers: Vec::with_capacity(max_tanks),
            stat_levels: Vec::with_capacity(max_tanks),
            stat_points: Vec::with_capacity(max_tanks),
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
        self.scores.push(0);
        self.kills.push(0);
        self.aim.push(0.0);
        self.movement_dirs.push(None);
        self.auto_fire.push(false);
        self.auto_spin.push(false);
        self.repel.push(false);
        self.teams.push(NO_TEAM);
        self.levels.push(1);
        self.max_healths.push(basic_tank.max_health);
        self.reload_times.push(0.5);
        self.tank_types.push(Arc::new(basic_tank));
        self.barrels.push(basic_barrels);
        self.barrel_timers.push(timers);
        self.stat_levels.push([0; 8]);
        self.stat_points.push(0);
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
        self.scores.swap(slot, last);
        self.kills.swap(slot, last);
        self.aim.swap(slot, last);
        self.movement_dirs.swap(slot, last);
        self.auto_fire.swap(slot, last);
        self.auto_spin.swap(slot, last);
        self.repel.swap(slot, last);
        self.teams.swap(slot, last);
        self.levels.swap(slot, last);
        self.tank_types.swap(slot, last);
        self.max_healths.swap(slot, last);
        self.reload_times.swap(slot, last);
        self.barrels.swap(slot, last);
        self.barrel_timers.swap(slot, last);
        self.stat_levels.swap(slot, last);
        self.stat_points.swap(slot, last);
        self.upgrade_offered.swap(slot, last);

        self.ids.pop();
        self.names.pop();
        self.xp.pop();
        self.scores.pop();
        self.kills.pop();
        self.aim.pop();
        self.movement_dirs.pop();
        self.auto_fire.pop();
        self.auto_spin.pop();
        self.repel.pop();
        self.teams.pop();
        self.levels.pop();
        self.tank_types.pop();
        self.max_healths.pop();
        self.reload_times.pop();
        self.barrels.pop();
        self.barrel_timers.pop();
        self.stat_levels.pop();
        self.stat_points.pop();
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
            score: &self.scores[slot],
            kills: &self.kills[slot],
            aim: &self.aim[slot],
            move_dir: &self.movement_dirs[slot],
            auto_fire: &self.auto_fire[slot],
            auto_spin: &self.auto_spin[slot],
            repel: &self.repel[slot],
            team: &self.teams[slot],
            level: &self.levels[slot],
            tank_type: &self.tank_types[slot],
            max_health: &self.max_healths[slot],
            reload_time: &self.reload_times[slot],
            barrels: &self.barrels[slot],
            stat_levels: &self.stat_levels[slot],
            stat_points: &self.stat_points[slot],
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
            score: &mut self.scores[slot],
            kills: &mut self.kills[slot],
            aim: &mut self.aim[slot],
            move_dir: &mut self.movement_dirs[slot],
            auto_fire: &mut self.auto_fire[slot],
            auto_spin: &mut self.auto_spin[slot],
            repel: &mut self.repel[slot],
            team: &mut self.teams[slot],
            level: &mut self.levels[slot],
            tank_type: &self.tank_types[slot],
            max_health: &mut self.max_healths[slot],
            reload_time: &mut self.reload_times[slot],
            barrels: &mut self.barrels[slot],
            barrel_timers: &mut self.barrel_timers[slot],
            stat_levels: &mut self.stat_levels[slot],
            stat_points: &mut self.stat_points[slot],
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

    pub fn team_of(&self, id: EntityId) -> u8 {
        self.get(id).map(|t| *t.team).unwrap_or(NO_TEAM)
    }

    pub fn allied(&self, a: EntityId, b: EntityId) -> bool {
        let team = self.team_of(a);
        team != NO_TEAM && team == self.team_of(b)
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
        self.barrels[slot] = barrel_defs(def);
        self.barrel_timers[slot] = initial_timers(self.reload_times[slot], def);
        self.upgrade_offered[slot] = None;

        for stat in 0..8 {
            let max = stat_max(def, stat, u8::MAX);
            let levels = &mut self.stat_levels[slot];

            if levels[stat] > max {
                self.stat_points[slot] += (levels[stat] - max) as u32;
                levels[stat] = max;
            }
        }

        true
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

const DEFAULT_STAT_NAMES: [&str; 8] = [
    "Health Regen",
    "Max Health",
    "Body Damage",
    "Bullet Speed",
    "Bullet Penetration",
    "Bullet Damage",
    "Reload",
    "Movement Speed",
];

pub fn stat_max(def: &Tank, stat: usize, fallback: u8) -> u8 {
    def.stats
        .get(7 - stat.min(7))
        .map(|s| s.max.min(u8::MAX as u32) as u8)
        .unwrap_or(fallback)
}

pub fn stat_names(def: &Tank) -> Vec<String> {
    (0..8)
        .map(|stat| {
            def.stats
                .get(7 - stat)
                .map(|s| s.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| DEFAULT_STAT_NAMES[stat].to_string())
        })
        .collect()
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
            is_trapezoid: b.is_trapezoid,
            trap: b.bullet.bullet_type == "trap",
        })
        .collect()
}
