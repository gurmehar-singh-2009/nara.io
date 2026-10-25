use std::cell::RefCell;

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};
use shared::packets::{
    TankOption,
    client_bound::{BarrelDef, TurretDef},
};

use crate::{
    entities::tank::{TankDraw, tank_instances},
    render::{
        buffers::EntityInstance,
        colours::{Color, DARK_THEME, to_glyphon, with_alpha},
        scoreboard::{pill_instance, rounded_ui_instance},
    },
};

const TILE_W: f32 = 118.0;
const TILE_H: f32 = 118.0;
const TILE_RADIUS: f32 = 12.0;
const TILE_BORDER: f32 = 3.0;
const GAP: f32 = 10.0;
const MIN_COLS: usize = 3;
const MAX_COLS: usize = 5;
const OPEN_TOP: f32 = 84.0;
const SLIDE_SPEED: f32 = 12.0;
const HOVER_SPEED: f32 = 18.0;
const HOVER_GROW: f32 = 0.06;
const STAGGER: f32 = 0.08;
const CLICKABLE_OPEN: f32 = 0.9;

const ICON_CY: f32 = 50.0;
const STRIP_H: f32 = 30.0;

const TANK_BODY_SIZE: f32 = 42.0;

const ICON_BODY_PX: f32 = 40.0;

const SPIN_SPEED: f32 = 1.2;

const NAME_FONT: f32 = 16.0;
const NAME_LINE: f32 = 20.0;
const TEXT_OUTLINE_PX: f32 = 1.5;

const IGNORE_W: f32 = 104.0;
const IGNORE_H: f32 = 30.0;
const IGNORE_GAP: f32 = 12.0;
const IGNORE_FILL: [f32; 4] = [0.2, 0.2, 0.2, 0.85];
const IGNORE_HOVER: [f32; 4] = [0.32, 0.32, 0.32, 0.92];
const IGNORE_BORDER: [f32; 4] = [0.08, 0.08, 0.08, 0.9];

const PLAYER_CLEARANCE: f32 = 110.0;
const MIN_FIT: f32 = 0.6;
const ROOM_GAP: f32 = 14.0;

thread_local! {
    static TANK_OPTIONS: RefCell<(u32, Vec<TankOption>)> = RefCell::new((0, Vec::new()));
}

pub fn push_tank_options(options: Vec<TankOption>) {
    TANK_OPTIONS.with(|cell| {
        let mut cell = cell.borrow_mut();
        cell.0 = cell.0.wrapping_add(1);
        cell.1 = options;
    });
}

pub fn current_options() -> Vec<TankOption> {
    TANK_OPTIONS.with(|cell| cell.borrow().1.clone())
}

fn options_snapshot() -> (u32, Vec<TankOption>) {
    TANK_OPTIONS.with(|cell| {
        let cell = cell.borrow();
        (cell.0, cell.1.clone())
    })
}

#[derive(Clone)]
pub struct TankClassDef {
    pub name: String,
    pub color: [f32; 4],
    pub sides: u32,
    pub barrels: Vec<BarrelDef>,
    pub turrets: Vec<TurretDef>,
}

impl TankClassDef {
    fn from_option(option: &TankOption, index: usize) -> Self {
        Self {
            name: option.name.clone(),
            color: tank_color(index),
            sides: option.sides,
            barrels: option.barrels.clone(),
            turrets: option.turrets.clone(),
        }
    }
}

pub fn default_classes() -> Vec<TankClassDef> {
    Vec::new()
}

fn tank_color(index: usize) -> Color {
    const PALETTE: [Color; 7] = [
        DARK_THEME.team_blue,   // #00B2E1
        DARK_THEME.team_red,    // #F14E54
        DARK_THEME.team_purple, // #F177DD
        DARK_THEME.team_green,  // #00E16E
        DARK_THEME.square,      // #FFE869
        DARK_THEME.pentagon,    // #768DFF
        DARK_THEME.triangle,    // #FC7677
    ];
    PALETTE[index % PALETTE.len()]
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

fn lighten(c: [f32; 4], t: f32) -> [f32; 4] {
    [
        c[0] + (1.0 - c[0]) * t,
        c[1] + (1.0 - c[1]) * t,
        c[2] + (1.0 - c[2]) * t,
        c[3],
    ]
}

fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + t * t * (2.7 * t + 1.7)
}

