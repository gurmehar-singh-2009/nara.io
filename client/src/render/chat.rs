use glam::Vec2;
use glyphon::{
    Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds,
    cosmic_text::{Style, Weight},
};

use crate::{
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, to_glyphon, with_alpha},
        scoreboard::{bar_ui_instance, rounded_ui_instance},
    },
    structs::game_state::ChatChannel,
};

const LEFT: f32 = 20.0;
const TOP: f32 = 96.0;
const WIDTH: f32 = 360.0;
const PAD: f32 = 12.0;

const TAB_W: f32 = 110.0;
const TAB_H: f32 = 28.0;
const TAB_GAP: f32 = 8.0;
const TAB_FONT: f32 = 15.0;
const TAB_LINE: f32 = 19.0;
const TAB_SLIDE_SPEED: f32 = 16.0;

const MSG_FONT: f32 = 18.0;
const MSG_LINE: f32 = 22.0;
const HEADER_GAP: f32 = 2.0;

const TIME_FONT: f32 = 13.0;
const TIME_LINE: f32 = 16.0;

const INPUT_FONT: f32 = 18.0;
const INPUT_LINE: f32 = 22.0;
const INPUT_H: f32 = 34.0;
const INPUT_SPEED: f32 = 14.0;

const MAX_LOG: usize = 350;
const VISIBLE_HISTORY: usize = 9;
const MAX_SHOWN_LINES: usize = 15;
const MAX_INPUT_CHARS: usize = 80; // can always change later
const MSG_LIFETIME_S: f64 = 10.0;
const MSG_FADE_S: f64 = 2.0;
const BLINK_MS: f64 = 500.0;
const PANEL_RADIUS: f32 = 12.0;
const PANEL_SPEED: f32 = 10.0;

const MSG_SLIDE_IN_S: f64 = 0.22;
const MSG_SLIDE_PX: f32 = 26.0;
const FLASH_DECAY: f32 = 1.6;

const TEXT_OUTLINE_PX: f32 = 1.5;

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const SYSTEM_COLOR: [f32; 4] = [0.72, 0.72, 0.76, 1.0];
const TIME_COLOR: [f32; 4] = [0.85, 0.85, 0.85, 1.0];

fn bold() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif).weight(Weight::BOLD)
}

fn plain() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif)
}

fn italic() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif).style(Style::Italic)
}

fn make_buffer_attrs(
    fs: &mut glyphon::FontSystem,
    metrics: Metrics,
    text: &str,
    attrs: &Attrs,
) -> Buffer {
    let mut buffer = Buffer::new(fs, metrics);

    buffer.set_text(text, attrs, Shaping::Basic, None);
    buffer.shape_until_scroll(fs, false);

    buffer
}

fn make_buffer(fs: &mut glyphon::FontSystem, metrics: Metrics, text: &str) -> Buffer {
    make_buffer_attrs(fs, metrics, text, &bold())
}

fn measure(buffer: &Buffer) -> f32 {
    buffer.layout_runs().next().map(|r| r.line_w).unwrap_or(0.0)
}

