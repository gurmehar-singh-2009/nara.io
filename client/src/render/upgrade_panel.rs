// this is the stats upgrade panel
// press u to quick toggle to test it out

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};

use crate::render::{
    buffers::EntityInstance,
    colours::{DARK_THEME, to_glyphon, with_alpha},
    scoreboard::pill_instance,
};

const NUM_UPGRADES: usize = 8;

const BAR_W: f32 = 270.0;
const BAR_H: f32 = 30.0;
const ROW_GAP: f32 = 6.0;
const PLUS_D: f32 = 30.0;
const PLUS_GAP: f32 = 8.0;
const ROW_W: f32 = BAR_W + PLUS_GAP + PLUS_D;
const PANEL_MARGIN: f32 = 16.0;
const BOTTOM_MARGIN: f32 = 24.0;

const OUTLINE: f32 = 4.0;
const SEG_GAP: f32 = 2.0;

const OPEN_X: f32 = PANEL_MARGIN;
const CLOSED_X: f32 = -(ROW_W + 40.0);
const SLIDE_SPEED: f32 = 12.0;
const ROW_SPEED: f32 = 14.0;

const TRIGGER_W: f32 = 0.22;
const TRIGGER_H: f32 = 0.35;
const CLICKABLE_OPEN: f32 = 0.9;
const HOVER_OPEN: f32 = 0.5;

const LABEL_FONT: f32 = 16.0;
const LABEL_LINE: f32 = 19.0;
const KEY_FONT: f32 = 12.0;
const KEY_LINE: f32 = 14.0;
const PLUS_FONT: f32 = 22.0;
const PLUS_LINE: f32 = 26.0;
const COUNTER_FONT: f32 = 20.0;
const COUNTER_LINE: f32 = 24.0;
const COUNTER_GAP: f32 = 8.0;

const TEXT_OUTLINE_PX: f32 = 1.5;
const FLASH_DECAY: f32 = 4.0;
const ROW_STAGGER: f32 = 0.06;
const PULSE_SPEED: f32 = 5.0;
const PULSE_GROW: f32 = 0.08;
const BADGE_PAD: f32 = 10.0;
const BADGE_H: f32 = 28.0;
const BADGE_POP: f32 = 0.35;
const BADGE_DECAY: f32 = 3.0;
const OUTLINE_PX: f32 = 3.0;

const TRACK: [f32; 4] = [0.27, 0.27, 0.27, 0.92];
const BAR_OUTLINE: [f32; 4] = [0.12, 0.12, 0.12, 0.95];
const BADGE_FILL: [f32; 4] = [0.16, 0.16, 0.16, 0.92];
const LOCKED: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

const STAT_COLORS: [[f32; 4]; NUM_UPGRADES] = [
    [0.933, 0.714, 0.565, 1.0],
    [0.925, 0.424, 0.941, 1.0],
    [0.604, 0.424, 0.941, 1.0],
    [0.424, 0.588, 0.941, 1.0],
    [0.941, 0.851, 0.424, 1.0],
    [0.941, 0.424, 0.424, 1.0],
    [0.596, 0.941, 0.424, 1.0],
    [0.424, 0.941, 0.925, 1.0],
];

fn bold() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif).weight(Weight::BOLD)
}

fn make_buffer(fs: &mut glyphon::FontSystem, metrics: Metrics, text: &str) -> Buffer {
    let mut buffer = Buffer::new(fs, metrics);
    buffer.set_text(text, &bold(), Shaping::Basic, None);
    buffer.shape_until_scroll(fs, false);
    buffer
}

fn reshape(fs: &mut glyphon::FontSystem, buffer: &mut Buffer, text: &str) {
    buffer.set_text(text, &bold(), Shaping::Basic, None);
    buffer.shape_until_scroll(fs, false);
}

fn measure(buffer: &Buffer) -> f32 {
    buffer.layout_runs().next().map(|r| r.line_w).unwrap_or(0.0)
}

fn lighten(color: [f32; 4], t: f32) -> [f32; 4] {
    [
        color[0] + (1.0 - color[0]) * t,
        color[1] + (1.0 - color[1]) * t,
        color[2] + (1.0 - color[2]) * t,
        color[3],
    ]
}