fn window_to_screen_scale(window: Vec2, screen: Vec2) -> f32 {
    if window.x > 0.0 {
        screen.x / window.x
    } else {
        1.0
    }
}

pub(crate) fn screen_to_world(
    center: Vec2,
    screen: Vec2,
    camera_pos: [f32; 2],
    zoom: f32,
) -> ([f32; 2], f32) {
    let aspect = if screen.y > 0.0 {
        screen.x / screen.y
    } else {
        1.0
    };
    let half_w = (screen.x * 0.5).max(1.0);
    let half_h = (screen.y * 0.5).max(1.0);

    let ndc_x = center.x / half_w - 1.0;
    let ndc_y = 1.0 - center.y / half_h;
    let position = [
        camera_pos[0] + ndc_x * aspect / zoom,
        camera_pos[1] + ndc_y / zoom,
    ];

    let px_per_world = (zoom * screen.y * 0.5).max(1e-4);
    (position, px_per_world)
}

pub(crate) fn push_tank_icon(
    instances: &mut Vec<EntityInstance>,
    def: &TankClassDef,
    world_pos: [f32; 2],
    spin: f32,
    scale: f32,
    alpha: f32,
) {
    let aims: Vec<f32> = def
        .turrets
        .iter()
        .map(|t| spin + t.angle.to_radians())
        .collect();

    let (parts, _) = tank_instances(&TankDraw {
        pos: Vec2::new(world_pos[0], world_pos[1]),
        rot: spin,
        scale,
        barrels: &def.barrels,
        recoils: &[],
        turrets: &def.turrets,
        turret_aims: &aims,
        sides: def.sides,
        body: def.color,
        alpha,
    });

    instances.extend(parts);
}

pub(crate) fn icon_scale(px: f32, screen: Vec2, zoom: f32) -> f32 {
    let px_per_world = (zoom * screen.y * 0.5).max(1e-4);
    px / (TANK_BODY_SIZE * px_per_world)
}

pub struct TankUpgradePanel {
    defs: Vec<TankClassDef>,
    labels: Vec<Buffer>,
    /// 0 = closed (above the screen), 1 = fully open
    open: f32,
    pinned: bool,
    /// icon rotation (radians); advances in tick so the tanks spin
    spin: f32,
    defs_gen: u32,
    hovers: Vec<f32>,
    hovered: Option<usize>,
    ignore_hovered: bool,
    dismissed: bool,
    ignore_label: Buffer,
    top: f32,
    top_target: f32,
}

impl TankUpgradePanel {
    pub fn new(fs: &mut glyphon::FontSystem, defs: Vec<TankClassDef>) -> Self {
        let mut panel = Self {
            defs: Vec::new(),
            labels: Vec::new(),
            open: 0.0,
            pinned: false,
            spin: 0.0,
            defs_gen: 0,
            hovers: Vec::new(),
            hovered: None,
            ignore_hovered: false,
            dismissed: false,
            ignore_label: make_buffer(fs, "Ignore"),
            top: OPEN_TOP,
            top_target: OPEN_TOP,
        };

        panel.set_defs(fs, defs);

        let (genr, options) = options_snapshot();
        if genr != 0 {
            let defs = options
                .iter()
                .enumerate()
                .map(|(i, o)| TankClassDef::from_option(o, i))
                .collect();
            panel.set_defs(fs, defs);
            panel.defs_gen = genr;
        }

        panel
    }

    fn set_defs(&mut self, fs: &mut glyphon::FontSystem, defs: Vec<TankClassDef>) {
        self.labels = defs.iter().map(|d| make_buffer(fs, &d.name)).collect();
        self.hovers = vec![0.0; defs.len()];
        self.defs = defs;
        self.dismissed = false;
    }

    pub fn dismiss(&mut self) {
        self.dismissed = true;
        self.pinned = false;
    }

