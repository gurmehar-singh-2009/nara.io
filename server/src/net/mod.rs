#![allow(internal_features)]

use std::{marker::ConstParamTy_, time::UNIX_EPOCH};

use chacha20poly1305::{Key, KeyInit, Nonce, aead::Aead};
use ed25519_dalek::Signer;
use getrandom::{SysRng, rand_core::UnwrapErr};
use hkdf::Hkdf;
use sha2::Sha256;
use shared::packets::{Packet, handshake::HandshakePacket};
use snafu::ResultExt;
use tokio::sync::mpsc::UnboundedSender;
use x25519_dalek::{EphemeralSecret, PublicKey};

pub mod ip_lookup;

use crate::errors::{ChannelEntrySnafu, CipherEncryptSnafu, ServerError};

#[derive(PartialEq, Eq)]
pub enum ConnectionState {
    Handshaking,
    Authenticated,
}

impl ConstParamTy_ for ConnectionState {}

#[derive(Debug)]
pub struct ClientConnection<const S: ConnectionState> {
    send_tx: UnboundedSender<Vec<u8>>,

    send_nonce_count: u32,
    recv_nonce_count: u32,

    send_cipher: Option<chacha20poly1305::ChaCha20Poly1305>,
    recv_cipher: Option<chacha20poly1305::ChaCha20Poly1305>,

    id: u32,
}

impl ClientConnection<{ ConnectionState::Handshaking }> {
    pub fn new(id: u32, send_tx: UnboundedSender<Vec<u8>>) -> Self {
        Self {
            send_tx,

            send_nonce_count: 0,
            recv_nonce_count: 0,

            send_cipher: None,
            recv_cipher: None,

            id,
        }
    }

    pub fn respond_handshake(
        mut self,
        their_public: &PublicKey,
        server_identity_key: &ed25519_dalek::SigningKey,
    ) -> Result<ClientConnection<{ ConnectionState::Authenticated }>, ServerError> {
        let mut rng = UnwrapErr(SysRng);

        let my_secret = EphemeralSecret::random_from_rng(&mut rng);

        let my_public = PublicKey::from(&my_secret);

        let public_key_signature = server_identity_key.sign(my_public.as_bytes());

        let shared_secret = my_secret.diffie_hellman(their_public);

        let hkdf = Hkdf::<Sha256>::new(None, shared_secret.as_bytes());

        drop(shared_secret);

        let mut client_to_server = [0u8; 32];
        let mut server_to_client = [0u8; 32];

        hkdf.expand(b"client-to-server", &mut client_to_server)
            .unwrap();

        hkdf.expand(b"server-to-client", &mut server_to_client)
            .unwrap();

        self.send_cipher = Some(chacha20poly1305::ChaCha20Poly1305::new(Key::from_slice(
            &server_to_client,
        )));

        self.recv_cipher = Some(chacha20poly1305::ChaCha20Poly1305::new(Key::from_slice(
            &client_to_server,
        )));

        let mut handshake = [0u8; 96];

        handshake[..32].copy_from_slice(my_public.as_bytes());

        handshake[32..].copy_from_slice(&public_key_signature.to_bytes());

        let handshake_packet = HandshakePacket::new(
            handshake,
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );

        self.send_plain(handshake_packet)?;

        Ok(ClientConnection {
            send_tx: self.send_tx,

            send_nonce_count: self.send_nonce_count,

            recv_nonce_count: self.recv_nonce_count,

            send_cipher: self.send_cipher,

            recv_cipher: self.recv_cipher,

            id: self.id,
        })
    }

    fn send_plain<P: Packet>(&mut self, packet: P) -> Result<(), ServerError> {
        self.send_tx
            .send(packet.encode().to_vec())
            .context(ChannelEntrySnafu { id: self.id })?;

        Ok(())
    }
}

impl ClientConnection<{ ConnectionState::Authenticated }> {
    fn nonce(nonce_counter: u32) -> Nonce {
        let mut bytes = [0u8; 12];

        bytes[..4].copy_from_slice(&nonce_counter.to_be_bytes());

        Nonce::from(bytes)
    }

    pub fn send<P: Packet>(&mut self, packet: P) -> Result<(), ServerError> {
        let cipher = match &mut self.send_cipher {
            Some(cipher) => cipher,

            None => do yeet ServerError::CipherNotInitialized,
        };

        let encoded = cipher
            .encrypt(
                &Self::nonce(self.send_nonce_count),
                packet.encode().as_ref(),
            )
            .context(CipherEncryptSnafu)?;

        self.send_tx
            .send(encoded)
            .context(ChannelEntrySnafu { id: self.id })?;

        self.send_nonce_count += 1;

        Ok(())
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, ServerError> {
        let cipher = match &mut self.recv_cipher {
            Some(cipher) => cipher,

            None => do yeet ServerError::CipherNotInitialized,
        };

        let decoded = cipher
            .decrypt(&Self::nonce(self.recv_nonce_count), ciphertext)
            .context(CipherEncryptSnafu)?;

        self.recv_nonce_count += 1;

        Ok(decoded)
    }
}
