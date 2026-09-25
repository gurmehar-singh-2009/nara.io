// so this packet is gonna be BIG
//
// containing:
//
// - all configs relevant to client prediction
// - tank hierarchy
// more to come later as i add

use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct InitPacket {

    }
}
