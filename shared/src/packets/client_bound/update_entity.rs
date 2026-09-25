use bitcode::{Decode, Encode};

use crate::{junk_packet, packets::client_bound::EntityType};

junk_packet! {
    pub struct UpdateEntityPacket {
        pub data: Vec<UpdateEntityPacketData>,
    }
}

#[derive(Debug, Encode, Decode, Clone)]
pub struct UpdateEntityPacketData {
    pub id: u32,
    pub entity_type: EntityType,
    pub x: f32,
    pub y: f32,
    pub rot: f32,
    pub scale: f32,
    pub kind: u32,
    pub health: u32,
    pub max_health: u32,
}
