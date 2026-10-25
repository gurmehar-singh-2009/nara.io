use bitcode::{Decode, Encode};

use crate::{
    junk_packet,
    packets::client_bound::{BarrelDef, EntityType, TurretDef},
};

junk_packet! {
    pub struct AddEntityPacket {
        pub id: u32,
        pub entity_type: EntityType,
        pub x: f32,
        pub y: f32,
        pub level: u32,
        pub name: String,
        pub is_entity_mine: bool,
        pub team: u8,
        pub barrels: Vec<BarrelDef>,
        pub turrets: Vec<TurretDef>,
    }
}
