use bitcode::{Decode, Encode};

use crate::junk_packet;

junk_packet! {
    pub struct ChatSendPacket {
        pub channel: u8,
        pub text: String,
    }
}

junk_packet! {
    pub struct ChatMessagePacket {
        pub sender_id: u32,
        pub channel: u8,
        pub team: u8,
        pub timestamp: u64,
        pub sender: String,
        pub text: String,
    }
}
