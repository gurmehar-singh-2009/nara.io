//! Handshake packet.
//! Used to establish the initial encryption.

use bitcode::{Decode, Encode};

use crate::junk_packet;

pub type HandShake = [u8; 96];

junk_packet! {
    pub struct HandshakePacket {
        pub handshake: HandShake,
    }
}
