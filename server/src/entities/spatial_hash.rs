use crate::entities::entity::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashEntity {
    Entity(EntityId),
    Bullet(usize),
}

pub struct SpatialHash {
    buckets: Vec<Vec<HashEntity>>,
}

const CELL_SIZE: f32 = 32.;
const TABLE_SIZE: usize = 2048; // MUST be a power of 2.

impl SpatialHash {
    pub fn new() -> Self {
        Self {
            buckets: vec![Vec::new(); TABLE_SIZE],
        }
    }

    fn hash_cell(&self, cell_x: i32, cell_y: i32) -> usize {
        let p1 = (cell_x as u32).wrapping_mul(73_856_093);
        let p2 = (cell_y as u32).wrapping_mul(19_349_663);
        ((p1 ^ p2) as usize) & (TABLE_SIZE - 1)
    }

    pub fn insert(&mut self, entity: HashEntity, x: f32, y: f32) {
        let cell_x = (x / CELL_SIZE).floor() as i32;
        let cell_y = (y / CELL_SIZE).floor() as i32;
        let index = self.hash_cell(cell_x, cell_y);
        self.buckets[index].push(entity);
    }

    pub fn get_nearby(&self, x: f32, y: f32, radius: f32) -> Vec<HashEntity> {
        let min_x = ((x - radius) / CELL_SIZE).floor() as i32;
        let max_x = ((x + radius) / CELL_SIZE).floor() as i32;
        let min_y = ((y - radius) / CELL_SIZE).floor() as i32;
        let max_y = ((y + radius) / CELL_SIZE).floor() as i32;

        let mut nearby = Vec::with_capacity(32);
        for cx in min_x..=max_x {
            for cy in min_y..=max_y {
                let index = self.hash_cell(cx, cy);
                nearby.extend_from_slice(&self.buckets[index]);
            }
        }
        nearby
    }

    pub fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }
}
