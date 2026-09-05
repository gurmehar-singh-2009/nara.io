//!   let mut defs = default_classes();
//!   defs.push(TankClassDef::with_barrels(
//!       "Octo Tank".into(),
//!       DARK_THEME.team_blue,
//!       vec![
//!           BarrelIcon::new(0.0, 0.0, 26.0, 9.0),
//!           BarrelIcon::new(90.0, 0.0, 26.0, 9.0),
//!           BarrelIcon::new(180.0, 0.0, 26.0, 9.0),
//!           BarrelIcon::new(270.0, 0.0, 26.0, 9.0),
//!       ],
//!   ));
//!   TankUpgradePanel::new(fs, defs)
//!
//! Barrel angles are axis-aligned (0 = up, 90 = right, 180 = down,
//! 270 = left) because screen-space rects can't rotate; `lateral`
//! offsets a barrel sideways for parallel barrels (Twin).

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};

use crate::render::{
    buffers::EntityInstance,
    colours::{DARK_THEME, to_glyphon},
    scoreboard::{rounded_ui_instance, ui_instance},
};

const TILE_W: f32 = 120.0;
const TILE_H: f32 = 120.0;
const TILE_RADIUS: f32 = 12.0;
const TILE_BORDER: f32 = 4.0;
const GAP: f32 = 14.0;
const COLS: usize = 3;
const OPEN_TOP: f32 = 14.0;
const SLIDE_SPEED: f32 = 12.0;
const CLICKABLE_OPEN: f32 = 0.9;

const ICON_CY: f32 = 46.0;
const BODY_R: f32 = 21.0;
const BODY_BORDER: f32 = 5.0;
const BARREL_BORDER: f32 = 3.0;
const BARREL_RADIUS: f32 = 3.0;

const NAME_FONT: f32 = 16.0;
const NAME_LINE: f32 = 20.0;
const NAME_BOTTOM: f32 = 10.0;
const TEXT_OUTLINE_PX: f32 = 1.5;

#[derive(Clone)]
pub struct BarrelIcon {
    pub angle_deg: f32,
    pub lateral: f32,
    pub length: f32,
    pub width: f32,
}

impl BarrelIcon {
    pub fn new(angle_deg: f32, lateral: f32, length: f32, width: f32) -> Self {
        Self {
            angle_deg,
            lateral,
            length,
            width,
        }
    }
}

#[derive(Clone)]
pub struct TankClassDef {
    pub name: String,
    pub color: [f32; 4],
    pub barrels: Vec<BarrelIcon>,
}

impl TankClassDef {
    pub fn new(name: impl Into<String>, color: [f32; 4]) -> Self {
        Self::with_barrels(name, color, vec![BarrelIcon::new(0.0, 0.0, 26.0, 9.0)])
    }

    pub fn with_barrels(
        name: impl Into<String>,
        color: [f32; 4],
        barrels: Vec<BarrelIcon>,
    ) -> Self {
        Self {
            name: name.into(),
            color,
            barrels,
        }
    }
}

pub fn default_classes() -> Vec<TankClassDef> {
    let t = &DARK_THEME;
    vec![
        TankClassDef::with_barrels(
            "Twin",
            t.team_blue,
            vec![
                BarrelIcon::new(0.0, -9.0, 27.0, 10.0),
                BarrelIcon::new(0.0, 9.0, 27.0, 10.0),
            ],
        ),
        TankClassDef::with_barrels(
            "Sniper",
            t.health_bar_foreground,
            vec![BarrelIcon::new(0.0, 0.0, 34.0, 8.0)],
        ),
        TankClassDef::with_barrels(
            "Machine Gun",
            t.xp_bar_fill,
            vec![BarrelIcon::new(0.0, 0.0, 23.0, 15.0)],
        ),
        TankClassDef::with_barrels(
            "Flank Guard",
            t.team_red,
            vec![
                BarrelIcon::new(0.0, 0.0, 25.0, 10.0),
                BarrelIcon::new(180.0, 0.0, 25.0, 10.0),
            ],
        ),
        TankClassDef::with_barrels("Smasher", t.team_purple, vec![]),
        TankClassDef::new("Auto Tank", t.pentagon),
    ]
}

fn bold() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif).weight(Weight::BOLD)
}

fn make_buffer(fs: &mut glyphon::FontSystem, text: &str) -> Buffer {
    let mut buffer = Buffer::new(fs, Metrics::new(NAME_FONT, NAME_LINE));
    buffer.set_text(text, &bold(), Shaping::Basic, None);
    buffer.shape_until_scroll(fs, false);
    buffer
}

