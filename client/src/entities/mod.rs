use crate::render::buffers::EntityInstance;

pub mod bullet;
pub mod drone;
pub mod shape;
pub mod tank;

pub trait Entity {
    fn get_render_instances(&self) -> Vec<EntityInstance>;
}
