// this is the stats upgrade panel
// press u to quick toggle to test it out
//
// row layout:
//   - solid stat-coloured bar (dark border + track)
//   - name top-left, level pill top-right
//   - MAX_LEVEL level slots along the bottom strip
//   - "+" button: colour ring + dark face, fills on hover, goes dark grey when
//     nothing can be spent on it
// "x N" unspent points sit in a pill at the top right.
//
// juice: hovering a row brightens its track; call flash(idx)
// right after spending a point for a white pop on the new slot.

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};

use crate::render::{
    buffers::EntityInstance,
    colours::{DARK_THEME, to_glyphon},
    scoreboard::bar_ui_instance,
};

const BAR_W: f32 = 300.0;
const PLUS_W: f32 = 44.0;
const PLUS_GAP: f32 = 6.0;
const ROW_W: f32 = BAR_W + PLUS_GAP + PLUS_W;
const BUTTON_H: f32 = 46.0;
const BUTTON_GAP: f32 = 8.0;
const PANEL_MARGIN: f32 = 16.0;

const OPEN_X: f32 = PANEL_MARGIN;
const CLOSED_X: f32 = -(ROW_W + 40.0);

const SLIDE_SPEED: f32 = 12.0;

const TRIGGER_W: f32 = 0.22;
const TRIGGER_H: f32 = 0.25;

const CLICKABLE_OPEN: f32 = 0.9;
const HOVER_OPEN: f32 = 0.5;

const COUNTER_H: f32 = 26.0;
const COUNTER_GAP: f32 = 6.0;
const COUNTER_FONT: f32 = 18.0;
const COUNTER_LINE: f32 = 22.0;
const COUNTER_PAD: f32 = 10.0;

// text
const LABEL_PAD: f32 = 14.0;
const LABEL_FONT: f32 = 18.0;
const LABEL_LINE: f32 = 22.0;

const NUM_FONT: f32 = 15.0;
const NUM_LINE: f32 = 18.0;
const NUM_PAD: f32 = 14.0;
const PILL: f32 = 26.0;

const PLUS_FONT: f32 = 26.0;
const PLUS_LINE: f32 = 30.0;

// bar internal layout
const INSET: f32 = 3.0;
const TOP_ZONE: f32 = 28.0;
const STRIP_H: f32 = 8.0;
const STRIP_PAD: f32 = 14.0;
const SEG_GAP: f32 = 3.0;

// shading (brightness factors of the stat colour)
const BORDER_F: f32 = 0.24;
const TRACK_F: f32 = 0.40;
const TRACK_HOVER_F: f32 = 0.52;
const SEG_EMPTY_F: f32 = 0.62;
const PILL_F: f32 = 0.30;
const PLUS_INNER_F: f32 = 0.45;

const MAX_LEVEL: u8 = 7;

const LOCKED: [f32; 4] = [0.42, 0.44, 0.48, 1.0];

const FLASH_DECAY: f32 = 4.0;

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

fn darken(color: [f32; 4], factor: f32) -> [f32; 4] {
    [
        color[0] * factor,
        color[1] * factor,
        color[2] * factor,
        color[3],
    ]
}

fn lighten(color: [f32; 4], t: f32) -> [f32; 4] {
    [
        color[0] + (1.0 - color[0]) * t,
        color[1] + (1.0 - color[1]) * t,
        color[2] + (1.0 - color[2]) * t,
        color[3],
    ]
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
    x_mark: Buffer,
    digits: Vec<Buffer>,
    open: f32,
    pinned: bool,
    hover: Option<usize>,
    flashes: [f32; NUM_UPGRADES],
}

impl UpgradePanel {
    pub fn new(fs: &mut glyphon::FontSystem) -> Self {
        let labels = upgrade_defs()
            .iter()
            .map(|(name, _)| make_buffer(fs, Metrics::new(LABEL_FONT, LABEL_LINE), name))
            .collect();
        let numbers = (0..=MAX_LEVEL)
            .map(|n| make_buffer(fs, Metrics::new(NUM_FONT, NUM_LINE), &n.to_string()))
            .collect();
        let plus = make_buffer(fs, Metrics::new(PLUS_FONT, PLUS_LINE), "+");
        let x_mark = make_buffer(fs, Metrics::new(COUNTER_FONT, COUNTER_LINE), "x");
        let digits = (0..10)
            .map(|d| make_buffer(fs, Metrics::new(COUNTER_FONT, COUNTER_LINE), &d.to_string()))
            .collect();
        Self {
            labels,
            numbers,
            plus,
            x_mark,
            digits,
            open: 0.0,
            pinned: false,
            hover: None,
            flashes: [0.0; NUM_UPGRADES],
        }
    }

