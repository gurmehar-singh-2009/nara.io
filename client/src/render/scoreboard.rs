use std::collections::HashMap;

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};

use crate::render::{
    buffers::EntityInstance,
    colours::{DARK_THEME, to_glyphon},
};

const RIGHT_MARGIN: f32 = 16.0;
const TOP_MARGIN: f32 = 24.0;
const ROW_WIDTH: f32 = 350.0;
const ROW_HEIGHT: f32 = 32.0;
const ROW_GAP: f32 = 6.0;
const ROW_FONT_SIZE: f32 = 24.0;
const ROW_LINE_HEIGHT: f32 = 28.0;
const TITLE_LINE_HEIGHT: f32 = 32.0;
const TITLE_TO_ROWS: f32 = 8.0;
const ICON_SIZE: f32 = 15.0;
const ICON_INSET: f32 = 3.0;
const MAX_ROWS: usize = 10;

const ANIM_SPEED: f32 = 10.0;
const ANIM_EPS: f32 = 0.5;
const TITLE_OUTLINE_PX: f32 = 2.5;
const ROW_TEXT_OUTLINE_PX: f32 = 2.0;

fn row_metrics() -> Metrics {
    Metrics::new(ROW_FONT_SIZE, ROW_LINE_HEIGHT)
}

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

fn fmt_score(score: f32) -> String {
    if score >= 1_000_000.0 {
        format!("{:.1}m", score / 1_000_000.0)
    } else if score >= 1_000.0 {
        format!("{:.1}k", score / 1_000.0)
    } else {
        format!("{}", score.round() as u32)
    }
}

fn row_text(name: &str, score: f32) -> String {
    if name.is_empty() {
        fmt_score(score)
    } else {
        format!("{name} - {}", fmt_score(score))
    }
}

pub(crate) fn ui_instance(
    center: Vec2,
    size_px: Vec2,
    screen: Vec2,
    shape_type: u32,
    fill: [f32; 4],
    border: [f32; 4],
    border_px: f32,
) -> EntityInstance {
    let pos = Vec2::new(
        center.x / screen.x * 2.0 - 1.0,
        1.0 - center.y / screen.y * 2.0,
    );
    let size = size_px * 2.0 / screen;

    EntityInstance {
        position: [pos.x, pos.y],
        size: [size.x, size.y],
        rotation: 0.0,
        shape_type,
        sides: 0,
        fill_color: fill,
        border_color: border,
        border_thickness: border_px,
        extra_param: 0.0,
    }
}

pub(crate) fn bar_ui_instance(
    center: Vec2,
    size_px: Vec2,
    screen: Vec2,
    fill: [f32; 4],
) -> EntityInstance {
    ui_instance(
        center,
        size_px,
        screen,
        5,
        fill,
        DARK_THEME.scoreboard_row,
        4.0,
    )
}

pub(crate) fn rounded_ui_instance(
    center: Vec2,
    size_px: Vec2,
    screen: Vec2,
    fill: [f32; 4],
    border: [f32; 4],
    border_px: f32,
    corner_radius: f32,
) -> EntityInstance {
    let mut inst = ui_instance(center, size_px, screen, 7, fill, border, border_px);
    inst.extra_param = corner_radius;
    inst
}

struct Row {
    name: String,
    target_score: f32,
    display_score: f32,
    target_ratio: f32,
    display_ratio: f32,
    text: String,
    buffer: Buffer,
}

pub struct Scoreboard {
    title: Buffer,
    rows: Vec<Row>,
}

impl Scoreboard {
    const MAX_RESHAPES_PER_FRAME: usize = 2;

    pub fn new(fs: &mut glyphon::FontSystem) -> Self {
        Self {
            title: make_buffer(fs, Metrics::new(32.0, 32.0), "Scoreboard"),
            rows: Vec::new(),
        }
    }

    pub fn sync(&mut self, fs: &mut glyphon::FontSystem, entries: &[(String, u32)]) {
        let entries = &entries[..entries.len().min(MAX_ROWS)];
        let top = entries.iter().map(|e| e.1).max().unwrap_or(1).max(1) as f32;

        if self.rows.len() == entries.len()
            && self
                .rows
                .iter()
                .zip(entries.iter())
                .all(|(row, (name, _))| row.name == *name)
        {
            for (row, (_, score)) in self.rows.iter_mut().zip(entries.iter()) {
                row.target_score = *score as f32;
                row.target_ratio = *score as f32 / top;
            }
            return;
        }

        let mut old: HashMap<String, (f32, f32, String)> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(|r| (r.name, (r.display_score, r.display_ratio, r.text)))
            .collect();

        self.rows = entries
            .iter()
            .map(|(name, score)| {
                let target_score = *score as f32;
                let target_ratio = target_score / top;
                if let Some((display_score, display_ratio, text)) = old.remove(name) {
                    let buffer = make_buffer(fs, row_metrics(), &text);
                    Row {
                        name: name.clone(),
                        target_score,
                        display_score,
                        target_ratio,
                        display_ratio,
                        text,
                        buffer,
                    }
                } else {
                    let text = row_text(name, target_score);
                    let buffer = make_buffer(fs, row_metrics(), &text);
                    Row {
                        name: name.clone(),
                        target_score,
                        display_score: target_score,
                        target_ratio,
                        display_ratio: target_ratio,
                        text,
                        buffer,
                    }
                }
            })
            .collect();
    }

