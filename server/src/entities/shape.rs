use glam::Vec2;

use crate::entities::entity::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Square,
    Triangle,
    Pentagon,
}

#[derive(Debug, Clone)]
pub struct Shapes {
    ids: Vec<EntityId>,
    kinds: Vec<ShapeKind>,
    rotations: Vec<f32>,
    rotation_speeds: Vec<f32>,
    xp_rewards: Vec<u32>,

    pub centers: Vec<Vec2>,
    orbit_radii: Vec<f32>,
    orbit_angles: Vec<f32>,
    orbit_speeds: Vec<f32>,

    sparse: Vec<Option<usize>>,
}

pub struct ShapeRef<'a> {
    pub kind: &'a ShapeKind,
    pub rotation: &'a f32,
    pub rotation_speed: &'a f32,
    pub xp_reward: &'a u32,
}

#[allow(dead_code)]
pub struct ShapeMut<'a> {
    pub kind: &'a mut ShapeKind,
    pub rotation: &'a mut f32,
    pub rotation_speed: &'a mut f32,
    pub xp_reward: &'a mut u32,
}

impl Shapes {
    pub fn new(max_shapes: usize) -> Self {
        Self {
            ids: Vec::with_capacity(max_shapes),
            kinds: Vec::with_capacity(max_shapes),
            rotations: Vec::with_capacity(max_shapes),
            rotation_speeds: Vec::with_capacity(max_shapes),
            xp_rewards: Vec::with_capacity(max_shapes),
            centers: Vec::with_capacity(max_shapes),
            orbit_radii: Vec::with_capacity(max_shapes),
            orbit_angles: Vec::with_capacity(max_shapes),
            orbit_speeds: Vec::with_capacity(max_shapes),
            sparse: Vec::with_capacity(max_shapes),
        }
    }

    fn ensure_sparse_capacity(&mut self, index: usize) {
        if index >= self.sparse.len() {
            self.sparse.resize(index + 1, None);
        }
    }

    pub fn insert(
        &mut self,
        id: EntityId,
        kind: ShapeKind,
        rotation_speed: f32,
        xp_reward: u32,
        center: Vec2,
        orbit_radius: f32,
        orbit_angle: f32,
        orbit_speed: f32,
    ) {
        self.ensure_sparse_capacity(id.index);
        if self.sparse[id.index].is_some() {
            return;
        }
        let slot = self.ids.len();
        self.ids.push(id);
        self.kinds.push(kind);
        self.rotations.push(1.0);
        self.rotation_speeds.push(rotation_speed);
        self.xp_rewards.push(xp_reward);
        self.centers.push(center);
        self.orbit_radii.push(orbit_radius);
        self.orbit_angles.push(orbit_angle);
        self.orbit_speeds.push(orbit_speed);
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
        self.kinds.swap(slot, last);
        self.rotations.swap(slot, last);
        self.rotation_speeds.swap(slot, last);
        self.xp_rewards.swap(slot, last);
        self.centers.swap(slot, last);
        self.orbit_radii.swap(slot, last);
        self.orbit_angles.swap(slot, last);
        self.orbit_speeds.swap(slot, last);

        self.ids.pop();
        self.kinds.pop();
        self.rotations.pop();
        self.rotation_speeds.pop();
        self.xp_rewards.pop();
        self.centers.pop();
        self.orbit_radii.pop();
        self.orbit_angles.pop();
        self.orbit_speeds.pop();

        self.sparse[id.index] = None;
        if slot != last {
            let moved_id = self.ids[slot];
            self.sparse[moved_id.index] = Some(slot);
        }
        true
    }

    pub fn get(&self, id: EntityId) -> Option<ShapeRef<'_>> {
        let slot = self.sparse.get(id.index).copied().flatten()?;
        if self.ids.get(slot).copied() != Some(id) {
            return None;
        }
        Some(ShapeRef {
            kind: &self.kinds[slot],
            rotation: &self.rotations[slot],
            rotation_speed: &self.rotation_speeds[slot],
            xp_reward: &self.xp_rewards[slot],
        })
    }

    #[allow(dead_code)]
    pub fn get_mut(&mut self, id: EntityId) -> Option<ShapeMut<'_>> {
        let slot = self.sparse.get(id.index).copied().flatten()?;
        if self.ids.get(slot).copied() != Some(id) {
            return None;
        }
        Some(ShapeMut {
            kind: &mut self.kinds[slot],
            rotation: &mut self.rotations[slot],
            rotation_speed: &mut self.rotation_speeds[slot],
            xp_reward: &mut self.xp_rewards[slot],
        })
    }

    pub fn tick(&mut self, dt: f32, positions: &mut [glam::Vec2]) {
        for i in 0..self.ids.len() {
            self.rotations[i] += self.rotation_speeds[i] * dt;
            self.orbit_angles[i] += self.orbit_speeds[i] * dt;

            let center = self.centers[i];
            let radius = self.orbit_radii[i];
            let angle = self.orbit_angles[i];

            let target_pos = center + Vec2::new(angle.cos(), angle.sin()) * radius;

            let entity_index = self.ids[i].index;
            if entity_index < positions.len() {
                positions[entity_index] = target_pos;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn push_center(&mut self, id: EntityId, push: Vec2) {
        if let Some(slot) = self.sparse.get(id.index).copied().flatten() {
            if self.ids.get(slot).copied() == Some(id) {
                self.centers[slot] += push;
            }
        }
    }
}
