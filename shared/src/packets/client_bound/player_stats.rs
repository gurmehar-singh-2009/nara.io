use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct PlayerStatsPacket {
        pub level: u32,
        pub score: u32,
        pub xp: u32,
        pub xp_to_next: u32,
        pub health: u32,
        pub max_health: u32,
    }
}