    pub fn toggle(&mut self) {
        self.pinned = !self.pinned;
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub fn flash(&mut self, idx: usize) {
        if idx < NUM_UPGRADES {
            self.flashes[idx] = 1.0;
        }
    }

    fn header_h() -> f32 {
        COUNTER_H + COUNTER_GAP
    }

    fn rows_h() -> f32 {
        (NUM_UPGRADES as f32) * BUTTON_H + ((NUM_UPGRADES - 1) as f32) * BUTTON_GAP
    }

    fn total_h() -> f32 {
        Self::header_h() + Self::rows_h()
    }

    fn panel_x(&self) -> f32 {
        OPEN_X + (CLOSED_X - OPEN_X) * (1.0 - self.open)
    }

    fn panel_rect(&self, screen: Vec2) -> (Vec2, Vec2) {
        let x = self.panel_x();
        let bottom = screen.y - PANEL_MARGIN;
        let top = bottom - Self::total_h();
        (Vec2::new(x, top), Vec2::new(x + ROW_W, bottom))
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

        for f in &mut self.flashes {
            *f = (*f - dt * FLASH_DECAY).max(0.0);
        }

        self.hover = if self.open > HOVER_OPEN {
            c.and_then(|c| self.row_at(c, screen))
        } else {
            None
        };
    }

    fn row_at(&self, c: Vec2, screen: Vec2) -> Option<usize> {
        let (min, max) = self.panel_rect(screen);
        if c.x < min.x || c.x > max.x || c.y < min.y || c.y > max.y {
            return None;
        }

        let rows_top = min.y + Self::header_h();
        if c.y < rows_top {
            return None;
        }

        let from_top = c.y - rows_top;
        let idx = (from_top / (BUTTON_H + BUTTON_GAP)).floor() as i64;
        let idx = idx.clamp(0, (NUM_UPGRADES - 1) as i64) as usize;
        let within_button = from_top - idx as f32 * (BUTTON_H + BUTTON_GAP) <= BUTTON_H;

        if within_button { Some(idx) } else { None }
    }

    pub fn hit_test(&self, cursor: Vec2, window: Vec2, screen: Vec2) -> Option<usize> {
        if self.open < CLICKABLE_OPEN {
            return None;
        }
        let scale = window_to_screen_scale(window, screen);
        let c = cursor * scale;
        self.row_at(c, screen)
    }

    pub fn render_data(
        &self,
        screen: Vec2,
        levels: &[u8; 8],
        points: u32,
    ) -> (Vec<EntityInstance>, Vec<TextArea<'_>>) {
        let mut instances = Vec::new();
        let mut areas = Vec::new();

        if self.open <= 0.01 {
            return (instances, areas);
        }

        let (min, _) = self.panel_rect(screen);
        let defs = upgrade_defs();

        self.push_counter(&mut instances, &mut areas, min, points, screen);

        let rows_top = min.y + Self::header_h();

        for (i, (_name, stat_color)) in defs.iter().enumerate() {
            let top = rows_top + i as f32 * (BUTTON_H + BUTTON_GAP);
            let cy = top + BUTTON_H * 0.5;
            let bar_left = min.x;

            let level = levels.get(i).copied().unwrap_or(0).min(MAX_LEVEL);
            let afford = points > 0 && level < MAX_LEVEL;
            let hovered = self.hover == Some(i);
            let flash = self.flashes[i];

            let track_f = if hovered { TRACK_HOVER_F } else { TRACK_F } + flash * 0.12;
            instances.push(bar_ui_instance(
                Vec2::new(bar_left + BAR_W * 0.5, cy),
                Vec2::new(BAR_W, BUTTON_H),
                screen,
                darken(*stat_color, BORDER_F),
            ));
            let in_w = BAR_W - INSET * 2.0;
            let in_h = BUTTON_H - INSET * 2.0;
            instances.push(bar_ui_instance(
                Vec2::new(bar_left + INSET + in_w * 0.5, cy),
                Vec2::new(in_w, in_h),
                screen,
                darken(*stat_color, track_f),
            ));

            push_outlined(
                &mut areas,
                &self.labels[i],
                bar_left + LABEL_PAD,
                top + INSET + (TOP_ZONE - LABEL_LINE) * 0.5,
                screen,
            );

            let pill_left = bar_left + BAR_W - NUM_PAD - PILL;
            let pill_top = top + INSET + (TOP_ZONE - PILL) * 0.5;
            instances.push(bar_ui_instance(
                Vec2::new(pill_left + PILL * 0.5, pill_top + PILL * 0.5),
                Vec2::new(PILL, PILL),
                screen,
                lighten(darken(*stat_color, PILL_F), flash * 0.5),
            ));
            let number = &self.numbers[level as usize];
            let num_w = measure(number);
            push_outlined(
                &mut areas,
                number,
                pill_left + (PILL - num_w) * 0.5,
                pill_top + (PILL - NUM_LINE) * 0.5,
                screen,
            );

            let strip_top = top + INSET + TOP_ZONE + 3.0;
            let strip_left = bar_left + STRIP_PAD;
            let strip_w = BAR_W - STRIP_PAD * 2.0;
            let seg_w = (strip_w - SEG_GAP * (MAX_LEVEL - 1) as f32) / MAX_LEVEL as f32;
            for k in 0..MAX_LEVEL {
                let seg_left = strip_left + k as f32 * (seg_w + SEG_GAP);
                let mut seg_color = if k < level {
                    *stat_color
                } else {
                    darken(*stat_color, SEG_EMPTY_F)
                };
                if flash > 0.0 && level > 0 && k == level - 1 {
                    seg_color = lighten(seg_color, flash);
                }
                instances.push(bar_ui_instance(
                    Vec2::new(seg_left + seg_w * 0.5, strip_top + STRIP_H * 0.5),
                    Vec2::new(seg_w, STRIP_H),
                    screen,
                    seg_color,
                ));
            }

            let plus_left = bar_left + BAR_W + PLUS_GAP;
            let (plus_outer, plus_inner) = if afford {
                let inner = if hovered {
                    lighten(*stat_color, flash * 0.4)
                } else {
                    darken(*stat_color, PLUS_INNER_F)
                };
                (*stat_color, inner)
            } else {
                (darken(LOCKED, 0.5), darken(LOCKED, 0.32))
            };
            instances.push(bar_ui_instance(
                Vec2::new(plus_left + PLUS_W * 0.5, cy),
                Vec2::new(PLUS_W, BUTTON_H),
                screen,
                plus_outer,
            ));
            instances.push(bar_ui_instance(
                Vec2::new(plus_left + PLUS_W * 0.5, cy),
                Vec2::new(PLUS_W - INSET * 2.0, BUTTON_H - INSET * 2.0),
                screen,
                plus_inner,
            ));
            let plus_w = measure(&self.plus);
            push_outlined(
                &mut areas,
                &self.plus,
                plus_left + (PLUS_W - plus_w) * 0.5,
                top + (BUTTON_H - PLUS_LINE) * 0.5,
                screen,
            );
        }

        (instances, areas)
    }

    fn push_counter<'a>(
        &'a self,
        instances: &mut Vec<EntityInstance>,
        areas: &mut Vec<TextArea<'a>>,
        min: Vec2,
        points: u32,
        screen: Vec2,
    ) {
        let mut rev_digits: Vec<u8> = Vec::new();
        let mut n = points;
        loop {
            rev_digits.push((n % 10) as u8);
            if n < 10 {
                break;
            }
            n /= 10;
        }

        let x_w = measure(&self.x_mark);
        let text_w = rev_digits
            .iter()
            .rev()
            .fold(x_w, |acc, d| acc + measure(&self.digits[*d as usize]));

        let pill_w = text_w + COUNTER_PAD * 2.0;
        let pill_left = min.x + ROW_W - pill_w;
        let cy = min.y + COUNTER_H * 0.5;

        instances.push(bar_ui_instance(
            Vec2::new(pill_left + pill_w * 0.5, cy),
            Vec2::new(pill_w, COUNTER_H),
            screen,
            DARK_THEME.scoreboard_row,
        ));

        let mut x = pill_left + COUNTER_PAD;
        let top = min.y + (COUNTER_H - COUNTER_LINE) * 0.5;
        push_outlined(areas, &self.x_mark, x, top, screen);
        x += x_w;
        for d in rev_digits.iter().rev() {
            let buffer = &self.digits[*d as usize];
            let w = measure(buffer);
            push_outlined(areas, buffer, x, top, screen);
            x += w;
        }
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
