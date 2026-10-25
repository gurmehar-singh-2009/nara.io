use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct TankInfoPacket {
        pub name: String,
        pub field_factor: f32,
        pub team: u8,
        pub stat_names: Vec<String>,
        pub stat_maxes: [u8; 8],
    }
}
