//! Repel packet.
//! This is used to repel the drones from your mouse position.

use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct RepelPacket {
        pub enabled: bool,
    }
}