fn window_to_screen_scale(window: Vec2, screen: Vec2) -> f32 {
    if window.x > 0.0 {
        screen.x / window.x
    } else {
        1.0
    }
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn lerp_toward(current: f32, target: f32, speed: f32, dt: f32) -> f32 {
    current + (target - current) * (1.0 - (-speed * dt).exp())
}

fn channel_index(channel: ChatChannel) -> usize {
    match channel {
        ChatChannel::Global => 0,
        ChatChannel::Team => 1,
    }
}

fn team_tag(team: u8) -> (&'static str, [f32; 4]) {
    match team {
        0 => ("[Blue]", DARK_THEME.team_blue),
        1 => ("[Red]", DARK_THEME.team_red),
        2 => ("[Purple]", DARK_THEME.team_purple),
        _ => ("[Team]", WHITE),
    }
}

fn format_hhmm(unix_secs: u64) -> String {
    let offset_min = js_sys::Date::new_0().get_timezone_offset() as i64;
    let local_secs = unix_secs as i64 - offset_min * 60;
    let mins = ((local_secs / 60) % (24 * 60) + 24 * 60) % (24 * 60);
    format!("{:02}:{:02}", mins / 60, mins % 60)
}

struct ChatMessage {
    tag: Option<Buffer>,
    name: Buffer,
    time: Buffer,
    body: Buffer,
    body_lines: usize,
    is_system: bool,
    team: u8,
    born_at: f64,
}

impl ChatMessage {
    fn line_count(&self) -> usize {
        if self.is_system {
            self.body_lines
        } else {
            1 + self.body_lines
        }
    }

    fn height(&self) -> f32 {
        if self.is_system {
            self.body_lines as f32 * MSG_LINE
        } else {
            MSG_LINE + HEADER_GAP + self.body_lines as f32 * MSG_LINE
        }
    }
}

pub struct ChatPanel {
    global: Vec<ChatMessage>,
    team: Vec<ChatMessage>,
    active: ChatChannel,
    input: String,
    input_buffer: Buffer,
    // the `_` cursor thingy
    cursor_buffer: Buffer,
    tab_global: Buffer,
    tab_team: Buffer,
    input_active: bool,
    cursor_on: bool,
    last_blink: f64,
    now: f64,

    // animation state
    panel_alpha: f32,
    input_t: f32,
    /// 0 = under Global tab, 1 = under Team tab
    tab_indicator: f32,
    /// per-tab "message arrived here while you were elsewhere" flash
    flash: [f32; 2],
}

impl ChatPanel {
    pub fn new(fs: &mut glyphon::FontSystem) -> Self {
        Self {
            global: Vec::new(),
            team: Vec::new(),
            active: ChatChannel::Global,
            input: String::new(),
            input_buffer: make_buffer(fs, Metrics::new(INPUT_FONT, INPUT_LINE), ""),
            cursor_buffer: make_buffer(fs, Metrics::new(INPUT_FONT, INPUT_LINE), "_"),
            tab_global: make_buffer(fs, Metrics::new(TAB_FONT, TAB_LINE), "Global"),
            tab_team: make_buffer(fs, Metrics::new(TAB_FONT, TAB_LINE), "Team"),
            input_active: false,
            cursor_on: true,
            last_blink: 0.0,
            now: 0.0,
            panel_alpha: 0.0,
            input_t: 0.0,
            tab_indicator: 0.0,
            flash: [0.0, 0.0],
        }
    }

    pub fn is_open(&self) -> bool {
        self.input_active
    }

    pub fn active_channel(&self) -> ChatChannel {
        self.active
    }

    pub fn set_channel(&mut self, channel: ChatChannel) {
        self.active = channel;
    }

    pub fn switch_channel(&mut self) {
        self.active = match self.active {
            ChatChannel::Global => ChatChannel::Team,
            ChatChannel::Team => ChatChannel::Global,
        };
    }

    pub fn open_input(&mut self) {
        self.input_active = true;
    }

    pub fn close_input(&mut self, fs: &mut glyphon::FontSystem) {
        self.input_active = false;
        self.input.clear();
        self.reshape_input(fs);
    }

    pub fn backspace(&mut self, fs: &mut glyphon::FontSystem) {
        self.input.pop();
        self.reshape_input(fs);
    }

    pub fn type_text(&mut self, fs: &mut glyphon::FontSystem, text: &str) {
        for c in text.chars() {
            if (c as u32) < 0x20 {
                continue;
            }

            if self.input.chars().count() >= MAX_INPUT_CHARS {
                break;
            }

            self.input.push(c);
        }

        self.reshape_input(fs);
    }

    pub fn submit(&mut self, fs: &mut glyphon::FontSystem) -> Option<String> {
        let msg = if self.input.is_empty() {
            None
        } else {
            Some(self.input.clone())
        };

        self.close_input(fs);

        msg
    }

    pub fn receive(
        &mut self,
        fs: &mut glyphon::FontSystem,
        channel: ChatChannel,
        team: u8,
        sender: &str,
        text: &str,
        timestamp: u64,
    ) {
        let is_system = sender.is_empty();

        let tag = if is_system || channel != ChatChannel::Global {
            None
        } else {
            let (label, _) = team_tag(team);
            Some(make_buffer_attrs(
                fs,
                Metrics::new(TAB_FONT, TAB_LINE),
                label,
                &bold(),
            ))
        };

        let name = make_buffer_attrs(
            fs,
            Metrics::new(MSG_FONT, MSG_LINE),
            if is_system { "" } else { sender },
            &bold(),
        );

        let time = make_buffer_attrs(
            fs,
            Metrics::new(TIME_FONT, TIME_LINE),
            &format_hhmm(timestamp),
            &plain(),
        );

        let mut body = Buffer::new(fs, Metrics::new(MSG_FONT, MSG_LINE));
        body.set_size(Some(WIDTH - 2.0 * PAD), None);
        let body_attrs = if is_system { italic() } else { plain() };
        body.set_text(text, &body_attrs, Shaping::Basic, None);
        body.shape_until_scroll(fs, false);
        let body_lines = body.layout_runs().count().max(1);

        let list = match channel {
            ChatChannel::Global => &mut self.global,
            ChatChannel::Team => &mut self.team,
        };

        list.push(ChatMessage {
            tag,
            name,
            time,
            body,
            body_lines,
            is_system,
            team,
            born_at: self.now,
        });

        while list.len() > MAX_LOG {
            list.remove(0);
        }

        if channel != self.active {
            self.flash[channel_index(channel)] = 1.0;
        }
    }

    fn messages_for(&self, channel: ChatChannel) -> &Vec<ChatMessage> {
        match channel {
            ChatChannel::Global => &self.global,
            ChatChannel::Team => &self.team,
        }
    }

    fn message_alpha(&self, m: &ChatMessage) -> f32 {
        if self.input_active {
            return 1.0;
        }

        let age = self.now - m.born_at;

        (((MSG_LIFETIME_S - age) / MSG_FADE_S) as f32).clamp(0.0, 1.0)
    }

    pub fn tick(&mut self, now: f64, dt: f32) {
        self.now = now;

        if now - self.last_blink >= BLINK_MS {
            self.cursor_on = !self.cursor_on;
            self.last_blink = now;
        }

        let any_visible = self
            .messages_for(self.active)
            .iter()
            .any(|m| self.message_alpha(m) > 0.01);

        let panel_target =
            if self.input_active || any_visible || self.flash[0] > 0.05 || self.flash[1] > 0.05 {
                1.0
            } else {
                0.0
            };
        self.panel_alpha = lerp_toward(self.panel_alpha, panel_target, PANEL_SPEED, dt);

        let input_target = if self.input_active { 1.0 } else { 0.0 };
        self.input_t = lerp_toward(self.input_t, input_target, INPUT_SPEED, dt);

        let tab_target = match self.active {
            ChatChannel::Global => 0.0,
            ChatChannel::Team => 1.0,
        };
        self.tab_indicator = lerp_toward(self.tab_indicator, tab_target, TAB_SLIDE_SPEED, dt);

        for f in self.flash.iter_mut() {
            *f = (*f - dt * FLASH_DECAY).max(0.0);
        }
    }

    fn reshape_input(&mut self, fs: &mut glyphon::FontSystem) {
        self.input_buffer
            .set_text(&self.input, &bold(), Shaping::Basic, None);
        self.input_buffer.shape_until_scroll(fs, false);
    }

    pub fn hit_test_tab(&self, cursor: Vec2, window: Vec2, screen: Vec2) -> Option<ChatChannel> {
        if self.panel_alpha < 0.3 {
            return None;
        }

        let scale = window_to_screen_scale(window, screen);
        let c = cursor * scale;

        let tabs_top = TOP + PAD;

        for i in 0..2usize {
            let tab_left = LEFT + i as f32 * (TAB_W + TAB_GAP);

            if c.x >= tab_left
                && c.x <= tab_left + TAB_W
                && c.y >= tabs_top
                && c.y <= tabs_top + TAB_H
            {
                return Some(if i == 0 {
                    ChatChannel::Global
                } else {
                    ChatChannel::Team
                });
            }
        }

        None
    }

    pub fn render_data(&self, screen: Vec2) -> (Vec<EntityInstance>, Vec<TextArea<'_>>) {
        let mut instances = Vec::new();
        let mut areas = Vec::new();

        if self.panel_alpha <= 0.02 {
            return (instances, areas);
        }

        let alpha_p = self.panel_alpha;

        let msgs = self.messages_for(self.active);
        let mut shown: Vec<&ChatMessage> = Vec::new();
        let mut lines = 0usize;

        for m in msgs.iter().rev() {
            if self.input_active && shown.len() >= VISIBLE_HISTORY {
                break;
            }
            if !self.input_active && self.message_alpha(m) <= 0.01 {
                continue;
            }
            if lines >= MAX_SHOWN_LINES {
                break;
            }
            shown.push(m);
            lines += m.line_count();
        }
        shown.reverse();

        let log_h: f32 = shown.iter().map(|m| m.height()).sum();
        let input_h = (INPUT_H + 8.0) * self.input_t;

        let bg_h = PAD + TAB_H + 8.0 + log_h + input_h + PAD;

        instances.push(rounded_ui_instance(
            Vec2::new(LEFT + WIDTH * 0.5, TOP + bg_h * 0.5),
            Vec2::new(WIDTH, bg_h),
            screen,
            with_alpha(DARK_THEME.bar_background, alpha_p),
            with_alpha(DARK_THEME.scoreboard_row, alpha_p),
            4.0,
            PANEL_RADIUS,
        ));

        let tabs_top = TOP + PAD;
        let tabs: [(&Buffer, ChatChannel); 2] = [
            (&self.tab_global, ChatChannel::Global),
            (&self.tab_team, ChatChannel::Team),
        ];

        for (i, (buffer, channel)) in tabs.iter().enumerate() {
            let tab_left = LEFT + i as f32 * (TAB_W + TAB_GAP);
            let is_active = *channel == self.active;

            let fill = if is_active {
                DARK_THEME.scoreboard_row
            } else {
                mix(DARK_THEME.bar_background, WHITE, self.flash[i] * 0.4)
            };

            instances.push(bar_ui_instance(
                Vec2::new(tab_left + TAB_W * 0.5, tabs_top + TAB_H * 0.5),
                Vec2::new(TAB_W, TAB_H),
                screen,
                with_alpha(fill, alpha_p),
            ));

            let text_alpha = if is_active {
                1.0
            } else {
                0.55 + self.flash[i] * 0.45
            };
            let w = measure(buffer);

            push_outlined(
                &mut areas,
                buffer,
                tab_left + (TAB_W - w) * 0.5,
                tabs_top + (TAB_H - TAB_LINE) * 0.5,
                screen,
                text_alpha * alpha_p,
            );
        }

        let ind_left = LEFT + self.tab_indicator * (TAB_W + TAB_GAP);
        instances.push(bar_ui_instance(
            Vec2::new(ind_left + TAB_W * 0.5, tabs_top + TAB_H + 1.5),
            Vec2::new(TAB_W, 3.0),
            screen,
            with_alpha(WHITE, alpha_p),
        ));

        let log_top = tabs_top + TAB_H + 8.0;
        let mut y = log_top;

        for m in shown {
            let age = self.now - m.born_at;
            let t_in = (age / MSG_SLIDE_IN_S).clamp(0.0, 1.0) as f32;
            let eased = 1.0 - (1.0 - t_in).powi(3);
            let x_off = -MSG_SLIDE_PX * (1.0 - eased);
            let alpha = self.message_alpha(m) * eased * alpha_p;

            if alpha <= 0.01 {
                continue;
            }

            if m.is_system {
                push_outlined_color(
                    &mut areas,
                    &m.body,
                    LEFT + PAD + x_off,
                    y,
                    screen,
                    with_alpha(SYSTEM_COLOR, alpha),
                );
                y += m.height();
                continue;
            }

            let mut x = LEFT + PAD + x_off;
            if let Some(tag) = &m.tag {
                let (_, color) = team_tag(m.team);
                push_outlined_color(
                    &mut areas,
                    tag,
                    x,
                    y + (MSG_LINE - TAB_LINE) * 0.5,
                    screen,
                    with_alpha(color, alpha),
                );
                x += measure(tag) + 6.0;
            }

            push_outlined_color(&mut areas, &m.name, x, y, screen, with_alpha(WHITE, alpha));

            let time_w = measure(&m.time);
            push_outlined_color(
                &mut areas,
                &m.time,
                LEFT + WIDTH - PAD - time_w,
                y + (MSG_LINE - TIME_LINE) * 0.5,
                screen,
                with_alpha(TIME_COLOR, alpha * 0.65),
            );

            y += MSG_LINE + HEADER_GAP;

            push_outlined_color(
                &mut areas,
                &m.body,
                LEFT + PAD + x_off,
                y,
                screen,
                with_alpha(WHITE, alpha),
            );

            y += m.body_lines as f32 * MSG_LINE;
        }

        if self.input_t > 0.02 {
            let row_top = log_top + log_h + 8.0 * self.input_t;
            let h = INPUT_H * self.input_t;

            instances.push(bar_ui_instance(
                Vec2::new(LEFT + WIDTH * 0.5, row_top + h * 0.5),
                Vec2::new(WIDTH - 2.0 * PAD, h),
                screen,
                with_alpha(DARK_THEME.scoreboard_row, alpha_p),
            ));

            let text_left = LEFT + PAD + 4.0;
            let text_top = row_top + (INPUT_H - INPUT_LINE) * 0.5;
            let a = self.input_t * alpha_p;

            push_outlined(
                &mut areas,
                &self.input_buffer,
                text_left,
                text_top,
                screen,
                a,
            );

            if self.cursor_on {
                let cursor_left = text_left + measure(&self.input_buffer) + 2.0;

                push_outlined(
                    &mut areas,
                    &self.cursor_buffer,
                    cursor_left,
                    text_top,
                    screen,
                    a,
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
    alpha: f32,
) {
    push_outlined_color(areas, buffer, left, top, screen, with_alpha(WHITE, alpha));
}

fn push_outlined_color<'a>(
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
    let outline = to_glyphon(with_alpha(DARK_THEME.fill_border, color[3]));
    let fill = to_glyphon(color);

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
