//! Spawn request packet.
//! Used to request to spawn into the game.

use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct SpawnReqPacket {
        pub name: String,
    }
}
