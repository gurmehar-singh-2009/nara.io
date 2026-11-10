use std::sync::atomic::{AtomicBool, Ordering};

use shared::packets::NO_TEAM;

pub type Color = [f32; 4];

#[inline]
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    [color[0], color[1], color[2], color[3] * alpha]
}

#[inline]
pub fn darken(color: Color, factor: f32) -> Color {
    [
        color[0] * factor,
        color[1] * factor,
        color[2] * factor,
        color[3],
    ]
}

const BARREL_FILL: Color = [0.600, 0.600, 0.600, 1.0];

/// Our blue. Used for team 0, your own tank in FFA, and anything that should match it.
pub const PLAYER_BLUE: Color = [0.22, 0.55, 0.98, 1.0];
const TEAM_RED: Color = [0.945, 0.306, 0.329, 1.0];
const TEAM_PURPLE: Color = [0.945, 0.467, 0.867, 1.0];
const TEAM_GREEN: Color = [0.0, 0.882, 0.431, 1.0];

const HEALTH_BG: Color = [0.149, 0.149, 0.149, 1.0];
const HEALTH_FG: Color = [0.557, 1.0, 0.376, 1.0];
/// Same rim light mode gets from `outline_for` (fill * 0.75), used in both themes so the
/// bar never gets dark mode's flat black outline.
const HEALTH_RIM: Color = [0.418, 0.750, 0.282, 1.0];

const XP_FILL: Color = [1.0, 0.871, 0.263, 1.0];

const SQUARE: Color = [1.0, 0.910, 0.412, 1.0];
const TRIANGLE: Color = [0.988, 0.463, 0.467, 1.0];
const PENTAGON: Color = [0.463, 0.553, 1.0, 1.0];
const FALLEN: Color = [0.549, 0.549, 0.549, 1.0];

const STAT_REGEN: Color = [0.933, 0.714, 0.565, 1.0]; // #EEB690
const STAT_MAX_HEALTH: Color = [0.925, 0.424, 0.941, 1.0]; // #EC6CF0
const STAT_BODY: Color = [0.604, 0.424, 0.941, 1.0]; // #9A6CF0
const STAT_B_SPEED: Color = [0.424, 0.588, 0.941, 1.0]; // #6C96F0
const STAT_PEN: Color = [0.941, 0.851, 0.424, 1.0]; // #F0D96C
const STAT_B_DMG: Color = [0.941, 0.424, 0.424, 1.0]; // #F06C6C
const STAT_RELOAD: Color = [0.596, 0.941, 0.424, 1.0]; // #98F06C
const STAT_MOVE: Color = [0.424, 0.941, 0.925, 1.0]; // #6CF0EC

pub struct DiepTheme {
    pub background: Color,
    pub border: Color,
    pub border_alpha: f32,
    pub fill_borders: bool,

    pub grid: Color,
    pub grid_alpha: f32,

    pub barrel: Color,

    pub team_blue: Color,
    pub team_red: Color,
    pub team_purple: Color,
    pub team_green: Color,

    pub health_bar_background: Color,
    pub health_bar_foreground: Color,
    pub health_bar_border: Color,

    pub xp_bar_fill: Color,
    pub score_bar_fill: Color,
    /// Track behind every bar and the fill of every pill/box in the HUD.
    pub bar_background: Color,
    /// Edge of every bar, pill and HUD box.
    pub bar_border: Color,
    /// Secondary panel fill: chat rows, active chat tab, panel rims.
    pub panel_background: Color,
    /// Text outline.
    pub fill_border: Color,

    pub stat_regen: Color,
    pub stat_max_health: Color,
    pub stat_body_damage: Color,
    pub stat_bullet_speed: Color,
    pub stat_penetration: Color,
    pub stat_bullet_damage: Color,
    pub stat_reload: Color,
    pub stat_movement: Color,

    pub minimap_background: Color,
    pub minimap_border: Color,

    pub score_text: Color,

    pub square: Color,
    pub triangle: Color,
    pub pentagon: Color,
    pub crashers: Color,

    pub arena_closer: Color,
    pub maze_walls: Color,
    pub map_outside: Color,
    pub fallen_boss: Color,

    pub scoreboard_title: Color,
    pub scoreboard_text: Color,
}

impl DiepTheme {
    pub fn outline_for(&self, fill: Color) -> Color {
        if self.fill_borders {
            darken(fill, 0.75)
        } else {
            self.border
        }
    }

