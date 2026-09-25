#![feature(yeet_expr)]
#![feature(adt_const_params)]
#![feature(const_param_ty_trait)]
#![feature(stmt_expr_attributes)]
#![allow(incomplete_features)]
#![allow(clippy::module_inception)] // I don't think it's a big deal.

use std::{ops::Deref, sync::Arc};

use ed25519_dalek::SigningKey;
use futures_util::{SinkExt, StreamExt};
use shared::packets::{
    PACKET_SEED, Packet, TankSelectPacket, handshake::HandshakePacket, server_bound::{AimPacket, AutoFirePacket, ChatSendPacket, MovementPacket, SpawnReqPacket},
};
use snafu::ResultExt;
use tokio::{net::TcpListener, sync::mpsc::unbounded_channel};
use tokio_tungstenite::{accept_async_with_config, tungstenite::protocol::WebSocketConfig};
use x25519_dalek::PublicKey;

mod game;
mod scripting;

use paris::{error, info, log};

// const TANK_TREE: std::sync::LazyLock<Vec<TankTree>> = std::sync::LazyLock::new(||
// load_tank_tree());
use crate::{
    entities::connections::Connections,
    errors::{LoadConfigSnafu, PortBindFailureSnafu, ServerError},
    fs::load_config::Config,
    game::game_state::{GameEvents, GameState},
    net::{
        ClientConnection,
        ip_lookup::{LookupProvider, lookup, normalize_ip},
    },
};

mod anti_cheat;
mod entities;
mod errors;
mod fs;
mod net;

/// Public key.
const PUBLIC_KEY_BYTES: &[u8; 32] = include_bytes!("../server_key.bin");

/// Returns a `SigningKey` given the public key.
pub fn get_server_signing_key() -> SigningKey {
    SigningKey::from_bytes(PUBLIC_KEY_BYTES)
}