    pub fn make_room(&mut self, above: f32) {
        self.top_target = OPEN_TOP.max(above + ROOM_GAP);
    }

    fn fit(&self, screen: Vec2) -> f32 {
        let room = screen.y * 0.5 - PLAYER_CLEARANCE - OPEN_TOP - IGNORE_GAP - IGNORE_H;
        (room / self.total_h(1.0)).clamp(MIN_FIT, 1.0)
    }

    fn ignore_rect(&self, screen: Vec2) -> (Vec2, Vec2) {
        let s = self.fit(screen);
        let top = self.grid_top(s) + self.total_h(s) + IGNORE_GAP;
        let left = (screen.x - IGNORE_W) * 0.5;
        (
            Vec2::new(left, top),
            Vec2::new(left + IGNORE_W, top + IGNORE_H),
        )
    }

    pub fn hit_ignore(&self, cursor: Vec2, window: Vec2, screen: Vec2) -> bool {
        if self.open < CLICKABLE_OPEN {
            return false;
        }

        let c = cursor * window_to_screen_scale(window, screen);
        let (min, max) = self.ignore_rect(screen);
        c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y
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

    fn cols(&self) -> usize {
        let n = self.defs.len();
        if n <= MIN_COLS {
            return MIN_COLS;
        }
        ((n as f32).sqrt().ceil() as usize).clamp(MIN_COLS, MAX_COLS)
    }

    fn rows(&self) -> usize {
        let cols = self.cols();
        (self.defs.len() + cols - 1) / cols
    }

    fn total_w(&self, s: f32) -> f32 {
        let cols = self.cols() as f32;
        (cols * TILE_W + (cols - 1.0) * GAP) * s
    }

    fn total_h(&self, s: f32) -> f32 {
        let rows = self.rows().max(1);
        (rows as f32 * TILE_H + (rows as f32 - 1.0) * GAP) * s
    }

    fn grid_top(&self, s: f32) -> f32 {
        let closed = -(self.total_h(s) + 40.0);
        closed + (self.top - closed) * self.open
    }

    fn tile_rects(&self, screen: Vec2) -> Vec<(Vec2, Vec2)> {
        let s = self.fit(screen);
        let (tile_w, tile_h, gap) = (TILE_W * s, TILE_H * s, GAP * s);
        let left = (screen.x - self.total_w(s)) * 0.5;
        let top = self.grid_top(s);
        self.defs
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let col = (i % self.cols()) as f32;
                let row = (i / self.cols()) as f32;
                let min = Vec2::new(left + col * (tile_w + gap), top + row * (tile_h + gap));
                (min, min + Vec2::new(tile_w, tile_h))
            })
            .collect()
    }

    pub fn tick(
        &mut self,
        fs: &mut glyphon::FontSystem,
        dt: f32,
        available: bool,
        cursor: Option<Vec2>,
        window: Vec2,
        screen: Vec2,
    ) {
        let (genr, options) = options_snapshot();
        if genr != self.defs_gen {
            self.defs_gen = genr;
            let defs: Vec<TankClassDef> = options
                .iter()
                .enumerate()
                .map(|(i, o)| TankClassDef::from_option(o, i))
                .collect();
            self.set_defs(fs, defs);
        }

        let target = if self.pinned || (available && !self.dismissed) {
            1.0
        } else {
            0.0
        };
        let t = 1.0 - (-SLIDE_SPEED * dt).exp();
        self.open += (target - self.open) * t;
        if (self.open - target).abs() < 0.002 {
            self.open = target;
        }
        self.top += (self.top_target - self.top) * t;

        // SPIN YAY
        self.spin += dt * SPIN_SPEED;

        self.hovered = cursor.and_then(|c| self.hit_test(c, window, screen));
        self.ignore_hovered = cursor.is_some_and(|c| self.hit_ignore(c, window, screen));

        let t = 1.0 - (-HOVER_SPEED * dt).exp();
        for (i, h) in self.hovers.iter_mut().enumerate() {
            let target = if self.hovered == Some(i) { 1.0 } else { 0.0 };
            *h += (target - *h) * t;
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

    pub fn render_data(
        &self,
        screen: Vec2,
        camera_pos: [f32; 2],
        zoom: f32,
    ) -> (Vec<EntityInstance>, Vec<TextArea<'_>>) {
        let mut instances = Vec::new();
        let mut areas = Vec::new();

        if self.open <= 0.01 {
            return (instances, areas);
        }

        let rects = self.tile_rects(screen);
        let s = self.fit(screen);

        for (i, def) in self.defs.iter().enumerate() {
            let (min, _) = rects[i];
            let appear = ease_out_back((self.open - i as f32 * STAGGER) / (1.0 - STAGGER * 4.0));
            let alpha = appear.clamp(0.0, 1.0);
            if alpha <= 0.01 {
                continue;
            }

            let hover = self.hovers.get(i).copied().unwrap_or(0.0);
            let grow = 1.0 + hover * HOVER_GROW;
            let drop = (1.0 - appear) * -40.0;

            let center = Vec2::new(min.x + TILE_W * s * 0.5, min.y + TILE_H * s * 0.5 + drop);
            let size = Vec2::new(TILE_W, TILE_H) * s * grow;
            let fill = lighten(def.color, hover * 0.15);

            instances.push(rounded_ui_instance(
                center,
                size,
                screen,
                with_alpha(fill, alpha),
                with_alpha(darken(def.color, 0.55), alpha),
                TILE_BORDER,
                TILE_RADIUS * s * grow,
            ));

            let strip_h = STRIP_H * s * grow;
            let strip_center = Vec2::new(
                center.x,
                center.y + size.y * 0.5 - TILE_BORDER - strip_h * 0.5,
            );

            instances.push(rounded_ui_instance(
                strip_center,
                Vec2::new(size.x - TILE_BORDER * 2.0, strip_h),
                screen,
                with_alpha(darken(def.color, 0.78), alpha),
                with_alpha(darken(def.color, 0.78), 0.0),
                0.0,
                (TILE_RADIUS * s - TILE_BORDER).max(2.0) * grow,
            ));

            let icon_c = Vec2::new(center.x, center.y - size.y * 0.5 + ICON_CY * s * grow);
            let (world_pos, _) = screen_to_world(icon_c, screen, camera_pos, zoom);
            push_tank_icon(
                &mut instances,
                def,
                world_pos,
                self.spin,
                icon_scale(ICON_BODY_PX * s * grow, screen, zoom),
                alpha,
            );

            let text_s = s.max(0.8);
            let name_w = measure(&self.labels[i]) * text_s;
            push_outlined(
                &mut areas,
                &self.labels[i],
                center.x - name_w * 0.5,
                strip_center.y - NAME_LINE * text_s * 0.5,
                text_s,
                screen,
                alpha,
            );
        }

        if !self.defs.is_empty() {
            let (min, max) = self.ignore_rect(screen);
            let center = (min + max) * 0.5;
            let fill = if self.ignore_hovered {
                IGNORE_HOVER
            } else {
                IGNORE_FILL
            };
            let alpha = ((self.open - 0.5) * 2.0).clamp(0.0, 1.0);

            instances.push(pill_instance(
                center,
                Vec2::new(IGNORE_W, IGNORE_H),
                screen,
                with_alpha(fill, alpha),
                with_alpha(IGNORE_BORDER, alpha),
                2.0,
            ));

            let w = measure(&self.ignore_label);
            push_outlined(
                &mut areas,
                &self.ignore_label,
                center.x - w * 0.5,
                center.y - NAME_LINE * 0.5,
                1.0,
                screen,
                alpha,
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
    scale: f32,
    screen: Vec2,
    alpha: f32,
) {
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: screen.x as i32,
        bottom: screen.y as i32,
    };
    let outline_color = to_glyphon(with_alpha(DARK_THEME.fill_border, alpha));
    let fill_color = to_glyphon([1.0, 1.0, 1.0, alpha]);

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
            scale,
            bounds,
            default_color: outline_color,
            custom_glyphs: &[],
        });
    }

    areas.push(TextArea {
        buffer,
        left,
        top,
        scale,
        bounds,
        default_color: fill_color,
        custom_glyphs: &[],
    });
}
