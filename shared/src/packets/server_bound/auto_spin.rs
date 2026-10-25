// this just toggles auto spin

use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct AutoSpinPacket {
        pub enabled: bool,
    }
}
