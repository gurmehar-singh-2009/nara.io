use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct MouseMovePacket {
        pub x: u16,
        pub y: u16,
        pub window_width: u16,
        pub window_height: u16,
    }
}
