pub type Color = [f32; 4];

#[inline]
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    [color[0], color[1], color[2], color[3] * alpha]
}

#[inline]
pub fn to_glyphon(color: Color) -> glyphon::Color {
    glyphon::Color::rgba(
        (color[0] * 255.0) as u8,
        (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8,
        (color[3] * 255.0) as u8,
    )
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

const TANK_BODY: Color = [0.0, 0.698, 0.882, 1.0]; // #00B2E1
const TANK_OUTLINE: Color = [0.0, 0.520, 0.657, 1.0]; // #0085A8 (fill x 0.75)
const BARREL_FILL: Color = [0.600, 0.600, 0.600, 1.0]; // #999999
const BARREL_EDGE: Color = [0.447, 0.447, 0.447, 1.0]; // #727272 (fill x 0.75)

const TEAM_RED: Color = [0.988, 0.463, 0.467, 1.0]; // #FC7677
const TEAM_PURPLE: Color = [0.945, 0.467, 0.867, 1.0]; // #F177DD
const TEAM_GREEN: Color = [0.0, 0.882, 0.431, 1.0]; // #00E16E

const HEALTH_BG: Color = [0.149, 0.149, 0.149, 1.0]; // #262626
const HEALTH_FG: Color = [0.522, 0.890, 0.490, 1.0]; // #85E37D

const XP_FILL: Color = [1.0, 0.871, 0.263, 1.0]; // #FFDE43

const SQUARE: Color = [1.0, 0.910, 0.412, 1.0]; // #FFE869
const PENTAGON: Color = [0.463, 0.553, 1.0, 1.0]; // #768DFF
const FALLEN: Color = [0.549, 0.549, 0.549, 1.0]; // #8C8C8C

// stat upgrade panel colours (one distinct hue per stat)
const STAT_REGEN: Color = [0.945, 0.467, 0.867, 1.0]; // pink
const STAT_MAX_HEALTH: Color = [0.580, 0.360, 0.910, 1.0]; // purple
const STAT_BODY: Color = [0.988, 0.463, 0.467, 1.0]; // red
const STAT_B_SPEED: Color = [0.290, 0.500, 1.000, 1.0]; // blue
const STAT_PEN: Color = [1.000, 0.871, 0.263, 1.0]; // yellow
const STAT_B_DMG: Color = [1.000, 0.490, 0.300, 1.0]; // orange-red
const STAT_RELOAD: Color = [0.720, 0.890, 0.310, 1.0]; // yellow-green
const STAT_MOVE: Color = [0.320, 0.870, 0.930, 1.0]; // cyan

pub struct DiepTheme {
    pub background: Color,
    pub border: Color,
    pub border_alpha: f32,
    pub fill_borders: bool,

    pub grid: Color,
    pub grid_alpha: f32,

    pub tank_body: Color,
    pub tank_outline: Color,
    pub barrel: Color,
    pub barrel_outline: Color,
    pub bullet: Color,

    pub team_blue: Color,
    pub team_red: Color,
    pub team_purple: Color,
    pub team_green: Color,

    pub health_bar_background: Color,
    pub health_bar_foreground: Color,

    pub xp_bar_fill: Color,
    pub score_bar_fill: Color,
    pub bar_background: Color,
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

    pub scoreboard_row: Color,
    pub scoreboard_row_border: Color,
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
            background: [0.020, 0.020, 0.020, 1.0], // #050505
            border: [0.0, 0.0, 0.0, 1.0],
            border_alpha: 0.35,
            fill_borders: false,              // flat black outlines in dark mode
            grid: [0.078, 0.078, 0.078, 1.0], // #141414
            grid_alpha: 1.0,
            maze_walls: [0.078, 0.078, 0.078, 1.0],
            map_outside: [0.157, 0.157, 0.157, 0.25], // #282828 grey
            minimap_border: [0.078, 0.078, 0.078, 1.0],
            minimap_background: [0.157, 0.157, 0.157, 1.0],

            bar_background: [0.130, 0.130, 0.130, 0.85],
            scoreboard_row: [0.130, 0.130, 0.130, 0.85],
            scoreboard_row_border: [0.040, 0.040, 0.040, 0.90],
            scoreboard_title: [1.0, 1.0, 1.0, 1.0],
            scoreboard_text: [1.0, 1.0, 1.0, 1.0],
            score_text: [0.900, 0.920, 0.950, 1.0],
            fill_border: [0.0, 0.0, 0.0, 1.0],

            tank_body: TANK_BODY,
            tank_outline: TANK_OUTLINE,
            barrel: BARREL_FILL,
            barrel_outline: BARREL_EDGE,
            bullet: TANK_BODY,
            team_blue: TANK_BODY,
            team_red: TEAM_RED,
            team_purple: TEAM_PURPLE,
            team_green: TEAM_GREEN,
            health_bar_background: HEALTH_BG,
            health_bar_foreground: HEALTH_FG,
            xp_bar_fill: XP_FILL,
            score_bar_fill: TEAM_GREEN,
            square: SQUARE,
            triangle: TEAM_RED,
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
            fill_borders: true,               // every outline = fill darkened ~75%
            grid: [0.722, 0.722, 0.722, 1.0], // #B8B8B8 (baked, subtle)
            grid_alpha: 1.0,
            maze_walls: [0.733, 0.733, 0.733, 1.0],
            map_outside: [0.647, 0.647, 0.647, 0.25], // #A5A5A5 grey
            minimap_background: [0.804, 0.804, 0.804, 0.90],
            minimap_border: [0.733, 0.733, 0.733, 1.0],

            bar_background: [0.310, 0.310, 0.310, 0.85],
            scoreboard_row: [0.310, 0.310, 0.310, 0.85],
            scoreboard_row_border: [0.180, 0.180, 0.180, 0.90],
            scoreboard_title: [1.0, 1.0, 1.0, 1.0],
            scoreboard_text: [1.0, 1.0, 1.0, 1.0],
            score_text: [0.0, 0.0, 0.0, 1.0],
            fill_border: [0.0, 0.0, 0.0, 1.0],

            tank_body: TANK_BODY,
            tank_outline: TANK_OUTLINE,
            barrel: BARREL_FILL,
            barrel_outline: BARREL_EDGE,
            bullet: TANK_BODY,
            team_blue: TANK_BODY,
            team_red: TEAM_RED,
            team_purple: TEAM_PURPLE,
            team_green: TEAM_GREEN,
            health_bar_background: HEALTH_BG,
            health_bar_foreground: HEALTH_FG,
            xp_bar_fill: XP_FILL,
            score_bar_fill: TEAM_GREEN,
            square: SQUARE,
            triangle: TEAM_RED,
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

    pub const fn active() -> DiepTheme {
        if LIGHT_MODE {
            DiepTheme::light()
        } else {
            DiepTheme::dark()
        }
    }
}

pub const LIGHT_MODE: bool = true;

pub const DARK_THEME: DiepTheme = DiepTheme::active();
pub const DEFAULT_THEME: DiepTheme = DiepTheme::active();
