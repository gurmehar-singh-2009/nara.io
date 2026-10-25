use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct SpectatePacket {
        pub x: f32,
        pub y: f32,
    }
}
