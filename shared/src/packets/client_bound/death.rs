use bitcode::{Decode, Encode};

use crate::{junk_packet, packets::client_bound::BarrelDef};

junk_packet! {
    pub struct DeathPacket {
        pub killer: String,
        pub killer_kind: u8,
        pub killer_tank: String,
        pub killer_team: u8,
        pub killer_sides: u32,
        pub killer_barrels: Vec<BarrelDef>,
        pub score: u32,
        pub level: u32,
        pub kills: u32,
        pub time_alive: u32,
        pub tank: String,
    }
}
