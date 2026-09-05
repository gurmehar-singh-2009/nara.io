// this is the stats upgrade panel
// press u to quick toggle to test it out

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};

use crate::render::{
    buffers::EntityInstance,
    colours::{DARK_THEME, to_glyphon},
    scoreboard::bar_ui_instance,
};

const BUTTON_W: f32 = 350.0;
const BUTTON_H: f32 = 44.0;
const BUTTON_GAP: f32 = 8.0;
const PANEL_MARGIN: f32 = 16.0;

const OPEN_X: f32 = PANEL_MARGIN;
const CLOSED_X: f32 = -(BUTTON_W + 40.0);

const SLIDE_SPEED: f32 = 12.0;

const TRIGGER_W: f32 = 0.22;
const TRIGGER_H: f32 = 0.25;

const CLICKABLE_OPEN: f32 = 0.9;

const LABEL_PAD: f32 = 14.0;
const LABEL_FONT: f32 = 18.0;
const LABEL_LINE: f32 = 22.0;

const TRACK_W: f32 = 90.0;
const TRACK_H: f32 = 26.0;
const TRACK_RIGHT: f32 = 274.0;

const NUM_PILL: f32 = 24.0;
const NUM_FONT: f32 = 15.0;
const NUM_LINE: f32 = 19.0;
const NUM_RIGHT: f32 = 306.0;

const PLUS_PILL: f32 = 30.0;
const PLUS_FONT: f32 = 22.0;
const PLUS_LINE: f32 = 26.0;
const PLUS_RIGHT: f32 = 338.0;

const MAX_LEVEL: f32 = 7.0;

const TEXT_OUTLINE_PX: f32 = 1.5;

const NUM_UPGRADES: usize = 8;

fn bold() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif).weight(Weight::BOLD)
}

fn make_buffer(fs: &mut glyphon::FontSystem, metrics: Metrics, text: &str) -> Buffer {
    let mut buffer = Buffer::new(fs, metrics);
    buffer.set_text(text, &bold(), Shaping::Basic, None);
    buffer.shape_until_scroll(fs, false);
    buffer
}

fn measure(buffer: &Buffer) -> f32 {
    buffer.layout_runs().next().map(|r| r.line_w).unwrap_or(0.0)
}

