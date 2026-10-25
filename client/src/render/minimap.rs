use glam::Vec2;
use shared::packets::NO_TEAM;

use crate::{
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, team_color, with_alpha},
        scoreboard::rounded_ui_instance,
    },
    structs::game_state::GameState,
};

pub const SIZE: f32 = 180.0;
pub const MARGIN: f32 = 16.0;

const SELF_DOT: f32 = 9.0;
const TEAMMATE_DOT: f32 = 6.0;

pub fn render_data(game: &GameState, screen: Vec2) -> Vec<EntityInstance> {
    let mut instances = Vec::new();
    let bound = game.arena.bound.max(1.0);

    let center = Vec2::new(
        screen.x - MARGIN - SIZE * 0.5,
        screen.y - MARGIN - SIZE * 0.5,
    );
    let min = center - Vec2::splat(SIZE * 0.5);

    instances.push(rounded_ui_instance(
        center,
        Vec2::new(SIZE, SIZE),
        screen,
        with_alpha(DARK_THEME.minimap_background, 0.9),
        with_alpha(DARK_THEME.minimap_border, 0.9),
        3.0,
        8.0,
    ));

    let to_map = |p: Vec2| -> Vec2 {
        let x = ((p.x + bound) / (bound * 2.0)).clamp(0.0, 1.0);
        let y = ((p.y + bound) / (bound * 2.0)).clamp(0.0, 1.0);
        min + Vec2::new(x * SIZE, (1.0 - y) * SIZE)
    };

    let to_map_size = |w: f32, h: f32| Vec2::new(w, h) / (bound * 2.0) * SIZE;

    for z in game.arena.zones.iter() {
        let tint = if z.team == NO_TEAM {
            DARK_THEME.fallen_boss
        } else {
            team_color(z.team)
        };

        instances.push(rounded_ui_instance(
            to_map(Vec2::new(z.x, z.y)),
            to_map_size(z.w, z.h),
            screen,
            with_alpha(tint, 0.35),
            with_alpha(tint, 0.0),
            0.0,
            2.0,
        ));
    }

    for w in game.arena.walls.iter() {
        instances.push(rounded_ui_instance(
            to_map(Vec2::new(w.x, w.y)),
            to_map_size(w.w, w.h).max(Vec2::splat(1.0)),
            screen,
            with_alpha(DARK_THEME.maze_walls, 0.95),
            with_alpha(DARK_THEME.maze_walls, 0.0),
            0.0,
            0.0,
        ));
    }

    let my_team = game.tank_info.team;

    for p in game.players.iter() {
        if p.dying {
            continue;
        }

        let is_self = Some(p.id) == game.my_player_id;
        let teammate = my_team != NO_TEAM && p.team == my_team;

        if !is_self && !teammate {
            continue;
        }

        let (d, color) = if is_self {
            (SELF_DOT, DARK_THEME.team_green)
        } else {
            (TEAMMATE_DOT, team_color(p.team))
        };

        let dot = to_map(p.render_pos);
        let dot = Vec2::new(
            dot.x.clamp(min.x + 4.0, min.x + SIZE - 4.0),
            dot.y.clamp(min.y + 4.0, min.y + SIZE - 4.0),
        );

        instances.push(rounded_ui_instance(
            dot,
            Vec2::new(d, d),
            screen,
            color,
            DARK_THEME.fill_border,
            1.5,
            d * 0.5,
        ));
    }

    instances
}