    pub fn tick(&mut self, fs: &mut glyphon::FontSystem, dt: f32) {
        let t = 1.0 - (-ANIM_SPEED * dt).exp();
        let mut reshapes = 0;

        for row in self.rows.iter_mut() {
            if row.display_score == row.target_score && row.display_ratio == row.target_ratio {
                continue;
            }

            let ds_diff = row.target_score - row.display_score;
            if ds_diff.abs() < ANIM_EPS {
                row.display_score = row.target_score;
            } else {
                row.display_score += ds_diff * t;
            }

            let dr_diff = row.target_ratio - row.display_ratio;
            if dr_diff.abs() < 0.001 {
                row.display_ratio = row.target_ratio;
            } else {
                row.display_ratio += dr_diff * t;
            }

            let text = row_text(&row.name, row.display_score);
            if text != row.text && reshapes < Self::MAX_RESHAPES_PER_FRAME {
                row.buffer.set_text(&text, &bold(), Shaping::Basic, None);
                row.buffer.shape_until_scroll(fs, false);
                row.text = text;
                reshapes += 1;
            }
        }
    }

    pub fn render_data(&self, screen: Vec2) -> (Vec<EntityInstance>, Vec<TextArea<'_>>) {
        let mut instances = Vec::new();
        let mut areas = Vec::new();
        let theme = &DARK_THEME;

        let right = screen.x - RIGHT_MARGIN;
        let left = right - ROW_WIDTH;
        let center_x = left + ROW_WIDTH * 0.5;

        let title_left = center_x - measure(&self.title) * 0.5;
        push_text(
            &mut areas,
            &self.title,
            title_left,
            TOP_MARGIN,
            screen,
            theme.scoreboard_title,
            TITLE_OUTLINE_PX,
        );

        let mut y = TOP_MARGIN + TITLE_LINE_HEIGHT + TITLE_TO_ROWS;

        for row in self.rows.iter() {
            let center_y = y + ROW_HEIGHT * 0.5;

            instances.push(bar_ui_instance(
                Vec2::new(center_x, center_y),
                Vec2::new(ROW_WIDTH, ROW_HEIGHT),
                screen,
                theme.scoreboard_row,
            ));

            let fill_width = (ROW_WIDTH * row.display_ratio).max(ROW_HEIGHT);
            instances.push(bar_ui_instance(
                Vec2::new(left + fill_width * 0.5, center_y),
                Vec2::new(fill_width, ROW_HEIGHT),
                screen,
                theme.health_bar_foreground,
            ));

            let icon_x = (left + fill_width - ICON_SIZE * 0.5 - ICON_INSET)
                .max(left + ICON_SIZE * 0.5 + ICON_INSET);
            instances.push(ui_instance(
                Vec2::new(icon_x, center_y),
                Vec2::new(ICON_SIZE, ICON_SIZE),
                screen,
                6,
                theme.team_blue,
                DARK_THEME.scoreboard_row,
                4.0,
            ));

            let text_left = center_x - measure(&row.buffer) * 0.5;
            let text_top = y + (ROW_HEIGHT - ROW_LINE_HEIGHT) * 0.5;
            push_text(
                &mut areas,
                &row.buffer,
                text_left,
                text_top,
                screen,
                theme.scoreboard_text,
                ROW_TEXT_OUTLINE_PX,
            );

            y += ROW_HEIGHT + ROW_GAP;
        }

        (instances, areas)
    }
}

fn push_text<'a>(
    areas: &mut Vec<TextArea<'a>>,
    buffer: &'a Buffer,
    left: f32,
    top: f32,
    screen: Vec2,
    color: [f32; 4],
    outline_px: f32,
) {
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: screen.x as i32,
        bottom: screen.y as i32,
    };
    let outline_color = to_glyphon(DARK_THEME.fill_border);

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
            left: left + dx * outline_px,
            top: top + dy * outline_px,
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
        default_color: to_glyphon(color),
        custom_glyphs: &[],
    });
}
