use glam::Vec2;

use crate::{
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, with_alpha},
        scoreboard::{bar_ui_instance, rounded_ui_instance},
    },
    structs::game_state::GameState,
};

const MAP_BOUND: f32 = 2500.0;
const SIZE: f32 = 180.0;
const MARGIN: f32 = 16.0;

const SELF_DOT: f32 = 9.0;
const OTHER_DOT: f32 = 6.0;
const PENTAGON_DOT: f32 = 4.0;

pub fn render_data(game: &GameState, screen: Vec2) -> Vec<EntityInstance> {
    let mut instances = Vec::new();

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
        let x = ((p.x + MAP_BOUND) / (MAP_BOUND * 2.0)).clamp(0.0, 1.0);
        let y = ((p.y + MAP_BOUND) / (MAP_BOUND * 2.0)).clamp(0.0, 1.0);
        let m = min + Vec2::new(x * SIZE, (1.0 - y) * SIZE);
        Vec2::new(
            m.x.clamp(min.x + 4.0, min.x + SIZE - 4.0),
            m.y.clamp(min.y + 4.0, min.y + SIZE - 4.0),
        )
    };

    // CURRENTLY RENDERING PENTAGONS CHANGE LATER TO TEAMMATES AND WALLS!!
    for s in game.shapes.iter() {
        if s.dying || s.sides != 5 {
            continue;
        }
        instances.push(bar_ui_instance(
            to_map(s.render_pos),
            Vec2::new(PENTAGON_DOT, PENTAGON_DOT),
            screen,
            DARK_THEME.pentagon,
        ));
    }

    for p in game.players.iter() {
        if p.dying {
            continue;
        }
        let is_self = Some(p.id) == game.my_player_id;
        let (d, color) = if is_self {
            (SELF_DOT, DARK_THEME.team_green)
        } else {
            (OTHER_DOT, DARK_THEME.scoreboard_text)
        };
        instances.push(bar_ui_instance(
            to_map(p.render_pos),
            Vec2::new(d, d),
            screen,
            color,
        ));
    }

    instances
}
