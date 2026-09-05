use std::{cell::RefCell, rc::Rc, time::Duration};

use ed25519_dalek::VerifyingKey;
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{Message, futures::WebSocket};
use shared::packets::PACKET_SEED;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::window;
use winit::{event_loop::EventLoop, platform::web::EventLoopExtWebSys};

mod render;
mod socket;

#[cfg(target_arch = "wasm32")]
use tokio_with_wasm::alias as tokio;

use crate::{render::renderer::Renderer, socket::Socket, structs::game_state::GameState};

const PUBLIC_KEY_BYTES: &[u8; 32] = include_bytes!("../public_key.bin");

pub fn get_server_verifying_key() -> VerifyingKey {
    VerifyingKey::from_bytes(PUBLIC_KEY_BYTES).expect("Invalid verifying key length")
}

mod entities;
mod structs;

#[wasm_bindgen(start)]
pub async fn start() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).unwrap_throw();

    let window = window().unwrap();
    let game_state = Rc::new(RefCell::new(GameState {
        my_player_id: None,
        players: vec![],
        shapes: vec![],
        bullets: vec![],

        movement_dir: None,
        mouse_angle: None,
        auto_fire: false,

        move_up: false,
        move_down: false,
        move_left: false,
        move_right: false,

        level: 1,
        xp: 0,
        xp_to_next: 250,
        health: 100,
        max_health: 100,

        leaderboard: vec![],
        upgrade_request: None,
        upgrade_levels: [0u8; 8],

        chat_message: None,
        chat_channel: structs::game_state::ChatChannel::Global,
        incoming_chat: Vec::new(),
        class_upgrades_available: false,
        class_choice: None,
    }));
    let game_state_for_socket = Rc::clone(&game_state);

    let closure = Closure::<dyn FnMut()>::new(move || {
        let game_state = Rc::clone(&game_state_for_socket);

        spawn_local(async move {
            Socket::new(game_state, "ws://localhost:8080".into()).await;
        });
    });

    web_sys::console::log_1(&format!("PACKET SEED: {}", PACKET_SEED).into());

    window
        .add_event_listener_with_callback("load", closure.as_ref().unchecked_ref())
        .unwrap();

    closure.forget();

    let event_loop = EventLoop::with_user_event().build().unwrap();
    let render = Renderer::new(&event_loop, Rc::clone(&game_state));

    event_loop.spawn_app(render);
}
