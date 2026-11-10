//! The autofire packet.
//! This toggles autofire on/off. Used for both holding the mouse and using the
//! autofire keybind (`E`).

use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct AutoFirePacket {
        pub enabled: bool,
    }
}
