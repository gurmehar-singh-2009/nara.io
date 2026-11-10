//! The aim packet.
//! This allows you to aim.

use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct AimPacket {
        pub dir: f32,
    }
}