fn upgrade_defs() -> [(&'static str, [f32; 4]); NUM_UPGRADES] {
    let t = &DARK_THEME;
    [
        ("Health Regen", t.health_bar_foreground),
        ("Max Health", t.team_purple),
        ("Body Damage", t.team_red),
        ("Bullet Speed", t.team_blue),
        ("Bullet Penetration", t.pentagon),
        ("Bullet Damage", t.xp_bar_fill),
        ("Reload", t.barrel),
        ("Movement Speed", t.score_bar_fill),
    ]
}

fn window_to_screen_scale(window: Vec2, screen: Vec2) -> f32 {
    if window.x > 0.0 {
        screen.x / window.x
    } else {
        1.0
    }
}

pub struct UpgradePanel {
    labels: Vec<Buffer>,
    numbers: Vec<Buffer>,
    plus: Buffer,
    open: f32,
    pinned: bool,
}

impl UpgradePanel {
    pub fn new(fs: &mut glyphon::FontSystem) -> Self {
        let labels = upgrade_defs()
            .iter()
            .map(|(name, _)| make_buffer(fs, Metrics::new(LABEL_FONT, LABEL_LINE), name))
            .collect();
        let numbers = (1..=NUM_UPGRADES)
            .map(|n| make_buffer(fs, Metrics::new(NUM_FONT, NUM_LINE), &n.to_string()))
            .collect();
        let plus = make_buffer(fs, Metrics::new(PLUS_FONT, PLUS_LINE), "+");
        Self {
            labels,
            numbers,
            plus,
            open: 0.0,
            pinned: false,
        }
    }

    pub fn toggle(&mut self) {
        self.pinned = !self.pinned;
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    fn total_h() -> f32 {
        (NUM_UPGRADES as f32) * BUTTON_H + ((NUM_UPGRADES - 1) as f32) * BUTTON_GAP
    }

    fn panel_x(&self) -> f32 {
        OPEN_X + (CLOSED_X - OPEN_X) * (1.0 - self.open)
    }

    fn panel_rect(&self, screen: Vec2) -> (Vec2, Vec2) {
        let x = self.panel_x();
        let bottom = screen.y - PANEL_MARGIN;
        let top = bottom - Self::total_h();
        (Vec2::new(x, top), Vec2::new(x + BUTTON_W, bottom))
    }

    pub fn tick(&mut self, dt: f32, cursor: Option<Vec2>, window: Vec2, screen: Vec2) {
        let scale = window_to_screen_scale(window, screen);
        let c = cursor.map(|c| c * scale);

        let target = if self.pinned {
            1.0
        } else {
            let in_trigger = c.map_or(false, |c| {
                c.x >= 0.0
                    && c.x < screen.x * TRIGGER_W
                    && c.y > screen.y * (1.0 - TRIGGER_H)
                    && c.y <= screen.y
            });
            let (min, max) = self.panel_rect(screen);
            let hovering = c.map_or(false, |c| {
                c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y
            });

            if in_trigger || hovering { 1.0 } else { 0.0 }
        };

        let t = 1.0 - (-SLIDE_SPEED * dt).exp();
        self.open += (target - self.open) * t;
        if (self.open - target).abs() < 0.002 {
            self.open = target;
        }
    }

    pub fn hit_test(&self, cursor: Vec2, window: Vec2, screen: Vec2) -> Option<usize> {
        if self.open < CLICKABLE_OPEN {
            return None;
        }
        let scale = window_to_screen_scale(window, screen);
        let c = cursor * scale;

        let (min, max) = self.panel_rect(screen);
        if c.x < min.x || c.x > max.x || c.y < min.y || c.y > max.y {
            return None;
        }

        let from_top = c.y - min.y;
        let idx = (from_top / (BUTTON_H + BUTTON_GAP)).floor() as i64;
        let idx = idx.clamp(0, (NUM_UPGRADES - 1) as i64) as usize;
        let within_button = from_top - idx as f32 * (BUTTON_H + BUTTON_GAP) <= BUTTON_H;

        if within_button { Some(idx) } else { None }
    }

    pub fn render_data(
        &self,
        screen: Vec2,
        levels: &[u8; 8],
    ) -> (Vec<EntityInstance>, Vec<TextArea<'_>>) {
        let mut instances = Vec::new();
        let mut areas = Vec::new();

        if self.open <= 0.01 {
            return (instances, areas);
        }

        let (min, _) = self.panel_rect(screen);
        let defs = upgrade_defs();

        for (i, (_name, color)) in defs.iter().enumerate() {
            let top = min.y + i as f32 * (BUTTON_H + BUTTON_GAP);
            let cy = top + BUTTON_H * 0.5;
            let left = min.x;

            instances.push(bar_ui_instance(
                Vec2::new(left + BUTTON_W * 0.5, cy),
                Vec2::new(BUTTON_W, BUTTON_H),
                screen,
                DARK_THEME.bar_background,
            ));

            push_outlined(
                &mut areas,
                &self.labels[i],
                left + LABEL_PAD,
                top + (BUTTON_H - LABEL_LINE) * 0.5,
                screen,
            );

            let track_left = left + TRACK_RIGHT - TRACK_W;
            instances.push(bar_ui_instance(
                Vec2::new(track_left + TRACK_W * 0.5, cy),
                Vec2::new(TRACK_W, TRACK_H),
                screen,
                DARK_THEME.scoreboard_row,
            ));
            let level = levels.get(i).copied().unwrap_or(0) as f32;
            if level > 0.0 {
                let frac = (level / MAX_LEVEL).clamp(0.0, 1.0);
                let fill_w = (TRACK_W * frac).max(TRACK_H);
                instances.push(bar_ui_instance(
                    Vec2::new(track_left + fill_w * 0.5, cy),
                    Vec2::new(fill_w, TRACK_H),
                    screen,
                    *color,
                ));
            }

            let num_left = left + NUM_RIGHT - NUM_PILL;
            instances.push(bar_ui_instance(
                Vec2::new(num_left + NUM_PILL * 0.5, cy),
                Vec2::new(NUM_PILL, NUM_PILL),
                screen,
                DARK_THEME.bar_background,
            ));
            let num_w = measure(&self.numbers[i]);
            push_outlined(
                &mut areas,
                &self.numbers[i],
                num_left + (NUM_PILL - num_w) * 0.5,
                top + (BUTTON_H - NUM_LINE) * 0.5,
                screen,
            );

            let plus_left = left + PLUS_RIGHT - PLUS_PILL;
            instances.push(bar_ui_instance(
                Vec2::new(plus_left + PLUS_PILL * 0.5, cy),
                Vec2::new(PLUS_PILL, PLUS_PILL),
                screen,
                *color,
            ));
            let plus_w = measure(&self.plus);
            push_outlined(
                &mut areas,
                &self.plus,
                plus_left + (PLUS_PILL - plus_w) * 0.5,
                top + (BUTTON_H - PLUS_LINE) * 0.5,
                screen,
            );
        }

        (instances, areas)
    }
}

fn push_outlined<'a>(
    areas: &mut Vec<TextArea<'a>>,
    buffer: &'a Buffer,
    left: f32,
    top: f32,
    screen: Vec2,
) {
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: screen.x as i32,
        bottom: screen.y as i32,
    };
    let outline_color = to_glyphon(DARK_THEME.fill_border);
    let fill_color = to_glyphon([1.0, 1.0, 1.0, 1.0]);

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
            left: left + dx * TEXT_OUTLINE_PX,
            top: top + dy * TEXT_OUTLINE_PX,
            scale: 1.0,
            bounds,
            default_color: outline_color,
            custom_glyphs: &[],
        });
    }

    areas.push(TextArea {
        buffer,
        left,
        top,
        scale: 1.0,
        bounds,
        default_color: fill_color,
        custom_glyphs: &[],
    });
}