    pub const fn dark() -> Self {
        Self {
            background: [0.020, 0.020, 0.020, 1.0],
            border: [0.0, 0.0, 0.0, 1.0],
            border_alpha: 0.35,
            fill_borders: false,
            grid: [0.078, 0.078, 0.078, 1.0], // #141414
            grid_alpha: 1.0,
            maze_walls: [0.078, 0.078, 0.078, 1.0],
            map_outside: [0.157, 0.157, 0.157, 0.25], // #282828
            minimap_border: [0.078, 0.078, 0.078, 1.0],
            minimap_background: [0.157, 0.157, 0.157, 1.0],

            bar_background: [0.130, 0.130, 0.130, 0.88],
            bar_border: [0.040, 0.040, 0.040, 0.95],
            panel_background: [0.200, 0.200, 0.200, 0.85],
            scoreboard_title: [1.0, 1.0, 1.0, 1.0],
            scoreboard_text: [1.0, 1.0, 1.0, 1.0],
            score_text: [0.900, 0.920, 0.950, 1.0],
            fill_border: [0.0, 0.0, 0.0, 1.0],

            barrel: BARREL_FILL,
            team_blue: PLAYER_BLUE,
            team_red: TEAM_RED,
            team_purple: TEAM_PURPLE,
            team_green: TEAM_GREEN,
            health_bar_background: [0.240, 0.240, 0.240, 1.0],
            health_bar_foreground: HEALTH_FG,
            health_bar_border: HEALTH_RIM,
            xp_bar_fill: XP_FILL,
            score_bar_fill: TEAM_GREEN,
            square: SQUARE,
            triangle: TRIANGLE,
            pentagon: PENTAGON,
            crashers: TEAM_PURPLE,
            arena_closer: SQUARE,
            fallen_boss: FALLEN,

            stat_regen: STAT_REGEN,
            stat_max_health: STAT_MAX_HEALTH,
            stat_body_damage: STAT_BODY,
            stat_bullet_speed: STAT_B_SPEED,
            stat_penetration: STAT_PEN,
            stat_bullet_damage: STAT_B_DMG,
            stat_reload: STAT_RELOAD,
            stat_movement: STAT_MOVE,
        }
    }

    pub const fn light() -> Self {
        Self {
            background: [0.804, 0.804, 0.804, 1.0], // #CDCDCD
            border: [0.0, 0.0, 0.0, 1.0],
            border_alpha: 0.10,
            fill_borders: true,
            grid: [0.722, 0.722, 0.722, 1.0], // #B8B8B8
            grid_alpha: 1.0,
            maze_walls: [0.733, 0.733, 0.733, 1.0],
            map_outside: [0.647, 0.647, 0.647, 0.25], // #A5A5A5
            minimap_background: [0.804, 0.804, 0.804, 0.90],
            minimap_border: [0.733, 0.733, 0.733, 1.0],

            bar_background: [0.240, 0.240, 0.240, 0.88],
            bar_border: [0.100, 0.100, 0.100, 0.95],
            panel_background: [0.330, 0.330, 0.330, 0.85],
            scoreboard_title: [1.0, 1.0, 1.0, 1.0],
            scoreboard_text: [1.0, 1.0, 1.0, 1.0],
            score_text: [0.0, 0.0, 0.0, 1.0],
            fill_border: [0.0, 0.0, 0.0, 1.0],

            barrel: BARREL_FILL,
            team_blue: PLAYER_BLUE,
            team_red: TEAM_RED,
            team_purple: TEAM_PURPLE,
            team_green: TEAM_GREEN,
            health_bar_background: HEALTH_BG,
            health_bar_foreground: HEALTH_FG,
            health_bar_border: HEALTH_RIM,
            xp_bar_fill: XP_FILL,
            score_bar_fill: TEAM_GREEN,
            square: SQUARE,
            triangle: TRIANGLE,
            pentagon: PENTAGON,
            crashers: TEAM_PURPLE,
            arena_closer: SQUARE,
            fallen_boss: FALLEN,

            stat_regen: STAT_REGEN,
            stat_max_health: STAT_MAX_HEALTH,
            stat_body_damage: STAT_BODY,
            stat_bullet_speed: STAT_B_SPEED,
            stat_penetration: STAT_PEN,
            stat_bullet_damage: STAT_B_DMG,
            stat_reload: STAT_RELOAD,
            stat_movement: STAT_MOVE,
        }
    }

}

static DARK: DiepTheme = DiepTheme::dark();
static LIGHT: DiepTheme = DiepTheme::light();
static LIGHT_MODE: AtomicBool = AtomicBool::new(false);

/// The theme picked in the menu. Read it every frame; it can change while playing.
#[inline]
pub fn theme() -> &'static DiepTheme {
    if LIGHT_MODE.load(Ordering::Relaxed) {
        &LIGHT
    } else {
        &DARK
    }
}

pub fn set_light_mode(light: bool) {
    LIGHT_MODE.store(light, Ordering::Relaxed);
}

pub fn team_color(team: u8) -> Color {
    let t = theme();

    match team {
        0 => t.team_blue,
        1 => t.team_red,
        2 => t.team_green,
        3 => t.team_purple,
        _ => t.fallen_boss,
    }
}

pub fn entity_color(team: u8, is_mine: bool) -> Color {
    if team != NO_TEAM {
        team_color(team)
    } else if is_mine {
        theme().team_blue
    } else {
        theme().team_red
    }
}
