use std::{cell::RefCell, rc::Rc};

use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce, aead::Aead};
use futures::{SinkExt, StreamExt, channel::mpsc::unbounded};
use glam::Vec2;
use gloo_net::websocket::{Message, futures::WebSocket};
use hkdf::Hkdf;
use js_sys::Uint8Array;
use sha2::Sha256;
use shared::packets::{
    PACKET_SEED, Packet, PlayerUpgradesPacket, StatUpgradePacket, TankSelectPacket, TankTreePacket,
    client_bound::{
        AddEntityPacket, ArenaPacket, DeathPacket, EntityType, LeaderboardPacket,
        PlayerStatsPacket,
        RemoveEntityPacket, ServerInfoPacket, SpectatePacket, TankInfoPacket, UpdateEntityPacket,
    },
    handshake::{HandShake, HandshakePacket},
    level_scale,
    pong::{PingPacket, PongPacket},
    server_bound::{
        AimPacket, AutoFirePacket, AutoSpinPacket, ChatMessagePacket, ChatSendPacket,
        MouseMovePacket, MovementPacket, RepelPacket, SpawnReqPacket,
    },
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Crypto, HtmlInputElement, window};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    entities::tank::Tank,
    render::tank_upgrades,
    structs::game_state::{ArenaInfo, ChatChannel, DeathInfo, GameState, IncomingChat, TankInfo},
};

const PING_INTERVAL_MS: f64 = 1000.0;

pub struct Socket {
    pub secret_key: StaticSecret,
}

pub fn get_random_bytes(crypto: &Crypto, buf: &mut [u8]) -> Result<(), js_sys::Error> {
    let temp_js_array = Uint8Array::new_with_length(buf.len() as u32);
    crypto
        .get_random_values_with_js_u8_array(&temp_js_array)
        .map_err(|_| js_sys::Error::new("Crypto error"))?;
    temp_js_array.copy_to(buf);
    Ok(())
}

fn nonce(counter: u32) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[..4].copy_from_slice(&counter.to_be_bytes());
    Nonce::from(bytes)
}

fn derive_ciphers(
    secret_key: &StaticSecret,
    server_public_key: &PublicKey,
) -> (ChaCha20Poly1305, ChaCha20Poly1305) {
    let shared_secret = secret_key.diffie_hellman(server_public_key);
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret.as_bytes());
    let mut client_to_server = [0u8; 32];
    let mut server_to_client = [0u8; 32];

    hkdf.expand(b"client-to-server", &mut client_to_server)
        .expect("HKDF expansion failed");
    hkdf.expand(b"server-to-client", &mut server_to_client)
        .expect("HKDF expansion failed");

    let send_cipher = ChaCha20Poly1305::new(Key::from_slice(&client_to_server));
    let recv_cipher = ChaCha20Poly1305::new(Key::from_slice(&server_to_client));

    (send_cipher, recv_cipher)
}

