use std::cell::RefCell;

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};
use shared::packets::{TankOption, client_bound::BarrelDef};

use crate::render::{
    buffers::EntityInstance,
    colours::{DARK_THEME, to_glyphon},
    scoreboard::rounded_ui_instance,
};

const TILE_W: f32 = 140.0;
const TILE_H: f32 = 140.0;
const TILE_RADIUS: f32 = 12.0;
const TILE_BORDER: f32 = 4.0;
const GAP: f32 = 14.0;
const MIN_COLS: usize = 3;
const MAX_COLS: usize = 5;
const OPEN_TOP: f32 = 14.0;
const SLIDE_SPEED: f32 = 12.0;
const CLICKABLE_OPEN: f32 = 0.9;

const ICON_CY: f32 = 68.0;

const TANK_BODY_SIZE: f32 = 42.0; // `let size = 42.0 * self.scale`
const TANK_BORDER: f32 = 3.0; // `border_thickness: 3.0 * self.scale`

const ICON_BODY_PX: f32 = 54.0;

const SPIN_SPEED: f32 = 1.2;

const NAME_FONT: f32 = 20.0;
const NAME_LINE: f32 = 24.0;
const NAME_BOTTOM: f32 = 10.0;
const TEXT_OUTLINE_PX: f32 = 1.5;

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
}

impl TankClassDef {
    fn from_option(option: &TankOption) -> Self {
        Self {
            name: option.name.clone(),
            color: tier_color(option.tier),
            sides: option.sides,
            barrels: option.barrels.clone(),
        }
    }
}

pub fn default_classes() -> Vec<TankClassDef> {
    Vec::new()
}

fn tier_color(tier: u32) -> [f32; 4] {
    match tier % 4 {
        0 => DARK_THEME.team_blue,
        1 => DARK_THEME.team_red,
        2 => DARK_THEME.team_purple,
        _ => DARK_THEME.pentagon,
    }
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

fn screen_to_world(center: Vec2, screen: Vec2, camera_pos: [f32; 2], zoom: f32) -> ([f32; 2], f32) {
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

fn push_tank_icon(
    instances: &mut Vec<EntityInstance>,
    def: &TankClassDef,
    world_pos: [f32; 2],
    spin: f32,
    scale: f32,
) {
    for barrel in def.barrels.iter() {
        let barrel_angle = barrel.angle.to_radians();
        let world_angle = spin + barrel_angle;

        let base_local = Vec2::new(barrel.x, barrel.y) * scale;
        let base_pos =
            Vec2::new(world_pos[0], world_pos[1]) + Vec2::from_angle(spin).rotate(base_local);
        let center_offset = Vec2::from_angle(world_angle) * (barrel.length * scale * 0.5);
        let barrel_pos = base_pos + center_offset;

        instances.push(EntityInstance {
            position: [barrel_pos.x, barrel_pos.y],
            size: [barrel.length * scale, barrel.width * scale],
            rotation: world_angle,
            shape_type: 1,
            sides: 4,
            fill_color: DARK_THEME.barrel,
            border_color: DARK_THEME.barrel_outline,
            border_thickness: TANK_BORDER * scale,
            extra_param: 1.0,
        });
    }

    let size = TANK_BODY_SIZE * scale;
    let shape_type = if def.sides >= 3 { 3 } else { 0 };

    instances.push(EntityInstance {
        position: world_pos,
        size: [size, size],
        rotation: spin,
        shape_type,
        sides: def.sides,
        fill_color: def.color,
        border_color: darken(def.color, 0.35),
        border_thickness: TANK_BORDER * scale,
        extra_param: 1.0,
    });
}

pub struct TankUpgradePanel {
    defs: Vec<TankClassDef>,
    labels: Vec<Buffer>,
    open: f32,
    pinned: bool,
    spin: f32,
    defs_gen: u32,
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
        };

        panel.set_defs(fs, defs);

        let (genr, options) = options_snapshot();
        if genr != 0 {
            let defs = options.iter().map(TankClassDef::from_option).collect();
            panel.set_defs(fs, defs);
            panel.defs_gen = genr;
        }

        panel
    }

    fn set_defs(&mut self, fs: &mut glyphon::FontSystem, defs: Vec<TankClassDef>) {
        self.labels = defs.iter().map(|d| make_buffer(fs, &d.name)).collect();
        self.defs = defs;
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

    fn total_w(&self) -> f32 {
        let cols = self.cols() as f32;
        cols * TILE_W + (cols - 1.0) * GAP
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
                let col = (i % self.cols()) as f32;
                let row = (i / self.cols()) as f32;
                let min = Vec2::new(left + col * (TILE_W + GAP), top + row * (TILE_H + GAP));
                (min, min + Vec2::new(TILE_W, TILE_H))
            })
            .collect()
    }

    pub fn tick(&mut self, fs: &mut glyphon::FontSystem, dt: f32, available: bool) {
        let (genr, options) = options_snapshot();
        if genr != self.defs_gen {
            self.defs_gen = genr;
            let defs: Vec<TankClassDef> = options.iter().map(TankClassDef::from_option).collect();
            self.set_defs(fs, defs);
        }

        let target = if self.pinned || available { 1.0 } else { 0.0 };
        let t = 1.0 - (-SLIDE_SPEED * dt).exp();
        self.open += (target - self.open) * t;
        if (self.open - target).abs() < 0.002 {
            self.open = target;
        }

        self.spin += dt * SPIN_SPEED;
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

        let px_per_world = (zoom * screen.y * 0.5).max(1e-4);
        let scale = ICON_BODY_PX / (TANK_BODY_SIZE * px_per_world);

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

            let (world_pos, _) = screen_to_world(icon_c, screen, camera_pos, zoom);
            push_tank_icon(&mut instances, def, world_pos, self.spin, scale);

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
