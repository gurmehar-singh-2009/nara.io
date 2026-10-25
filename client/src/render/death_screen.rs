use glam::Vec2;
use glyphon::{
    Attrs, Buffer, Color, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight,
};
use shared::packets::{KILLER_PENTAGON, KILLER_SQUARE, KILLER_TANK, KILLER_TRIANGLE};

use crate::{
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, PLAYER_BLUE, entity_color, to_glyphon, with_alpha},
        scoreboard::{pill_instance, rounded_ui_instance},
        tank_upgrades::{TankClassDef, icon_scale, push_tank_icon, screen_to_world},
    },
    structs::game_state::DeathInfo,
};

// lwk lazy
// ALSO: these constants are named like this so it will be easier to convert
// when i move everything to config.toml
const DIM_ALPHA: f32 = 0.55;
const DIM_FADE: f32 = 0.6;

const CARD_DELAY: f32 = 0.45;
const CARD_IN: f32 = 0.55;
const CARD_SLIDE: f32 = 60.0;
const CARD_W: f32 = 520.0;
const CARD_H: f32 = 432.0;
const CARD_RADIUS: f32 = 22.0;
const CARD_BORDER: f32 = 3.0;
const CARD_FILL: [f32; 4] = [0.1, 0.1, 0.11, 0.94];
const CARD_EDGE: [f32; 4] = [0.0, 0.0, 0.0, 0.6];
const SHADOW: [f32; 4] = [0.0, 0.0, 0.0, 0.28];
const SHADOW_OFFSET: f32 = 10.0;

const ACCENT_W: f32 = 72.0;
const ACCENT_H: f32 = 5.0;
const ACCENT_Y: f32 = 16.0;

const TITLE_Y: f32 = 34.0;
const ICON_Y: f32 = 104.0;
const ICON_PX: f32 = 54.0;
const ICON_SPIN: f32 = 0.7;
const KILLER_Y: f32 = 150.0;
const KILLER_TANK_Y: f32 = 192.0;
const DIVIDER_Y: f32 = 228.0;
const DIVIDER_FILL: [f32; 4] = [1.0, 1.0, 1.0, 0.1];

const TILES_Y: f32 = 248.0;
const TILE_H: f32 = 72.0;
const TILE_GAP: f32 = 10.0;
const TILE_PAD: f32 = 26.0;
const TILE_FILL: [f32; 4] = [1.0, 1.0, 1.0, 0.06];
const TILE_RADIUS: f32 = 12.0;
const VALUE_TOP: f32 = 10.0;
const LABEL_BOTTOM: f32 = 12.0;

const COUNT_DELAY: f32 = 0.7;
const COUNT_TIME: f32 = 1.1;

const BUTTON_W: f32 = 230.0;
const BUTTON_H: f32 = 48.0;
const BUTTON_BOTTOM: f32 = 50.0;
const BUTTON_EDGE: [f32; 4] = [0.05, 0.2, 0.45, 1.0];
const BUTTON_GROW: f32 = 0.05;
const HOVER_SPEED: f32 = 16.0;
const HINT_GAP: f32 = 10.0;

const ARENA_COLOR: [f32; 4] = [0.75, 0.75, 0.78, 1.0];
const DIM_TEXT: [f32; 4] = [0.68, 0.68, 0.72, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const OUTLINE_PX: f32 = 2.0;

const STAT_LABELS: [&str; 4] = ["SCORE", "LEVEL", "KILLS", "TIME ALIVE"];

fn bold() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif).weight(Weight::BOLD)
}

fn make_buffer(fs: &mut glyphon::FontSystem, size: f32, text: &str) -> Buffer {
    let mut buffer = Buffer::new(fs, Metrics::new(size, size * 1.2));
    buffer.set_text(text, &bold(), Shaping::Basic, None);
    buffer.shape_until_scroll(fs, false);
    buffer
}

fn reshape(fs: &mut glyphon::FontSystem, buffer: &mut Buffer, text: &str) {
    buffer.set_text(text, &bold(), Shaping::Basic, None);
    buffer.shape_until_scroll(fs, false);
}

