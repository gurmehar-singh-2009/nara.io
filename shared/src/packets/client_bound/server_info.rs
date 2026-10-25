use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct ServerInfoPacket {
        pub load: f32,
        pub players: u32,
        pub bots: u32,
        pub shapes: u32,
    }
}
