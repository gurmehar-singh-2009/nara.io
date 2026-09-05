// so for chat:
// im gonna make it so that theres a global and team chat
// and chat messages ONLY show up in the chat log!! so no spamming
// and use the `rustrict` crate for swear censoring and put rate limits to
// chatting

use glam::Vec2;
use glyphon::{Attrs, Buffer, Family, Metrics, Shaping, TextArea, TextBounds, cosmic_text::Weight};

use crate::{
    render::{
        buffers::EntityInstance,
        colours::{DARK_THEME, to_glyphon, with_alpha},
        scoreboard::{bar_ui_instance, rounded_ui_instance},
    },
    structs::game_state::ChatChannel,
};

// could move these constants to a config file
// but wouldnt make that much sense

const LEFT: f32 = 20.0;
const TOP: f32 = 96.0;
const WIDTH: f32 = 360.0;
const PAD: f32 = 12.0;

const TAB_W: f32 = 110.0;
const TAB_H: f32 = 28.0;
const TAB_GAP: f32 = 8.0;
const TAB_FONT: f32 = 15.0;
const TAB_LINE: f32 = 19.0;

const MSG_FONT: f32 = 18.0;
const MSG_LINE: f32 = 22.0;

const INPUT_FONT: f32 = 18.0;
const INPUT_LINE: f32 = 22.0;
const INPUT_H: f32 = 34.0;

const MAX_LOG: usize = 350;
const VISIBLE_HISTORY: usize = 9;
const MAX_INPUT_CHARS: usize = 80; // can always change later
const MSG_LIFETIME_S: f64 = 10.0;
const MSG_FADE_S: f64 = 2.0;
const BLINK_MS: f64 = 500.0;
const PANEL_RADIUS: f32 = 12.0;

const TEXT_OUTLINE_PX: f32 = 1.5;

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

fn window_to_screen_scale(window: Vec2, screen: Vec2) -> f32 {
    if window.x > 0.0 {
        screen.x / window.x
    } else {
        1.0
    }
}

struct ChatMessage {
    buffer: Buffer,
    born_at: f64,
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

    pub fn receive(&mut self, fs: &mut glyphon::FontSystem, channel: ChatChannel, text: &str) {
        let list = match channel {
            ChatChannel::Global => &mut self.global,
            ChatChannel::Team => &mut self.team,
        };

        list.push(ChatMessage {
            buffer: make_buffer(fs, Metrics::new(MSG_FONT, MSG_LINE), text),
            born_at: self.now,
        });

        while list.len() > MAX_LOG {
            list.remove(0);
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

    fn panel_visible(&self) -> bool {
        if self.input_active {
            return true;
        }

        self.messages_for(self.active)
            .iter()
            .any(|m| self.message_alpha(m) > 0.01)
    }

    pub fn tick(&mut self, now: f64, _dt: f32) {
        self.now = now;

        if now - self.last_blink >= BLINK_MS {
            self.cursor_on = !self.cursor_on;
            self.last_blink = now;
        }
    }

    fn reshape_input(&mut self, fs: &mut glyphon::FontSystem) {
        self.input_buffer
            .set_text(&self.input, &bold(), Shaping::Basic, None);
        self.input_buffer.shape_until_scroll(fs, false);
    }

    pub fn hit_test_tab(&self, cursor: Vec2, window: Vec2, screen: Vec2) -> Option<ChatChannel> {
        if !self.panel_visible() {
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

        let mut shown: Vec<(&Buffer, f32)> = Vec::new();

        if self.input_active {
            let msgs = self.messages_for(self.active);
            let start = msgs.len().saturating_sub(VISIBLE_HISTORY);

            for m in &msgs[start..] {
                shown.push((&m.buffer, 1.0));
            }
        } else {
            for m in self.messages_for(self.active) {
                let a = self.message_alpha(m);

                if a > 0.01 {
                    shown.push((&m.buffer, a));
                }
            }
        }

        if shown.is_empty() && !self.input_active {
            return (instances, areas);
        }

        let log_h = shown.len() as f32 * MSG_LINE;
        let input_h = if self.input_active {
            INPUT_H + 8.0
        } else {
            0.0
        };

        let bg_h = PAD + TAB_H + 8.0 + log_h + input_h + PAD;

        instances.push(rounded_ui_instance(
            Vec2::new(LEFT + WIDTH * 0.5, TOP + bg_h * 0.5),
            Vec2::new(WIDTH, bg_h),
            screen,
            DARK_THEME.bar_background,
            DARK_THEME.scoreboard_row,
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
                DARK_THEME.bar_background
            };

            instances.push(bar_ui_instance(
                Vec2::new(tab_left + TAB_W * 0.5, tabs_top + TAB_H * 0.5),
                Vec2::new(TAB_W, TAB_H),
                screen,
                fill,
            ));

            let text_alpha = if is_active { 1.0 } else { 0.55 };
            let w = measure(buffer);

            push_outlined(
                &mut areas,
                buffer,
                tab_left + (TAB_W - w) * 0.5,
                tabs_top + (TAB_H - TAB_LINE) * 0.5,
                screen,
                text_alpha,
            );
        }

        let log_top = tabs_top + TAB_H + 8.0;

        for (i, (buffer, alpha)) in shown.iter().enumerate() {
            push_outlined(
                &mut areas,
                buffer,
                LEFT + PAD,
                log_top + i as f32 * MSG_LINE,
                screen,
                *alpha,
            );
        }

        if self.input_active {
            let row_top = log_top + log_h + 8.0;

            instances.push(bar_ui_instance(
                Vec2::new(LEFT + WIDTH * 0.5, row_top + INPUT_H * 0.5),
                Vec2::new(WIDTH - 2.0 * PAD, INPUT_H),
                screen,
                DARK_THEME.scoreboard_row,
            ));

            let text_left = LEFT + PAD + 4.0;
            let text_top = row_top + (INPUT_H - INPUT_LINE) * 0.5;

            push_outlined(
                &mut areas,
                &self.input_buffer,
                text_left,
                text_top,
                screen,
                1.0,
            );

            if self.cursor_on {
                let cursor_left = text_left + measure(&self.input_buffer) + 2.0;

                push_outlined(
                    &mut areas,
                    &self.cursor_buffer,
                    cursor_left,
                    text_top,
                    screen,
                    1.0,
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
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: screen.x as i32,
        bottom: screen.y as i32,
    };
    let outline = to_glyphon(with_alpha(DARK_THEME.fill_border, alpha));
    let fill = to_glyphon(with_alpha([1.0, 1.0, 1.0, 1.0], alpha));

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