#[tokio::main]
async fn main() -> Result<(), ServerError> {
    let addr = "0.0.0.0:8080".to_string();
    // Fine tune later.
    let socket_config = WebSocketConfig::default()
        .max_message_size(Some(4096))
        .max_frame_size(Some(1024));
    let signing_key = Arc::new(get_server_signing_key());
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("rust crypto error");

    // Map of all connections -> Player and vice versa.
    let connections = Connections::new();
    let config = Config::load("src/fs/data/config.toml").context(LoadConfigSnafu)?;
    let config = Arc::new(config);

    // Channel system for the game state.
    let (game_channel_send, game_channel_recv) = unbounded_channel::<GameEvents>();
    let game_channel_send = Arc::new(game_channel_send);
    // tank definitions come from content/tanks/*.lua now; the game state
    // builds the tree itself and hot-reloads it in its loop
    let mut game_state =
        match GameState::new(game_channel_recv, connections.clone(), config.clone()) {
            Ok(state) => state,
            Err(e) => {
                eprintln!(
                    "Failed to initialize game state! Missing file or script error: {}",
                    e
                );

                std::process::exit(1);
            }
        };

    // Spawn a system thread to offload our core game loop to.
    // We utilize channels so we can actually send messages to the game state.
    tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            game_state.game_loop().await;
        });
    });

    let try_socket = TcpListener::bind(addr).await;
    let listener = try_socket.context(PortBindFailureSnafu)?;

    // Success message.
    info!("nara.io server running on port 8080!");

    // For debugging purposes, the packet seed.
    log!("PACKET SEED: {}", PACKET_SEED);

    // Counter for the current player id.
    let mut current_id = 0;

    // Read incoming socket requests.
    while let Ok((stream, addr)) = listener.accept().await {
        let client_ip = addr.ip();

        if !client_ip.is_loopback() {
            let ip = normalize_ip(&client_ip.to_string());
            let mut is_bad_actor = false;

            for provider in LookupProvider::all() {
                if let Some(c) = &lookup(&ip, *provider) {
                    is_bad_actor |= [
                        c.connection.is_crawler,
                        c.connection.is_datacenter,
                        c.connection.is_vpn,
                        c.connection.is_proxy,
                        c.connection.is_tor,
                    ]
                    .into_iter()
                    .flatten()
                    .any(|v| v);

                    if is_bad_actor {
                        break;
                    }
                }
            }

            // The IP check is unreliable and bugs when hosting on VM.
            // Fix it later.
            // if is_bad_actor {
            //     error!("closed connection due to malicious ip: {}", ip);
            //     continue;
            // }
        }

        let ws_stream = match accept_async_with_config(stream, Some(socket_config)).await {
            Ok(ws) => ws,
            Err(e) => {
                error!("WebSocket handshake failed: {e}");
                continue;
            }
        };
        let (mut write, mut read) = ws_stream.split();

        // Initialze Read/Write channels for the socket.
        // We need to pass this into the `ClientConnection` struct.
        let (socket_send_tx, mut socket_recv_rx) =
            tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

        let id = current_id;

        // Which is defined here.
        let client_connection = ClientConnection::new(id, socket_send_tx);

        // Setup channel read/write system.
        tokio::spawn(async move {
            while let Some(msg) = socket_recv_rx.recv().await {
                if write.send(msg.into()).await.is_err() {
                    break;
                }
            }
        });

        // Increment ID. I don't care about reusing.
        current_id += 1;

        let signing_key = signing_key.clone();
        let game_channel_send = game_channel_send.clone();
        let connections = connections.clone();

        tokio::spawn(async move {
            // Create an authenticated connection instance.
            let authenticated_connection = match read.next().await {
                Some(Ok(msg)) => {
                    println!("{}", msg);
                    if !msg.is_binary() {
                        log!("u aint even trying gng");
                        return;
                    }

                    let msg = msg.into_data();

                    // println!("received {} bytes: {:02x?}", msg.len(), msg);

                    let decoded = match HandshakePacket::decode(&msg) {
                        Ok(data) => data,

                        // Decoding should never fail, disconnect the client.
                        Err(_) => {
                            log!("couldnt decrypt ts");
                            return;
                        }
                    };

                    // We expect the first packet to be the handshake
                    // initiation, otherwise we disconnect
                    // the client. In the future soft ban the client based on
                    // IP.
                    match client_connection.respond_handshake(
                        &PublicKey::from(<[u8; 32]>::try_from(&decoded.handshake[..32]).unwrap()),
                        &signing_key,
                    ) {
                        Ok(conn) => conn,
                        Err(_) => {
                            log!("handshake failed");
                            return;
                        }
                    }
                }

                _ => {
                    log!("gng...");
                    return;
                }
            };

            connections.insert(id, authenticated_connection);

            // Now that the handshake has been established previously
            // we can continously read the socket stream.
            while let Some(Ok(msg)) = read.next().await {
                if msg.is_binary() {
                    let ciphertext = msg.into_data();

                    let plaintext = match connections.decrypt(id, &ciphertext) {
                        Some(Ok(pt)) => pt,
                        Some(Err(_)) => {
                            log!("packet decryption failed for {id}");
                            continue;
                        }
                        None => {
                            log!("no connection found for id {id}");
                            continue;
                        }
                    };

                    if plaintext.is_empty() {
                        continue;
                    }

                    let wire_id = plaintext[0];
                    let logical_id = wire_id ^ PACKET_SEED;

                    match logical_id {
                        1 => match SpawnReqPacket::decode(&plaintext) {
                            Ok(SpawnReqPacket { name, .. }) => {
                                let _ =
                                    game_channel_send.send(GameEvents::PlayerSpawn { id, name });
                            }
                            Err(_) => {
                                log!("failed to decode SpawnReqPacket from {id}");
                            }
                        },
                        4 => match MovementPacket::decode(&plaintext) {
                            Ok(MovementPacket { dir, .. }) => {
                                let _ =
                                    game_channel_send.send(GameEvents::PlayerMovement { id, dir });
                            }
                            Err(_) => {
                                log!("failed to decode MovementPacket from {id}");
                            }
                        },

                        6 => match AutoFirePacket::decode(&plaintext) {
                            Ok(AutoFirePacket { enabled, .. }) => {
                                let _ = game_channel_send
                                    .send(GameEvents::PlayerAutoFire { id, enabled });
                            }
                            Err(_) => {
                                log!("failed to decode AutoFirePacket from {id}");
                            }
                        },

                        5 => match AimPacket::decode(&plaintext) {
                            Ok(AimPacket { dir, .. }) => {
                                let _ = game_channel_send.send(GameEvents::PlayerAim { id, dir });
                            }
                            Err(_) => {
                                log!("failed to decode AimPacket from {id}");
                            }
                        },

                        11 => match TankSelectPacket::decode(&plaintext) {
                            Ok(TankSelectPacket { tank_id, .. }) => {
                                let _ =
                                    game_channel_send.send(GameEvents::TankSelect { id, tank_id });
                            }
                            Err(_) => {
                                log!("failed to decode TankSelectPacket from {id}");
                            }
                        },

                        12 => match ChatSendPacket::decode(&plaintext) {
                            Ok(ChatSendPacket { channel, text, .. }) => {
                                let _ = game_channel_send.send(GameEvents::ChatMessage {
                                    id,
                                    channel,
                                    text,
                                });
                            }
                            Err(_) => {
                                log!("failed to decode ChatSendPacket from {id}");
                            }
                        },

                        logical_id => {
                            log!(
                                "unexpected packet from {id}: logical_id={logical_id} wire_id={wire_id}"
                            );
                        }
                    }
                }
            }

            let _ = game_channel_send.send(GameEvents::PlayerDisconnect { id });
        });
    }

    Ok(())
}
