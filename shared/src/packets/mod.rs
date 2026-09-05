use bitcode::{Decode, Encode};
use bytes::{BufMut, Bytes, BytesMut};
use snafu::ResultExt;

use crate::{
    errors::{DecodeSnafu, InvalidByteSizeSnafu, SharedError},
    packets::{
        client_bound::{
            AddEntityPacket, LeaderboardPacket, PlayerStatsPacket, RemoveEntityPacket,
            UpdateEntityPacket,
        },
        handshake::HandshakePacket,
        server_bound::{AimPacket, AutoFirePacket, MovementPacket, SpawnReqPacket},
    },
};

pub mod client_bound;
pub mod handshake;
pub mod server_bound;

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/packet_seed.rs"));

const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 1;
    }
    hash
}

const fn junk_size_a(seed: u8, name_hash: u64) -> usize {
    (((seed as u64 ^ name_hash) % 3) + 1) as usize
}

const fn junk_size_b(seed: u8, name_hash: u64) -> usize {
    (((seed as u64 ^ name_hash.rotate_left(17)) % 5) + 1) as usize
}

#[derive(Encode, Decode, Clone, Copy, Debug)]
// #[cfg_attr(debug_assertions, derive(Debug))]
pub struct JunkData<const N: usize>(pub [u8; N]);

impl<const N: usize> JunkData<N> {
    pub fn random(entropy: u64) -> Self {
        let mut data = [0u8; N];
        let mut state = entropy.wrapping_add(PACKET_SEED as u64);

        let mut i = 0;
        while i < N {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;

            data[i] = (state & 0xff) as u8;
            i += 1;
        }

        Self(data)
    }
}

impl<const N: usize> Default for JunkData<N> {
    fn default() -> Self {
        Self::random(0)
    }
}

#[macro_export]
macro_rules! junk_packet {
    (
        $(
            $(#[$meta:meta])*
            $vis:vis struct $name:ident {
                $(
                    $(#[$field_meta:meta])*
                    $field_vis:vis $field_name:ident : $field_ty:ty
                ),* $(,)?
            }
        )*
    ) => {
        $(
            $(#[$meta])*
            #[derive(Encode, Decode, Clone, Debug)]
            // #[cfg_attr(debug_assertions, derive(Debug))]
            $vis struct $name {
                pub _junk_a: $crate::packets::JunkData<{
                    $crate::packets::junk_size_a(
                        $crate::packets::PACKET_SEED,
                        $crate::packets::fnv1a(stringify!($name).as_bytes())
                    )
                }>,

                $(
                    $(#[$field_meta])*
                    $field_vis $field_name : $field_ty,
                )*

                pub _junk_b: $crate::packets::JunkData<{
                    $crate::packets::junk_size_b(
                        $crate::packets::PACKET_SEED,
                        $crate::packets::fnv1a(stringify!($name).as_bytes())
                    )
                }>,
            }

            impl $name {
                #[allow(unused)]
                pub fn new($($field_name: $field_ty,)* entropy: u64) -> Self {
                    Self {
                        _junk_a: $crate::packets::JunkData::random(entropy),
                        $($field_name,)*
                        _junk_b: $crate::packets::JunkData::random(entropy.wrapping_mul(31)),
                    }
                }
            }
        )*
    };
}

macro_rules! register_packets {
    ($(($index:expr, $packet:ident)),* $(,)?) => {
        $(
            impl Packet for $packet {
                const ID: u8 = ($index as u8) ^ PACKET_SEED;
            }
        )*
    };
}

pub trait Packet: Send + Sync + Sized + Encode + for<'de> Decode<'de> {
    const ID: u8;

    fn encode(&self) -> Bytes {
        let payload = bitcode::encode(self);

        let mut bytes = BytesMut::with_capacity(payload.len() + 1);

        bytes.put_u8(Self::ID);
        bytes.extend_from_slice(&payload);

        bytes.freeze()
    }

    fn decode(data: &[u8]) -> Result<Self, SharedError> {
        if data.is_empty() {
            do yeet InvalidByteSizeSnafu.build();
        }

        if data[0] != Self::ID {
            do yeet InvalidByteSizeSnafu.build(); // TODO: InvalidPacketId
        }

        bitcode::decode(&data[1..]).context(DecodeSnafu)
    }
}

register_packets! {
    (0, HandshakePacket),
    (1, SpawnReqPacket),
    (2, AddEntityPacket),
    (3, UpdateEntityPacket),
    (4, MovementPacket),
    (5, AimPacket),
    (6, AutoFirePacket),
    (7, PlayerStatsPacket),
    (8, RemoveEntityPacket),
    (9, LeaderboardPacket),
}
