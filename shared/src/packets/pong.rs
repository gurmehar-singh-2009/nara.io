use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct PingPacket {
        pub time: f64,
    }

    pub struct PongPacket {
        pub time: f64,
    }
}