fn darken(color: [f32; 4], factor: f32) -> [f32; 4] {
    [
        color[0] * factor,
        color[1] * factor,
        color[2] * factor,
        color[3],
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
    label_text: Vec<String>,
    keys: Vec<Buffer>,
    plus: Buffer,
    counter: Buffer,
    counter_points: u32,
    maxes: [u8; NUM_UPGRADES],
    open: f32,
    pinned: bool,
    hover: Option<usize>,
    flashes: [f32; NUM_UPGRADES],
    rows: [f32; NUM_UPGRADES],
    time: f32,
    badge_pop: f32,
}

impl UpgradePanel {
    pub fn new(fs: &mut glyphon::FontSystem) -> Self {
        let labels = (0..NUM_UPGRADES)
            .map(|_| make_buffer(fs, Metrics::new(LABEL_FONT, LABEL_LINE), ""))
            .collect();
        let keys = (1..=NUM_UPGRADES)
            .map(|n| make_buffer(fs, Metrics::new(KEY_FONT, KEY_LINE), &format!("[{n}]")))
            .collect();

        Self {
            labels,
            label_text: vec![String::new(); NUM_UPGRADES],
            keys,
            plus: make_buffer(fs, Metrics::new(PLUS_FONT, PLUS_LINE), "+"),
            counter: make_buffer(fs, Metrics::new(COUNTER_FONT, COUNTER_LINE), "x0"),
            counter_points: 0,
            maxes: [7; NUM_UPGRADES],
            open: 0.0,
            pinned: false,
            hover: None,
            flashes: [0.0; NUM_UPGRADES],
            rows: [1.0; NUM_UPGRADES],
            time: 0.0,
            badge_pop: 0.0,
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

    pub fn sync(
        &mut self,
        fs: &mut glyphon::FontSystem,
        names: &[String],
        maxes: &[u8; NUM_UPGRADES],
        points: u32,
    ) {
        for i in 0..NUM_UPGRADES {
            let name = names.get(i).map(String::as_str).unwrap_or("");
            if self.label_text[i] != name {
                reshape(fs, &mut self.labels[i], name);
                self.label_text[i] = name.to_string();
            }
        }

        self.maxes = *maxes;

        if self.counter_points != points {
            reshape(fs, &mut self.counter, &format!("x{points}"));
            self.counter_points = points;
            self.badge_pop = 1.0;
        }
    }

    fn visible_rows(&self) -> f32 {
        self.rows.iter().sum()
    }

    fn panel_x(&self) -> f32 {
        OPEN_X + (CLOSED_X - OPEN_X) * (1.0 - self.open)
    }

    fn row_x(&self, i: usize) -> f32 {
        let span = 1.0 - ROW_STAGGER * (NUM_UPGRADES - 1) as f32;
        let t = ((self.open - i as f32 * ROW_STAGGER) / span).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);

        OPEN_X + (CLOSED_X - OPEN_X) * (1.0 - eased)
    }

    fn rows_h(&self) -> f32 {
        self.visible_rows() * (BAR_H + ROW_GAP)
    }

    fn panel_rect(&self, screen: Vec2) -> (Vec2, Vec2) {
        let x = self.panel_x();
        let bottom = screen.y - BOTTOM_MARGIN;
        let top = bottom - self.rows_h() - COUNTER_LINE - COUNTER_GAP;
        (Vec2::new(x, top), Vec2::new(x + ROW_W, bottom))
    }

    fn row_tops(&self, screen: Vec2) -> [f32; NUM_UPGRADES] {
        let bottom = screen.y - BOTTOM_MARGIN;
        let mut y = bottom - self.rows_h();
        let mut tops = [0.0; NUM_UPGRADES];

        for i in 0..NUM_UPGRADES {
            tops[i] = y;
            y += self.rows[i] * (BAR_H + ROW_GAP);
        }

        tops
    }

    pub fn tick(&mut self, dt: f32, cursor: Option<Vec2>, window: Vec2, screen: Vec2, points: u32) {
        let scale = window_to_screen_scale(window, screen);
        let c = cursor.map(|c| c * scale);

        for i in 0..NUM_UPGRADES {
            let target = if self.maxes[i] > 0 { 1.0 } else { 0.0 };
            self.rows[i] += (target - self.rows[i]) * (1.0 - (-ROW_SPEED * dt).exp());
        }

        let in_trigger = c.is_some_and(|c| {
            c.x >= 0.0
                && c.x < screen.x * TRIGGER_W
                && c.y > screen.y * (1.0 - TRIGGER_H)
                && c.y <= screen.y
        });
        let (min, max) = self.panel_rect(screen);
        let hovering =
            c.is_some_and(|c| c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y);

        let target = if self.pinned || points > 0 || in_trigger || hovering {
            1.0
        } else {
            0.0
        };

        self.open += (target - self.open) * (1.0 - (-SLIDE_SPEED * dt).exp());
        if (self.open - target).abs() < 0.002 {
            self.open = target;
        }

        for f in &mut self.flashes {
            *f = (*f - dt * FLASH_DECAY).max(0.0);
        }

        self.time += dt;
        self.badge_pop = (self.badge_pop - dt * BADGE_DECAY).max(0.0);

        self.hover = if self.open > HOVER_OPEN {
            c.and_then(|c| self.row_at(c, screen))
        } else {
            None
        };
    }

    fn row_at(&self, c: Vec2, screen: Vec2) -> Option<usize> {
        let x = self.panel_x();
        if c.x < x || c.x > x + ROW_W {
            return None;
        }

        let tops = self.row_tops(screen);

        (0..NUM_UPGRADES).find(|&i| {
            self.rows[i] > 0.5 && c.y >= tops[i] && c.y <= tops[i] + BAR_H * self.rows[i]
        })
    }

    pub fn hit_test(&self, cursor: Vec2, window: Vec2, screen: Vec2) -> Option<usize> {
        if self.open < CLICKABLE_OPEN {
            return None;
        }

        let scale = window_to_screen_scale(window, screen);
        self.row_at(cursor * scale, screen)
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

        let tops = self.row_tops(screen);
        let (min, _) = self.panel_rect(screen);

        if points > 0 {
            let text_w = measure(&self.counter);
            let pop = 1.0 + self.badge_pop * BADGE_POP;
            let badge = Vec2::new(text_w + BADGE_PAD * 2.0, BADGE_H) * pop;
            let x = self.row_x(0);
            let center = Vec2::new(x + ROW_W - badge.x * 0.5 / pop, min.y + BADGE_H * 0.5);

            instances.push(pill_instance(
                center,
                badge,
                screen,
                BADGE_FILL,
                BAR_OUTLINE,
                OUTLINE_PX,
            ));

            push_outlined(
                &mut areas,
                &self.counter,
                center.x - text_w * 0.5,
                center.y - COUNTER_LINE * 0.5,
                screen,
                WHITE,
            );
        }

        for i in 0..NUM_UPGRADES {
            let row = self.rows[i];
            if row < 0.05 {
                continue;
            }

            let x = self.row_x(i);
            let max = self.maxes[i].max(1);
            let level = levels[i].min(max);
            let color = STAT_COLORS[i];
            let hovered = self.hover == Some(i);
            let flash = self.flashes[i];
            let afford = points > 0 && levels[i] < self.maxes[i];

            let h = BAR_H * row;
            let cy = tops[i] + h * 0.5;
            let bar_cx = x + BAR_W * 0.5;
            let track = if hovered { lighten(TRACK, 0.1) } else { TRACK };

            instances.push(pill_instance(
                Vec2::new(bar_cx, cy),
                Vec2::new(BAR_W, h),
                screen,
                with_alpha(track, row),
                with_alpha(BAR_OUTLINE, row),
                OUTLINE_PX,
            ));

            let inner_w = BAR_W - OUTLINE * 2.0;
            let inner_h = (h - OUTLINE * 2.0).max(0.0);
            let seg_w = (inner_w - SEG_GAP * (max - 1) as f32) / max as f32;
            let left = x + OUTLINE;

            for k in 0..level {
                let seg_left = left + k as f32 * (seg_w + SEG_GAP);
                let mut seg = if hovered { lighten(color, 0.12) } else { color };

                if flash > 0.0 && k + 1 == level {
                    seg = lighten(seg, flash * 0.7);
                }

                instances.push(pill_instance(
                    Vec2::new(seg_left + seg_w * 0.5, cy),
                    Vec2::new(seg_w, inner_h),
                    screen,
                    with_alpha(seg, row),
                    with_alpha(seg, 0.0),
                    0.0,
                ));
            }

            if row > 0.6 {
                let label = &self.labels[i];
                push_outlined(
                    &mut areas,
                    label,
                    bar_cx - measure(label) * 0.5,
                    cy - LABEL_LINE * 0.5,
                    screen,
                    with_alpha(WHITE, row),
                );

                let key = &self.keys[i];
                push_outlined(
                    &mut areas,
                    key,
                    x + BAR_W - OUTLINE - 10.0 - measure(key),
                    cy - KEY_LINE * 0.5,
                    screen,
                    with_alpha(WHITE, row * 0.6),
                );
            }

            let plus_cx = x + BAR_W + PLUS_GAP + PLUS_D * 0.5;
            let pulse = if afford {
                1.0 + ((self.time * PULSE_SPEED + i as f32 * 0.7).sin() * 0.5 + 0.5) * PULSE_GROW
            } else {
                1.0
            };
            let d = PLUS_D * row * pulse;
            let (fill, edge) = if afford {
                let fill = if hovered {
                    lighten(color, 0.25 + flash * 0.3)
                } else {
                    color
                };
                (fill, darken(color, 0.55))
            } else {
                (LOCKED, darken(LOCKED, 0.6))
            };

            instances.push(pill_instance(
                Vec2::new(plus_cx, cy),
                Vec2::new(d, d),
                screen,
                with_alpha(fill, row),
                with_alpha(edge, row),
                OUTLINE_PX,
            ));

            if row > 0.6 {
                push_outlined(
                    &mut areas,
                    &self.plus,
                    plus_cx - measure(&self.plus) * 0.5,
                    cy - PLUS_LINE * 0.5,
                    screen,
                    with_alpha(WHITE, if afford { row } else { row * 0.5 }),
                );
            }
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
    color: [f32; 4],
) {
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: screen.x as i32,
        bottom: screen.y as i32,
    };
    let outline_color = to_glyphon(with_alpha(DARK_THEME.fill_border, color[3]));
    let fill_color = to_glyphon(color);

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