fn size_of(buffer: &Buffer) -> Vec2 {
    let mut size = Vec2::ZERO;
    for run in buffer.layout_runs() {
        size.x = size.x.max(run.line_w);
        size.y += run.line_height;
    }
    size
}

fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + t * t * (2.7 * t + 1.7)
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn lighten(c: [f32; 4], t: f32) -> [f32; 4] {
    [
        c[0] + (1.0 - c[0]) * t,
        c[1] + (1.0 - c[1]) * t,
        c[2] + (1.0 - c[2]) * t,
        c[3],
    ]
}

fn grouped(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);

    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }

    out
}

fn duration(secs: u32) -> String {
    if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

fn window_to_screen_scale(window: Vec2, screen: Vec2) -> f32 {
    if window.x > 0.0 {
        screen.x / window.x
    } else {
        1.0
    }
}

pub struct DeathScreen {
    info: Option<DeathInfo>,
    icon: Option<TankClassDef>,
    accent: [f32; 4],
    t: f32,
    spin: f32,
    hover: f32,
    title: Buffer,
    killer: Buffer,
    killer_tank: Buffer,
    labels: Vec<Buffer>,
    values: Vec<Buffer>,
    value_text: Vec<String>,
    button: Buffer,
    hint: Buffer,
}

impl DeathScreen {
    pub fn new(fs: &mut glyphon::FontSystem) -> Self {
        Self {
            info: None,
            icon: None,
            accent: ARENA_COLOR,
            t: 0.0,
            spin: 0.0,
            hover: 0.0,
            title: make_buffer(fs, 15.0, "YOU WERE KILLED BY"),
            killer: make_buffer(fs, 32.0, ""),
            killer_tank: make_buffer(fs, 16.0, ""),
            labels: STAT_LABELS
                .iter()
                .map(|label| make_buffer(fs, 12.0, label))
                .collect(),
            values: (0..4).map(|_| make_buffer(fs, 26.0, "0")).collect(),
            value_text: vec![String::new(); 4],
            button: make_buffer(fs, 20.0, "Continue"),
            hint: make_buffer(fs, 13.0, "or press Enter"),
        }
    }

    pub fn is_active(&self) -> bool {
        self.info.is_some()
    }

    pub fn show(&mut self, fs: &mut glyphon::FontSystem, info: DeathInfo) {
        let color = match info.killer_kind {
            KILLER_TANK => entity_color(info.killer_team, false),
            KILLER_SQUARE => DARK_THEME.square,
            KILLER_TRIANGLE => DARK_THEME.triangle,
            KILLER_PENTAGON => DARK_THEME.pentagon,
            _ => ARENA_COLOR,
        };

        let sides = match info.killer_kind {
            KILLER_TANK => info.killer_sides,
            KILLER_SQUARE => 4,
            KILLER_TRIANGLE => 3,
            KILLER_PENTAGON => 5,
            _ => 1,
        };

        reshape(fs, &mut self.killer, &info.killer);
        reshape(fs, &mut self.killer_tank, &info.killer_tank);

        self.icon = Some(TankClassDef {
            name: info.killer.clone(),
            color,
            sides,
            barrels: info.killer_barrels.clone(),
            turrets: Vec::new(),
        });
        self.accent = color;
        self.value_text = vec![String::new(); 4];
        self.t = 0.0;
        self.hover = 0.0;
        self.info = Some(info);
    }

    pub fn hide(&mut self) {
        self.info = None;
        self.icon = None;
    }

    fn card_progress(&self) -> f32 {
        ((self.t - CARD_DELAY) / CARD_IN).clamp(0.0, 1.0)
    }

    fn card_top_left(&self, screen: Vec2) -> Vec2 {
        let slide = (1.0 - ease_out_back(self.card_progress())) * CARD_SLIDE;
        Vec2::new((screen.x - CARD_W) * 0.5, (screen.y - CARD_H) * 0.5 + slide)
    }

    fn button_rect(&self, screen: Vec2) -> (Vec2, Vec2) {
        let top_left = self.card_top_left(screen);
        let center = Vec2::new(
            top_left.x + CARD_W * 0.5,
            top_left.y + CARD_H - BUTTON_BOTTOM - BUTTON_H * 0.5,
        );
        let half = Vec2::new(BUTTON_W, BUTTON_H) * 0.5;
        (center - half, center + half)
    }

    pub fn hit_continue(&self, cursor: Vec2, window: Vec2, screen: Vec2) -> bool {
        if !self.is_active() || self.card_progress() < 0.8 {
            return false;
        }

        let c = cursor * window_to_screen_scale(window, screen);
        let (min, max) = self.button_rect(screen);
        c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y
    }

    pub fn tick(
        &mut self,
        fs: &mut glyphon::FontSystem,
        dt: f32,
        cursor: Option<Vec2>,
        window: Vec2,
        screen: Vec2,
    ) {
        let Some(info) = &self.info else {
            return;
        };

        self.t += dt;
        self.spin += dt * ICON_SPIN;

        let progress = ease_out_cubic((self.t - COUNT_DELAY) / COUNT_TIME);
        let counted = |v: u32| (v as f32 * progress).round() as u32;

        let texts = [
            grouped(counted(info.score)),
            counted(info.level).max(1).to_string(),
            counted(info.kills).to_string(),
            duration(counted(info.time_alive)),
        ];

        for (i, text) in texts.into_iter().enumerate() {
            if self.value_text[i] != text {
                reshape(fs, &mut self.values[i], &text);
                self.value_text[i] = text;
            }
        }

        let hovered = cursor.is_some_and(|c| self.hit_continue(c, window, screen));
        let target = if hovered { 1.0 } else { 0.0 };
        self.hover += (target - self.hover) * (1.0 - (-HOVER_SPEED * dt).exp());
    }

    pub fn render_data(
        &self,
        screen: Vec2,
        camera_pos: [f32; 2],
        zoom: f32,
    ) -> (Vec<EntityInstance>, Vec<TextArea<'_>>) {
        let mut instances = Vec::new();
        let mut areas = Vec::new();

        if self.info.is_none() {
            return (instances, areas);
        }

        let dim = (self.t / DIM_FADE).clamp(0.0, 1.0) * DIM_ALPHA;

        instances.push(rounded_ui_instance(
            screen * 0.5,
            screen * 1.1,
            screen,
            [0.0, 0.0, 0.0, dim],
            [0.0, 0.0, 0.0, 0.0],
            0.0,
            0.0,
        ));

        let alpha = ease_out_cubic(self.card_progress());
        if alpha <= 0.01 {
            return (instances, areas);
        }

        let top_left = self.card_top_left(screen);
        let cx = top_left.x + CARD_W * 0.5;
        let card_center = top_left + Vec2::new(CARD_W, CARD_H) * 0.5;

        instances.push(rounded_ui_instance(
            card_center + Vec2::new(0.0, SHADOW_OFFSET),
            Vec2::new(CARD_W, CARD_H),
            screen,
            with_alpha(SHADOW, alpha),
            with_alpha(SHADOW, 0.0),
            0.0,
            CARD_RADIUS,
        ));

        instances.push(rounded_ui_instance(
            card_center,
            Vec2::new(CARD_W, CARD_H),
            screen,
            with_alpha(CARD_FILL, alpha),
            with_alpha(CARD_EDGE, alpha),
            CARD_BORDER,
            CARD_RADIUS,
        ));

        instances.push(pill_instance(
            Vec2::new(cx, top_left.y + ACCENT_Y),
            Vec2::new(ACCENT_W, ACCENT_H),
            screen,
            with_alpha(self.accent, alpha),
            with_alpha(self.accent, 0.0),
            0.0,
        ));

        instances.push(pill_instance(
            Vec2::new(cx, top_left.y + DIVIDER_Y),
            Vec2::new(CARD_W - TILE_PAD * 2.0, 2.0),
            screen,
            with_alpha(DIVIDER_FILL, alpha),
            with_alpha(DIVIDER_FILL, 0.0),
            0.0,
        ));

        let tile_w = (CARD_W - TILE_PAD * 2.0 - TILE_GAP * 3.0) / 4.0;

        for i in 0..4 {
            let tile_cx = top_left.x + TILE_PAD + tile_w * 0.5 + i as f32 * (tile_w + TILE_GAP);
            let tile_cy = top_left.y + TILES_Y + TILE_H * 0.5;

            instances.push(rounded_ui_instance(
                Vec2::new(tile_cx, tile_cy),
                Vec2::new(tile_w, TILE_H),
                screen,
                with_alpha(TILE_FILL, alpha),
                with_alpha(TILE_FILL, 0.0),
                0.0,
                TILE_RADIUS,
            ));

            push_centered(
                &mut areas,
                &self.values[i],
                tile_cx,
                top_left.y + TILES_Y + VALUE_TOP,
                screen,
                with_alpha(WHITE, alpha),
            );

            let label_h = size_of(&self.labels[i]).y;
            push_centered(
                &mut areas,
                &self.labels[i],
                tile_cx,
                top_left.y + TILES_Y + TILE_H - LABEL_BOTTOM - label_h,
                screen,
                with_alpha(DIM_TEXT, alpha),
            );
        }

        let (button_min, button_max) = self.button_rect(screen);
        let button_center = (button_min + button_max) * 0.5;
        let grow = 1.0 + self.hover * BUTTON_GROW;

        instances.push(pill_instance(
            button_center,
            Vec2::new(BUTTON_W, BUTTON_H) * grow,
            screen,
            with_alpha(lighten(PLAYER_BLUE, self.hover * 0.15), alpha),
            with_alpha(BUTTON_EDGE, alpha),
            CARD_BORDER,
        ));

        let button_h = size_of(&self.button).y;
        push_centered(
            &mut areas,
            &self.button,
            cx,
            button_center.y - button_h * 0.5,
            screen,
            with_alpha(WHITE, alpha),
        );

        push_centered(
            &mut areas,
            &self.hint,
            cx,
            button_max.y + HINT_GAP,
            screen,
            with_alpha(DIM_TEXT, alpha),
        );

        push_centered(
            &mut areas,
            &self.title,
            cx,
            top_left.y + TITLE_Y,
            screen,
            with_alpha(DIM_TEXT, alpha),
        );

        let killer_h = size_of(&self.killer).y;
        push_centered(
            &mut areas,
            &self.killer,
            cx,
            top_left.y + KILLER_Y + (40.0 - killer_h) * 0.5,
            screen,
            with_alpha(self.accent, alpha),
        );

        push_centered(
            &mut areas,
            &self.killer_tank,
            cx,
            top_left.y + KILLER_TANK_Y,
            screen,
            with_alpha(DIM_TEXT, alpha),
        );

        if let Some(icon) = &self.icon {
            let (world_pos, _) =
                screen_to_world(Vec2::new(cx, top_left.y + ICON_Y), screen, camera_pos, zoom);

            push_tank_icon(
                &mut instances,
                icon,
                world_pos,
                self.spin,
                icon_scale(ICON_PX, screen, zoom),
                alpha,
            );
        }

        (instances, areas)
    }
}

fn push_centered<'a>(
    areas: &mut Vec<TextArea<'a>>,
    buffer: &'a Buffer,
    cx: f32,
    top: f32,
    screen: Vec2,
    color: [f32; 4],
) {
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: screen.x as i32,
        bottom: screen.y as i32,
    };
    let left = cx - size_of(buffer).x * 0.5;
    let outline = to_glyphon(with_alpha(DARK_THEME.fill_border, color[3]));
    let fill: Color = to_glyphon(color);

    const DIRS: [(f32, f32); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (1.0, 1.0),
        (1.0, -1.0),
        (-1.0, 1.0),
        (-1.0, -1.0),
    ];

    for (dx, dy) in DIRS {
        areas.push(TextArea {
            buffer,
            left: left + dx * OUTLINE_PX,
            top: top + dy * OUTLINE_PX,
            scale: 1.0,
            bounds,
            default_color: outline,
            custom_glyphs: &[],
        });
    }

    areas.push(TextArea {
        buffer,
        left,
        top,
        scale: 1.0,
        bounds,
        default_color: fill,
        custom_glyphs: &[],
    });
}