fn measure(buffer: &Buffer) -> f32 {
    buffer.layout_runs().next().map(|r| r.line_w).unwrap_or(0.0)
}

fn darken(c: [f32; 4], f: f32) -> [f32; 4] {
    [c[0] * f, c[1] * f, c[2] * f, c[3]]
}

fn window_to_screen_scale(window: Vec2, screen: Vec2) -> f32 {
    if window.x > 0.0 {
        screen.x / window.x
    } else {
        1.0
    }
}

pub struct TankUpgradePanel {
    defs: Vec<TankClassDef>,
    labels: Vec<Buffer>,
    /// 0 = closed (above the screen), 1 = fully open
    open: f32,
    pinned: bool,
}

impl TankUpgradePanel {
    pub fn new(fs: &mut glyphon::FontSystem, defs: Vec<TankClassDef>) -> Self {
        let labels = defs.iter().map(|d| make_buffer(fs, &d.name)).collect();
        Self {
            defs,
            labels,
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

    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    fn rows(&self) -> usize {
        (self.defs.len() + COLS - 1) / COLS
    }

    fn total_w(&self) -> f32 {
        COLS as f32 * TILE_W + (COLS as f32 - 1.0) * GAP
    }

    fn total_h(&self) -> f32 {
        let rows = self.rows().max(1);
        rows as f32 * TILE_H + (rows as f32 - 1.0) * GAP
    }

    fn grid_top(&self) -> f32 {
        let closed = -(self.total_h() + 40.0);
        closed + (OPEN_TOP - closed) * self.open
    }

    fn tile_rects(&self, screen: Vec2) -> Vec<(Vec2, Vec2)> {
        let left = (screen.x - self.total_w()) * 0.5;
        let top = self.grid_top();
        self.defs
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let col = (i % COLS) as f32;
                let row = (i / COLS) as f32;
                let min = Vec2::new(left + col * (TILE_W + GAP), top + row * (TILE_H + GAP));
                (min, min + Vec2::new(TILE_W, TILE_H))
            })
            .collect()
    }

    pub fn tick(&mut self, dt: f32, available: bool) {
        let target = if self.pinned || available { 1.0 } else { 0.0 };
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
        for (i, (min, max)) in self.tile_rects(screen).iter().enumerate() {
            if c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y {
                return Some(i);
            }
        }
        None
    }

    pub fn render_data(&self, screen: Vec2) -> (Vec<EntityInstance>, Vec<TextArea<'_>>) {
        let mut instances = Vec::new();
        let mut areas = Vec::new();

        if self.open <= 0.01 {
            return (instances, areas);
        }

        let rects = self.tile_rects(screen);

        for (i, def) in self.defs.iter().enumerate() {
            let (min, _) = rects[i];
            let tile_cx = min.x + TILE_W * 0.5;
            let tile_cy = min.y + TILE_H * 0.5;
            let icon_c = Vec2::new(tile_cx, min.y + ICON_CY);

            instances.push(rounded_ui_instance(
                Vec2::new(tile_cx, tile_cy),
                Vec2::new(TILE_W, TILE_H),
                screen,
                def.color,
                darken(def.color, 0.55),
                TILE_BORDER,
                TILE_RADIUS,
            ));

            for barrel in def.barrels.iter() {
                let a = barrel.angle_deg.to_radians();
                let dir = Vec2::new(a.sin(), -a.cos());
                let perp = Vec2::new(a.cos(), a.sin());
                let center = icon_c + dir * (barrel.length * 0.5) + perp * barrel.lateral;
                let vertical = barrel.angle_deg.rem_euclid(180.0) == 0.0;
                let size = if vertical {
                    Vec2::new(barrel.width, barrel.length)
                } else {
                    Vec2::new(barrel.length, barrel.width)
                };
                instances.push(rounded_ui_instance(
                    center,
                    size,
                    screen,
                    DARK_THEME.barrel,
                    DARK_THEME.barrel_outline,
                    BARREL_BORDER,
                    BARREL_RADIUS,
                ));
            }

            instances.push(ui_instance(
                icon_c,
                Vec2::new(BODY_R * 2.0, BODY_R * 2.0),
                screen,
                6,
                def.color,
                darken(def.color, 0.45),
                BODY_BORDER,
            ));

            let name_w = measure(&self.labels[i]);
            push_outlined(
                &mut areas,
                &self.labels[i],
                tile_cx - name_w * 0.5,
                min.y + TILE_H - NAME_LINE - NAME_BOTTOM,
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
