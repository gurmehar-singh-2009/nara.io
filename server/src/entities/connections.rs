use std::sync::Arc;

use dashmap::DashMap;
use paris::error;
use shared::packets::Packet;

use crate::{
    errors::ServerError,
    net::{ClientConnection, ConnectionState},
};

#[derive(Clone)]
pub struct Connections {
    inner: Arc<DashMap<u32, ClientConnection<{ ConnectionState::Authenticated }>>>,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    pub fn insert(&self, id: u32, conn: ClientConnection<{ ConnectionState::Authenticated }>) {
        self.inner.insert(id, conn);
    }

    pub fn remove(&self, id: u32) {
        self.inner.remove(&id);
    }

    pub fn contains(&self, id: u32) -> bool {
        self.inner.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn decrypt(&self, id: u32, ciphertext: &[u8]) -> Option<Result<Vec<u8>, ServerError>> {
        self.inner
            .get_mut(&id)
            .map(|mut conn| conn.decrypt(ciphertext))
    }

    pub fn send_to<P: Packet>(&self, id: u32, packet: P) {
        if let Some(mut conn) = self.inner.get_mut(&id) {
            // println!(
            //     "SENDING packet: type={}, wire_id={}, logical_id={}",
            //     std::any::type_name::<P>(),
            //     P::ID,
            //     P::ID ^ shared::packets::PACKET_SEED,
            // );

            if let Err(err) = conn.send(packet) {
                error!("failed to send to {id}: {err}");
            }
        }
    }

    pub fn broadcast<P: Packet + Clone>(&self, packet: P) {
        self.broadcast_with_exceptions(packet, &[]);
    }

    pub fn broadcast_with_exceptions<P: Packet + Clone>(&self, packet: P, exceptions: &[u32]) {
        for mut entry in self.inner.iter_mut() {
            if exceptions.contains(entry.key()) {
                continue;
            }

            if let Err(err) = entry.value_mut().send(packet.clone()) {
                error!("broadcast failed for {}: {err}", entry.key());
            }
        }
    }
}
