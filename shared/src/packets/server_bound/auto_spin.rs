//! The autospin packet.
//! This toggles autospin on/off.
//! Toggled using the keybind (`C`).

use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct AutoSpinPacket {
        pub enabled: bool,
    }
}