impl Socket {
    pub async fn new(game: Rc<RefCell<GameState>>, url: String) -> Self {
        let socket = WebSocket::open(&url).expect("Failed to connect to WebSocket server");
        let (mut send, mut recv) = socket.split();
        let (tx, mut rx) = unbounded::<Vec<u8>>();

        let window = window().expect("no window");
        let crypto = window.crypto().expect("crypto unavailable");

        let mut secret_bytes = [0u8; 32];
        get_random_bytes(&crypto, &mut secret_bytes).expect("crypto failed");

        let secret_key = StaticSecret::from(secret_bytes);
        let public_key = PublicKey::from(&secret_key);

        let mut handshake_bytes: HandShake = [0u8; 96];
        handshake_bytes[..32].copy_from_slice(public_key.as_bytes());
        get_random_bytes(&crypto, &mut handshake_bytes[32..]).expect("padding failed");

        let handshake = HandshakePacket::new(handshake_bytes, PACKET_SEED as u64);
        tx.unbounded_send(handshake.encode().to_vec())
            .expect("send failed");

        let send_cipher: Rc<RefCell<Option<ChaCha20Poly1305>>> = Rc::new(RefCell::new(None));
        let send_nonce_count: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
        let recv_cipher: Rc<RefCell<Option<ChaCha20Poly1305>>> = Rc::new(RefCell::new(None));
        let recv_nonce_count: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));

        spawn_local(async move {
            while let Some(packet) = rx.next().await {
                if let Err(err) = send.send(Message::Bytes(packet)).await {
                    web_sys::console::error_1(&format!("Failed sending packet: {err:?}").into());
                    break;
                }
            }
        });

        let pending_spawn: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        let tx_clone = tx.clone();
        let send_cipher_clone = Rc::clone(&send_cipher);
        let send_nonce_count_clone = Rc::clone(&send_nonce_count);
        let pending_spawn_clone = Rc::clone(&pending_spawn);
        let game_clone = Rc::clone(&game);

        let button = window
            .document()
            .unwrap()
            .get_element_by_id("playButton")
            .expect("playButton not found");

        let closure = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let game = game_clone.borrow();
            if game.my_player_id.is_some() {
                return;
            }
            drop(game);

            let name = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("nameInput"))
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                .map(|input| input.value())
                .unwrap_or_else(|| "Unnamed".into());

            let cipher_ref = send_cipher_clone.borrow();
            let Some(cipher) = cipher_ref.as_ref() else {
                web_sys::console::log_1(&"Connecting... spawning as soon as we're in.".into());
                *pending_spawn_clone.borrow_mut() = Some(name);
                return;
            };

            send_spawn(cipher, &send_nonce_count_clone, &tx_clone, name);
        });

        button
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget();

        let send_cipher_for_reader = Rc::clone(&send_cipher);
        let recv_cipher_for_reader = Rc::clone(&recv_cipher);
        let recv_nonce_count_for_reader = Rc::clone(&recv_nonce_count);
        let secret_key_for_reader = secret_key.clone();
        let game_for_reader = Rc::clone(&game);
        let send_nonce_count_for_reader = Rc::clone(&send_nonce_count);

        let mut last_auto_fire = false;
        let mut last_repel = false;
        let mut last_auto_spin = false;
        let mut last_ping_at = 0.0f64;

        spawn_local(async move {
            while let Some(result) = recv.next().await {
                match result {
                    Ok(Message::Bytes(bytes)) => {
                        if bytes.is_empty() {
                            continue;
                        }

                        if recv_cipher_for_reader.borrow().is_none() {
                            match HandshakePacket::decode(&bytes) {
                                Ok(handshake) => {
                                    let server_public_key = PublicKey::from(
                                        <[u8; 32]>::try_from(&handshake.handshake[..32])
                                            .expect("invalid server public key"),
                                    );
                                    let (new_send_cipher, new_recv_cipher) =
                                        derive_ciphers(&secret_key_for_reader, &server_public_key);
                                    *send_cipher_for_reader.borrow_mut() = Some(new_send_cipher);
                                    *recv_cipher_for_reader.borrow_mut() = Some(new_recv_cipher);
                                    *recv_nonce_count_for_reader.borrow_mut() = 0;
                                    web_sys::console::log_1(
                                        &"Handshake accepted! Encryption established.".into(),
                                    );

                                    if let Some(name) = pending_spawn.borrow_mut().take() {
                                        if let Some(cipher) = send_cipher_for_reader.borrow().as_ref()
                                        {
                                            send_spawn(
                                                cipher,
                                                &send_nonce_count_for_reader,
                                                &tx,
                                                name,
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("Handshake decode failed: {e:?}").into(),
                                    );
                                }
                            }
                            continue;
                        }

                        let plaintext = {
                            let cipher_ref = recv_cipher_for_reader.borrow();
                            let cipher = match cipher_ref.as_ref() {
                                Some(cipher) => cipher,
                                None => {
                                    web_sys::console::error_1(
                                        &"Received encrypted packet before cipher was ready".into(),
                                    );
                                    continue;
                                }
                            };

                            let counter = {
                                let mut counter = recv_nonce_count_for_reader.borrow_mut();
                                let value = *counter;
                                *counter += 1;
                                value
                            };

                            match cipher.decrypt(&nonce(counter), bytes.as_ref()) {
                                Ok(data) => data,
                                Err(err) => {
                                    web_sys::console::error_1(
                                        &format!(
                                            "Packet decryption failed (nonce={}): {err:?}",
                                            counter
                                        )
                                        .into(),
                                    );
                                    continue;
                                }
                            }
                        };

                        if plaintext.is_empty() {
                            continue;
                        }

                        let wire_id = plaintext[0];
                        let id = wire_id ^ PACKET_SEED;

                        match id {
                            0 => match HandshakePacket::decode(&plaintext) {
                                Ok(_) => web_sys::console::log_1(
                                    &"Unexpected encrypted handshake packet".into(),
                                ),
                                Err(e) => web_sys::console::error_1(
                                    &format!("Handshake decode failed: {e:?}").into(),
                                ),
                            },

                            2 => match AddEntityPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    match data.entity_type {
                                        EntityType::Player => {
                                            let pos = Vec2::from_array([data.x, data.y]);

                                            if let Some(p) =
                                                game.players.iter_mut().find(|p| p.id == data.id)
                                            {
                                                if p.dying {
                                                    p.pos = pos;
                                                    p.last_pos = pos;
                                                    p.render_pos = pos;
                                                    p.render_alpha = 0.0;
                                                    p.dying = false;
                                                }
                                                p.name = data.name;
                                                p.scale = level_scale(data.level);
                                                p.barrels = data.barrels.clone();
                                                p.turrets = data.turrets.clone();
                                                p.is_mine = data.is_entity_mine;
                                                p.team = data.team;
                                            } else {
                                                let mut entity = Tank::new(data.id, data.name, pos);
                                                entity.scale = level_scale(data.level);
                                                entity.barrels = data.barrels.clone();
                                                entity.turrets = data.turrets.clone();
                                                entity.is_mine = data.is_entity_mine;
                                                entity.team = data.team;
                                                game.players.push(entity);
                                            }

                                            if data.is_entity_mine {
                                                game.my_player_id = Some(data.id);
                                            }
                                        }
                                        EntityType::Shape => {
                                            game.shapes.retain(|s| s.id != data.id);
                                            let shape = crate::entities::shape::Shape::new(
                                                data.id, data.x, data.y, data.level,
                                            );
                                            game.shapes.push(shape);
                                        }
                                        EntityType::Bullet | EntityType::Trap => {
                                            let bullet = crate::entities::bullet::Bullet::new(
                                                data.id, data.x, data.y,
                                            );
                                            game.bullets.push(bullet);
                                        }
                                        EntityType::Drone => {
                                            game.drones.retain(|d| d.id != data.id);
                                            let mut drone = crate::entities::drone::Drone::new(
                                                data.id, data.x, data.y,
                                            );
                                            drone.team = data.team;
                                            game.drones.push(drone);
                                        }
                                    }
                                    drop(game);

                                    if data.is_entity_mine {
                                        show_game_view();
                                        show_death_notice(false);
                                    }
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("AddEntityPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            3 => match UpdateEntityPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let current_time =
                                        web_sys::window().unwrap().performance().unwrap().now();
                                    let mut game = game_for_reader.borrow_mut();

                                    for entry in data.data {
                                        match entry.entity_type {
                                            EntityType::Bullet | EntityType::Trap => {
                                                if let Some(bullet) = game
                                                    .bullets
                                                    .iter_mut()
                                                    .find(|b| b.id == entry.id)
                                                {
                                                    if bullet.dead {
                                                        continue;
                                                    }

                                                    let t = ((current_time
                                                        - bullet.last_update_time)
                                                        / 100.0)
                                                        .clamp(0.0, 1.0)
                                                        as f32;
                                                    bullet.last_pos =
                                                        bullet.last_pos.lerp(bullet.pos, t);

                                                    if entry.health == 0 {
                                                        bullet.pos =
                                                            glam::Vec2::new(entry.x, entry.y);
                                                        bullet.vel = glam::Vec2::ZERO;
                                                        bullet.dead = true;
                                                        bullet.dying = false;
                                                        bullet.last_update_time = current_time;
                                                        continue;
                                                    }

                                                    let dt = (current_time
                                                        - bullet.last_update_time)
                                                        .max(30.0)
                                                        as f32
                                                        / 1000.0 as f32;
                                                    let v = (glam::Vec2::new(entry.x, entry.y)
                                                        - bullet.pos)
                                                        / dt;
                                                    bullet.vel = if v.length() > 1500.0 {
                                                        v.normalize_or_zero() * 1500.0
                                                    } else {
                                                        v
                                                    };

                                                    bullet.pos = glam::Vec2::new(entry.x, entry.y);
                                                    bullet.rot = entry.rot;
                                                    bullet.last_update_time = current_time;
                                                    bullet.dying = false;
                                                    bullet.team = entry.team;
                                                    if entry.scale > 2.0 {
                                                        bullet.radius = entry.scale;
                                                    }
                                                } else {
                                                    let mut new_bullet =
                                                        crate::entities::bullet::Bullet::new(
                                                            entry.id, entry.x, entry.y,
                                                        );
                                                    new_bullet.last_update_time = current_time;
                                                    new_bullet.dying = false;
                                                    if entry.scale > 2.0 {
                                                        new_bullet.radius = entry.scale;
                                                    }
                                                    new_bullet.recoil = entry.kind;
                                                    new_bullet.team = entry.team;
                                                    new_bullet.trap =
                                                        entry.entity_type == EntityType::Trap;
                                                    new_bullet.dead = entry.health == 0;
                                                    new_bullet.is_mine = bullet_owner_is_mine(
                                                        entry.kind,
                                                        game.my_player_id,
                                                    );
                                                    game.bullets.push(new_bullet);
                                                }
                                            }
                                            EntityType::Drone => {
                                                if let Some(drone) = game
                                                    .drones
                                                    .iter_mut()
                                                    .find(|d| d.id == entry.id)
                                                {
                                                    let t = ((current_time
                                                        - drone.last_update_time)
                                                        / 100.0)
                                                        .clamp(0.0, 1.0)
                                                        as f32;
                                                    drone.last_pos =
                                                        drone.last_pos.lerp(drone.pos, t);
                                                    drone.pos = glam::Vec2::new(entry.x, entry.y);
                                                    drone.last_rot = drone.rot;
                                                    drone.rot = entry.rot;
                                                    drone.radius = entry.scale;
                                                    drone.team = entry.team;
                                                    drone.last_update_time = current_time;
                                                    drone.dying = false;
                                                } else {
                                                    let mut new_drone =
                                                        crate::entities::drone::Drone::new(
                                                            entry.id, entry.x, entry.y,
                                                        );
                                                    new_drone.last_update_time = current_time;
                                                    new_drone.radius = entry.scale;
                                                    new_drone.rot = entry.rot;
                                                    new_drone.recoil = entry.kind;
                                                    new_drone.team = entry.team;
                                                    new_drone.is_mine = bullet_owner_is_mine(
                                                        entry.kind,
                                                        game.my_player_id,
                                                    );
                                                    game.drones.push(new_drone);
                                                }
                                            }
                                            EntityType::Shape => {
                                                if let Some(shape) = game
                                                    .shapes
                                                    .iter_mut()
                                                    .find(|s| s.id == entry.id)
                                                {
                                                    shape.last_pos = shape.pos;
                                                    shape.pos = glam::Vec2::new(entry.x, entry.y);
                                                    shape.last_rot = shape.rot;
                                                    shape.rot = entry.rot;
                                                    shape.health = entry.health;
                                                    shape.max_health = entry.max_health;
                                                    shape.last_update_time = current_time;
                                                } else {
                                                    let mut new_shape =
                                                        crate::entities::shape::Shape::new(
                                                            entry.id, entry.x, entry.y, entry.kind,
                                                        );
                                                    new_shape.last_update_time = current_time;
                                                    new_shape.health = entry.health;
                                                    new_shape.max_health = entry.max_health;
                                                    game.shapes.push(new_shape);
                                                }
                                            }
                                            EntityType::Player => {
                                                if let Some(player) = game
                                                    .players
                                                    .iter_mut()
                                                    .find(|p| p.id == entry.id)
                                                {
                                                    let t = ((current_time
                                                        - player.last_update_time)
                                                        / 100.0)
                                                        .clamp(0.0, 1.0)
                                                        as f32;
                                                    player.last_pos =
                                                        player.last_pos.lerp(player.pos, t);
                                                    player.pos = glam::Vec2::new(entry.x, entry.y);
                                                    player.last_rot = player.rot;
                                                    player.rot = entry.rot;
                                                    player.scale = entry.scale;
                                                    player.health = entry.health;
                                                    player.max_health = entry.max_health;
                                                    player.team = entry.team;
                                                    player.last_turret_aims =
                                                        if player.render_turret_aims.len()
                                                            == entry.turrets.len()
                                                        {
                                                            player.render_turret_aims.clone()
                                                        } else {
                                                            entry.turrets.clone()
                                                        };
                                                    player.turret_aims = entry.turrets;
                                                    player.last_update_time = current_time;
                                                    player.dying = false;
                                                }
                                            }
                                        }
                                    }

                                    let plaintext =
                                        MovementPacket::new(game.movement_dir, PACKET_SEED as u64)
                                            .encode();
                                    let nonce_counter = {
                                        let mut counter = send_nonce_count_for_reader.borrow_mut();
                                        let value = *counter;
                                        *counter += 1;
                                        value
                                    };
                                    let cipher_ref = send_cipher_for_reader.borrow();
                                    if let Some(cipher) = cipher_ref.as_ref() {
                                        if let Ok(encrypted) = cipher
                                            .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                        {
                                            let _ = tx.unbounded_send(encrypted.to_vec());
                                        }
                                    }

                                    let plaintext = AimPacket::new(
                                        game.mouse_angle.unwrap_or(0.),
                                        PACKET_SEED as u64,
                                    )
                                    .encode();
                                    let nonce_counter = {
                                        let mut counter = send_nonce_count_for_reader.borrow_mut();
                                        let value = *counter;
                                        *counter += 1;
                                        value
                                    };
                                    let cipher_ref = send_cipher_for_reader.borrow();
                                    if let Some(cipher) = cipher_ref.as_ref() {
                                        if let Ok(encrypted) = cipher
                                            .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                        {
                                            let _ = tx.unbounded_send(encrypted.to_vec());
                                        }
                                    }

                                    if let Some([x, y, w, h]) = game.mouse_screen {
                                        let plaintext =
                                            MouseMovePacket::new(x, y, w, h, PACKET_SEED as u64)
                                                .encode();
                                        let nonce_counter = {
                                            let mut counter =
                                                send_nonce_count_for_reader.borrow_mut();
                                            let value = *counter;
                                            *counter += 1;
                                            value
                                        };
                                        let cipher_ref = send_cipher_for_reader.borrow();
                                        if let Some(cipher) = cipher_ref.as_ref() {
                                            if let Ok(encrypted) = cipher
                                                .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                            {
                                                let _ = tx.unbounded_send(encrypted.to_vec());
                                            }
                                        }
                                    }

                                    if current_time - last_ping_at >= PING_INTERVAL_MS {
                                        last_ping_at = current_time;
                                        let plaintext =
                                            PingPacket::new(current_time, PACKET_SEED as u64)
                                                .encode();
                                        let nonce_counter = {
                                            let mut counter =
                                                send_nonce_count_for_reader.borrow_mut();
                                            let value = *counter;
                                            *counter += 1;
                                            value
                                        };
                                        let cipher_ref = send_cipher_for_reader.borrow();
                                        if let Some(cipher) = cipher_ref.as_ref() {
                                            if let Ok(encrypted) = cipher
                                                .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                            {
                                                let _ = tx.unbounded_send(encrypted.to_vec());
                                            }
                                        }
                                    }

                                    let current_auto_spin = game.auto_spin;
                                    if current_auto_spin != last_auto_spin {
                                        last_auto_spin = current_auto_spin;
                                        let plaintext = AutoSpinPacket::new(
                                            current_auto_spin,
                                            PACKET_SEED as u64,
                                        )
                                        .encode();
                                        let nonce_counter = {
                                            let mut counter =
                                                send_nonce_count_for_reader.borrow_mut();
                                            let value = *counter;
                                            *counter += 1;
                                            value
                                        };
                                        let cipher_ref = send_cipher_for_reader.borrow();
                                        if let Some(cipher) = cipher_ref.as_ref() {
                                            if let Ok(encrypted) = cipher
                                                .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                            {
                                                let _ = tx.unbounded_send(encrypted.to_vec());
                                            }
                                        }
                                    }

                                    let current_repel = game.repel;
                                    if current_repel != last_repel {
                                        last_repel = current_repel;
                                        let plaintext =
                                            RepelPacket::new(current_repel, PACKET_SEED as u64)
                                                .encode();
                                        let nonce_counter = {
                                            let mut counter =
                                                send_nonce_count_for_reader.borrow_mut();
                                            let value = *counter;
                                            *counter += 1;
                                            value
                                        };
                                        let cipher_ref = send_cipher_for_reader.borrow();
                                        if let Some(cipher) = cipher_ref.as_ref() {
                                            if let Ok(encrypted) = cipher
                                                .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                            {
                                                let _ = tx.unbounded_send(encrypted.to_vec());
                                            }
                                        }
                                    }

                                    let current_auto_fire = game.auto_fire || game.fire_clicked;
                                    game.fire_clicked = false;
                                    if current_auto_fire != last_auto_fire {
                                        last_auto_fire = current_auto_fire;
                                        let plaintext = AutoFirePacket::new(
                                            current_auto_fire,
                                            PACKET_SEED as u64,
                                        )
                                        .encode();
                                        let nonce_counter = {
                                            let mut counter =
                                                send_nonce_count_for_reader.borrow_mut();
                                            let value = *counter;
                                            *counter += 1;
                                            value
                                        };
                                        let cipher_ref = send_cipher_for_reader.borrow();
                                        if let Some(cipher) = cipher_ref.as_ref() {
                                            if let Ok(encrypted) = cipher
                                                .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                            {
                                                let _ = tx.unbounded_send(encrypted.to_vec());
                                            }
                                        }
                                    }

                                    if let Some(choice) = game.class_choice.take() {
                                        game.class_upgrades_available = false;

                                        if choice > 0 {
                                            if let Some(option) = tank_upgrades::current_options()
                                                .get((choice - 1) as usize)
                                            {
                                                let my_player_id = game.my_player_id;
                                                if let Some(player) = game
                                                    .players
                                                    .iter_mut()
                                                    .find(|p| Some(p.id) == my_player_id)
                                                {
                                                    player.barrels = option.barrels.clone();
                                                }

                                                let plaintext = TankSelectPacket::new(
                                                    option.id,
                                                    PACKET_SEED as u64,
                                                )
                                                .encode();
                                                let nonce_counter = {
                                                    let mut counter =
                                                        send_nonce_count_for_reader.borrow_mut();
                                                    let value = *counter;
                                                    *counter += 1;
                                                    value
                                                };
                                                let cipher_ref = send_cipher_for_reader.borrow();
                                                if let Some(cipher) = cipher_ref.as_ref() {
                                                    if let Ok(encrypted) = cipher.encrypt(
                                                        &nonce(nonce_counter),
                                                        plaintext.as_ref(),
                                                    ) {
                                                        let _ =
                                                            tx.unbounded_send(encrypted.to_vec());
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if let Some(text) = game.chat_message.take() {
                                        let channel = match game.chat_channel {
                                            ChatChannel::Global => 0u8,
                                            ChatChannel::Team => 1u8,
                                        };
                                        let plaintext =
                                            ChatSendPacket::new(channel, text, PACKET_SEED as u64)
                                                .encode();
                                        let nonce_counter = {
                                            let mut counter =
                                                send_nonce_count_for_reader.borrow_mut();
                                            let value = *counter;
                                            *counter += 1;
                                            value
                                        };
                                        let cipher_ref = send_cipher_for_reader.borrow();
                                        if let Some(cipher) = cipher_ref.as_ref() {
                                            if let Ok(encrypted) = cipher
                                                .encrypt(&nonce(nonce_counter), plaintext.as_ref())
                                            {
                                                let _ = tx.unbounded_send(encrypted.to_vec());
                                            }
                                        }
                                    }

                                    if let Some(request) = game.upgrade_request.take() {
                                        if (1..=8).contains(&request) {
                                            let plaintext = StatUpgradePacket::new(
                                                request - 1,
                                                PACKET_SEED as u64,
                                            )
                                            .encode();
                                            let nonce_counter = {
                                                let mut counter =
                                                    send_nonce_count_for_reader.borrow_mut();
                                                let value = *counter;
                                                *counter += 1;
                                                value
                                            };
                                            let cipher_ref = send_cipher_for_reader.borrow();
                                            if let Some(cipher) = cipher_ref.as_ref() {
                                                if let Ok(encrypted) = cipher.encrypt(
                                                    &nonce(nonce_counter),
                                                    plaintext.as_ref(),
                                                ) {
                                                    let _ = tx.unbounded_send(encrypted.to_vec());
                                                }
                                            }
                                        }
                                    }

                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("UpdateEntityPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            7 => match PlayerStatsPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.level = data.level;
                                    game.score = data.score;
                                    game.xp = data.xp;
                                    game.xp_to_next = data.xp_to_next;
                                    game.health = data.health;
                                    game.max_health = data.max_health;
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("PlayerStatsPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            8 => match RemoveEntityPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    let is_my_player = data.entity_type == EntityType::Player
                                        && Some(data.id) == game.my_player_id;

                                    match data.entity_type {
                                        EntityType::Player => {
                                            if let Some(p) =
                                                game.players.iter_mut().find(|p| p.id == data.id)
                                            {
                                                p.dying = true;
                                            }
                                        }
                                        EntityType::Shape => {
                                            if let Some(s) =
                                                game.shapes.iter_mut().find(|s| s.id == data.id)
                                            {
                                                s.dying = true;
                                            }
                                        }
                                        EntityType::Bullet | EntityType::Trap => {
                                            if let Some(b) =
                                                game.bullets.iter_mut().find(|b| b.id == data.id)
                                            {
                                                b.dying = true;
                                            }
                                        }
                                        EntityType::Drone => {
                                            if let Some(d) =
                                                game.drones.iter_mut().find(|d| d.id == data.id)
                                            {
                                                d.dying = true;
                                            }
                                        }
                                    }

                                    if is_my_player {
                                        game.my_player_id = None;
                                        game.auto_fire = false;
                                        game.class_upgrades_available = false;
                                        game.repel = false;
                                        game.auto_fire_toggle = false;
                                        game.auto_spin = false;
                                        last_auto_fire = false;
                                        last_repel = false;
                                        last_auto_spin = false;
                                    }
                                    drop(game);

                                    if is_my_player {
                                        game_for_reader.borrow_mut().died_at = Some(
                                            web_sys::window().unwrap().performance().unwrap().now(),
                                        );
                                        tank_upgrades::push_tank_options(Vec::new());
                                    }
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("RemoveEntityPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            9 => match LeaderboardPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.leaderboard = data.entries;
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("LeaderboardPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            10 => match TankTreePacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.class_upgrades_available = !data.tanks.is_empty();
                                    drop(game);
                                    tank_upgrades::push_tank_options(data.tanks);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("TankTreePacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            13 => match ChatMessagePacket::decode(&plaintext) {
                                Ok(data) => {
                                    let channel = if data.channel == 1 {
                                        ChatChannel::Team
                                    } else {
                                        ChatChannel::Global
                                    };
                                    let mut game = game_for_reader.borrow_mut();
                                    game.incoming_chat.push(IncomingChat {
                                        sender_id: data.sender_id,
                                        channel,
                                        team: data.team,
                                        sender: data.sender,
                                        text: data.text,
                                        timestamp: data.timestamp,
                                    });
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("ChatMessagePacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            25 => match DeathPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.death = Some(DeathInfo {
                                        killer: data.killer,
                                        killer_kind: data.killer_kind,
                                        killer_tank: data.killer_tank,
                                        killer_team: data.killer_team,
                                        killer_sides: data.killer_sides,
                                        killer_barrels: data.killer_barrels,
                                        score: data.score,
                                        level: data.level,
                                        kills: data.kills,
                                        time_alive: data.time_alive,
                                    });
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("DeathPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            21 => match SpectatePacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.spectate = Some(Vec2::new(data.x, data.y));
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("SpectatePacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            22 => match ServerInfoPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.server_load = data.load;
                                    game.server_players = data.players;
                                    game.server_bots = data.bots;
                                    game.server_shapes = data.shapes;
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("ServerInfoPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            24 => match PongPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let now =
                                        web_sys::window().unwrap().performance().unwrap().now();
                                    let rtt = (now - data.time).max(0.0) as f32;
                                    let mut game = game_for_reader.borrow_mut();
                                    game.ping_ms = Some(match game.ping_ms {
                                        Some(old) => old + (rtt - old) * 0.3,
                                        None => rtt,
                                    });
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("PongPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            18 => match ArenaPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.arena = ArenaInfo {
                                        bound: data.bound,
                                        team_count: data.team_count,
                                        walls: data.walls,
                                        zones: data.zones,
                                    };
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("ArenaPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            19 => match TankInfoPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.tank_info = TankInfo {
                                        name: data.name,
                                        field_factor: data.field_factor,
                                        team: data.team,
                                        stat_names: data.stat_names,
                                        stat_maxes: data.stat_maxes,
                                    };
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("TankInfoPacket decode failed: {e:?}").into(),
                                    );
                                }
                            },

                            15 => match PlayerUpgradesPacket::decode(&plaintext) {
                                Ok(data) => {
                                    let mut game = game_for_reader.borrow_mut();
                                    game.upgrade_levels = data.levels;
                                    game.upgrade_points = data.points;
                                    drop(game);
                                }
                                Err(e) => {
                                    web_sys::console::error_1(
                                        &format!("PlayerUpgradesPacket decode failed: {e:?}")
                                            .into(),
                                    );
                                }
                            },

                            _ => {}
                        }
                    }
                    Ok(Message::Text(_)) => {
                        web_sys::console::warn_1(
                            &"Received unexpected text WebSocket message".into(),
                        );
                    }
                    Err(e) => {
                        web_sys::console::warn_1(&format!("WebSocket closed: {e:?}").into());
                        break;
                    }
                }
            }
        });

        Self { secret_key }
    }
}

fn send_spawn(
    cipher: &ChaCha20Poly1305,
    nonce_count: &RefCell<u32>,
    tx: &futures::channel::mpsc::UnboundedSender<Vec<u8>>,
    name: String,
) {
    let plaintext = SpawnReqPacket::new(name, js_sys::Date::now() as u64).encode();

    let counter = {
        let mut counter = nonce_count.borrow_mut();
        let value = *counter;
        *counter += 1;
        value
    };

    match cipher.encrypt(&nonce(counter), plaintext.as_ref()) {
        Ok(encrypted) => {
            if let Err(err) = tx.unbounded_send(encrypted) {
                web_sys::console::error_1(
                    &format!("Failed queueing SpawnReqPacket: {err:?}").into(),
                );
            }
        }
        Err(err) => {
            web_sys::console::error_1(&format!("Failed encrypting SpawnReqPacket: {err:?}").into());
        }
    }
}

fn bullet_owner_is_mine(owner_info: u32, my_id: Option<u32>) -> bool {
    if owner_info == 0 || owner_info & 0x8000_0000 != 0 {
        return false;
    }
    Some(owner_info >> 8) == my_id
}

fn set_display(id: &str, display: &str) {
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(el) = document.get_element_by_id(id) {
            let _ = el.set_attribute("style", &format!("display: {display};"));
        }
    }
}

fn set_playing(playing: bool) {
    if let Some(body) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
    {
        let classes = body.class_list();
        let _ = if playing {
            classes.add_1("playing")
        } else {
            classes.remove_1("playing")
        };
    }
}

pub(crate) fn show_home_screen() {
    set_playing(false);
    set_display("uiOverlay", "flex");
}

fn show_game_view() {
    set_playing(true);
    set_display("uiOverlay", "none");
}

pub(crate) fn show_death_notice(visible: bool) {
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(el) = document.get_element_by_id("deathNotice") {
            let _ = el.set_attribute(
                "style",
                if visible {
                    "display: block;"
                } else {
                    "display: none;"
                },
            );
        }
        if let Some(btn) = document
            .get_element_by_id("playButton")
            .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
        {
            btn.set_inner_text(if visible { "Respawn" } else { "Play" });
        }
    }
}
