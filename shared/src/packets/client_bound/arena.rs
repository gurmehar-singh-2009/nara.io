use bitcode::{Decode, Encode};

use crate::junk_packet;

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
pub struct WallDef {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
pub struct ZoneDef {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub team: u8,
}

junk_packet! {
    pub struct ArenaPacket {
        pub bound: f32,
        pub team_count: u8,
        pub walls: Vec<WallDef>,
        pub zones: Vec<ZoneDef>,
    }
}
