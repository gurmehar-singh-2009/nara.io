use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use glam::Vec2;
use glyphon::{
    Attrs, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, cosmic_text::Weight,
};
use shared::packets::{NO_TEAM, camera_zoom};
use wasm_bindgen::{JsCast, closure::Closure, prelude::*};
use wasm_bindgen_futures::spawn_local;
use web_sys::window;
use wgpu::{
    Adapter, Backends, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, Buffer, BufferBindingType, BufferUsages, ColorTargetState,
    ColorWrites, CurrentSurfaceTexture, Device, DeviceDescriptor, ExperimentalFeatures, Features,
    FragmentState, Instance, InstanceDescriptor, MultisampleState, PipelineCompilationOptions,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPipeline, RenderPipelineDescriptor,
    RequestAdapterOptions, ShaderModuleDescriptor, ShaderStages, Surface, SurfaceConfiguration,
    VertexState, util::DeviceExt,
};
use winit::{
    application::ApplicationHandler,
    event::{KeyEvent, MouseButton, WindowEvent},
    event_loop::{EventLoop, EventLoopProxy},
    keyboard::{KeyCode, PhysicalKey},
    platform::web::*,
    window::Window,
};

use crate::{
    entities::Entity,
    render::{
        buffers::{CameraUniform, EntityInstance},
        camera::{CameraController, SpringPos},
        chat::ChatPanel,
        colours::{DARK_THEME, team_color, to_glyphon, with_alpha},
        death_screen::DeathScreen,
        minimap,
        scoreboard::{Scoreboard, pill_instance, rounded_ui_instance},
        tank_upgrades::{TankUpgradePanel, default_classes},
        upgrade_panel::UpgradePanel,
    },
    structs::game_state::GameState,
};

const PLAYER_DISPLAY_SMOOTH_TIME: f32 = 0.06;
const INSTANCE_CAPACITY: u64 = 4096;

const PLAYER_ZOOM: f32 = shared::packets::BASE_ZOOM;

const BULLET_STALE_FADE_MS: f64 = 200.0;
const BULLET_STALE_CULL_MS: f64 = 900.0;
const PLAYER_STALE_MS: f64 = 1500.0;
const DEATH_FALLBACK_MS: f64 = 1500.0;
const TRAP_SPIN: f64 = 0.6;
const SPECTATE_ZOOM_MULT: f32 = 0.8;
const SPECTATE_FOLLOW_TIME: f32 = 0.35;

const CULL_MARGIN: f32 = 150.0;
const BULLET_CULL_RADIUS: f32 = 64.0;

const LEVEL_BAR_W_FRAC: f32 = 0.34;
const LEVEL_BAR_MAX_W: f32 = 560.0;
const SCORE_BAR_W_RATIO: f32 = 0.72;
const SCORE_BAR_H: f32 = 26.0;
const LEVEL_BAR_BOTTOM: f32 = 34.0;
const SCORE_TEXT_OUTLINE_PX: f32 = 2.0;
const HUD_BAR_SPEED: f32 = 8.0;
const HUD_BAR_OUTLINE: f32 = 3.0;
const HUD_BAR_TRACK: [f32; 4] = [0.2, 0.2, 0.2, 0.85];
const HUD_BAR_EDGE: [f32; 4] = [0.08, 0.08, 0.08, 0.95];
const NAME_LABEL_FONT: f32 = 24.0;
const NAME_LABEL_LINE: f32 = 28.0;
const NAME_LABEL_GAP: f32 = 6.0;

const HEALTH_BAR_BORDER: f32 = 1.5;
const HEALTH_BAR_INSET: f32 = 2.5;
const HEALTH_BAR_FADE_SPEED: f32 = 10.0;
const LEVEL_BAR_H: f32 = 30.0;
const LEVEL_BAR_GAP: f32 = 6.0;

const OUTLINE_DIRS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (1.0, 1.0),
    (1.0, -1.0),
    (-1.0, 1.0),
    (-1.0, -1.0),
];

const NOTICE_TIME: f32 = 2.5;
const NOTICE_FADE: f32 = 0.4;
const NOTICE_TOP: f32 = 16.0;
const NOTICE_H: f32 = 34.0;
const NOTICE_GAP: f32 = 8.0;
const NOTICE_SLIDE_SPEED: f32 = 14.0;
const MAX_NOTICES: usize = 4;

struct Notice {
    buffer: glyphon::Buffer,
    time: f32,
    y: f32,
}

impl Notice {
    fn alpha(&self) -> f32 {
        let fade_in = ((NOTICE_TIME - self.time) / 0.15).clamp(0.0, 1.0);
        (self.time / NOTICE_FADE).clamp(0.0, 1.0) * fade_in
    }
}

#[derive(Default)]
struct Notices {
    entries: Vec<Notice>,
}

impl Notices {
    fn bottom(&self) -> f32 {
        let n = self.entries.len() as f32;
        if n == 0.0 {
            return 0.0;
        }
        NOTICE_TOP + n * NOTICE_H + (n - 1.0) * NOTICE_GAP
    }

    fn push(&mut self, fs: &mut FontSystem, text: &str) {
        let mut buffer = glyphon::Buffer::new(fs, Metrics::new(20.0, 24.0));

        buffer.set_text(text, &bold_attrs(), Shaping::Basic, None);

        buffer.shape_until_scroll(fs, false);

        self.entries.insert(
            0,
            Notice {
                buffer,
                time: NOTICE_TIME,
                y: NOTICE_TOP - NOTICE_H,
            },
        );

        self.entries.truncate(MAX_NOTICES);
    }

    fn tick(&mut self, dt: f32) {
        let t = 1.0 - (-NOTICE_SLIDE_SPEED * dt).exp();

        for (i, notice) in self.entries.iter_mut().enumerate() {
            let target = NOTICE_TOP + NOTICE_H * 0.5 + i as f32 * (NOTICE_H + NOTICE_GAP);

            notice.y += (target - notice.y) * t;

            notice.time -= dt;
        }

        self.entries.retain(|n| n.time > 0.0);
    }
}

struct HudLayout {
    cx: f32,
    level_w: f32,
    level_cy: f32,
    score_w: f32,
    score_cy: f32,
    name_cy: f32,
}

#[allow(clippy::too_many_arguments)]
fn push_centered_text<'a>(
    areas: &mut Vec<TextArea<'a>>,
    buffer: &'a glyphon::Buffer,
    cx: f32,
    cy: f32,
    outline_px: f32,
    outline: Color,
    fill: Color,
    bounds: TextBounds,
) {
    let (w, h) = Renderer::text_dimensions(buffer);

    let left = cx - w * 0.5;
    let top = cy - h * 0.5;

    for (dx, dy) in OUTLINE_DIRS {
        areas.push(TextArea {
            buffer,
            left: left + dx * outline_px,
            top: top + dy * outline_px,
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

fn bold_attrs() -> Attrs<'static> {
    Attrs::new().family(Family::SansSerif).weight(Weight::BOLD)
}

#[derive(Clone)]
pub struct RenderEntity<'a> {
    pub instance: EntityInstance,
    pub text: Option<&'a TextComponent>,
}

pub struct RenderState {
    window: Arc<Window>,
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    is_surface_configured: bool,
    render_pipeline: RenderPipeline,
    instance_buffer: Buffer,
    camera_buffer: Buffer,
    camera_bind_group: BindGroup,
    num_instances: u32,
    world_instances: u32,

    scratch_instances: Vec<EntityInstance>,

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    world_text_renderer: TextRenderer,
    text_buffer: glyphon::Buffer,

    stats_text: String,
    last_stats_text: String,
    score_bar_buffer: glyphon::Buffer,
    score_text: String,
    last_score_bar_text: String,
    level_bar_buffer: glyphon::Buffer,
    level_text: String,
    last_level_text: String,
    name_bar_buffer: glyphon::Buffer,
    name_text: String,
    last_name_text: String,
    score_fill: f32,
    level_fill: f32,
    show_debug: bool,
    notices: Notices,
    net_buffers: Vec<glyphon::Buffer>,
    net_text: [String; 2],
    last_net_text: [String; 2],
    spectate_follow: SpringPos,
    debug_buffer: glyphon::Buffer,
    debug_text: String,
    debug_dirty: bool,
    debug_text_changed: bool,

    debug_frames: u32,
    debug_fps: f64,
    debug_frame_ms: f32,
    debug_last: f64,
    last_upgrade_levels: [u8; 8],
    scoreboard: Scoreboard,
    upgrade_panel: UpgradePanel,
    chat: ChatPanel,
    class_panel: TankUpgradePanel,
    death: DeathScreen,

    player_display: SpringPos,
    camera: CameraController,
    camera_enabled: bool,
    had_player: bool,
    last_frame_time: Option<f64>,
    cursor_pos: Option<Vec2>,

    debug_render_mode: u8,
    log_frames: u32,
    last_log_time: f64,

    game_state: Rc<RefCell<GameState>>,
}

impl RenderState {
    async fn create_surface_and_adapter(window: Arc<Window>) -> (Surface<'static>, Adapter) {
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::GL,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create WebGL2 surface");

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: true,
            })
            .await
            .expect("neither WebGPU nor WebGL2 is available");

        web_sys::console::log_1(&"wgpu backend: WebGL2".into());

        (surface, adapter)
    }

    fn pick_surface_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
        const PREFERRED: &[wgpu::TextureFormat] = &[
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ];

        PREFERRED
            .iter()
            .copied()
            .find(|f| formats.contains(f))
            .unwrap_or(formats[0])
    }

    fn upload_instances(device: &Device, queue: &Queue, buffer: &mut Buffer, raw_data: &[u8]) {
        let required_size = raw_data.len() as u64;

        if required_size == 0 {
            return;
        }

        if required_size > buffer.size() {
            let new_size = required_size.next_power_of_two();

            *buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Dynamic Instance Buffer"),
                size: new_size,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        queue.write_buffer(buffer, 0, raw_data);
    }

    pub async fn new(window: Arc<Window>, game: Rc<RefCell<GameState>>) -> Self {
        let size = window.inner_size();

        let (surface, adapter) = Self::create_surface_and_adapter(window.clone()).await;

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: None,
                required_features: Features::empty(),
                experimental_features: ExperimentalFeatures::disabled(),
                required_limits: adapter.limits(),
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = Self::pick_surface_format(&surface_caps.formats);

        let alpha_mode = if surface_caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Opaque)
        {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            surface_caps.alpha_modes[0]
        };

        let max_dim = device.limits().max_texture_dimension_2d;
        let physical_width = size.width.clamp(1, max_dim);
        let physical_height = size.height.clamp(1, max_dim);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: physical_width,
            height: physical_height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };

        let aspect_ratio = if physical_height > 0 {
            physical_width as f32 / physical_height as f32
        } else {
            1.0
        };

        let camera_uniform = CameraUniform {
            view_proj: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            camera_pos: [0.0, 0.0],
            zoom: PLAYER_ZOOM,
            aspect_ratio,
            screen_size: [physical_width as f32, physical_height as f32],
            _pad: [0.0, 0.0],
        };

        let camera_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("Camera Bind Group Layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let camera_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: &camera_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let vs_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Vertex Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./shader/vertex.wgsl").into()),
        });

        let fs_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Fragment Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./shader/fragment.wgsl").into()),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[Some(&camera_bind_group_layout)],
            immediate_size: 0,
        });

        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: VertexState {
                module: &vs_module,
                entry_point: Some("vs_main"),
                buffers: &[Some(EntityInstance::desc())],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &fs_module,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
            primitive: PrimitiveState::default(),
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Instance Buffer"),
            size: INSTANCE_CAPACITY * std::mem::size_of::<EntityInstance>() as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut font_system = FontSystem::new();

        font_system
            .db_mut()
            .load_font_data(include_bytes!("../../assets/Exo2.ttf").to_vec());

        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);

        let mut atlas = TextAtlas::new(&device, &queue, &cache, surface_format);

        let text_renderer = TextRenderer::new(
            &mut atlas,
            &device,
            MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            None,
        );

        let world_text_renderer = TextRenderer::new(
            &mut atlas,
            &device,
            MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            None,
        );

        let mut viewport = Viewport::new(&device, &cache);

        viewport.update(
            &queue,
            Resolution {
                width: physical_width,
                height: physical_height,
            },
        );

        let mut text_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(48.0, 56.0));

        text_buffer.set_size(Some(physical_width as f32), Some(physical_height as f32));

        text_buffer.set_text(
            "son",
            &Attrs::new().family(glyphon::Family::SansSerif),
            Shaping::Basic,
            None,
        );

        text_buffer.shape_until_scroll(&mut font_system, false);

        let scoreboard = Scoreboard::new(&mut font_system);
        let upgrade_panel = UpgradePanel::new(&mut font_system);
        let chat = ChatPanel::new(&mut font_system);
        let class_panel = TankUpgradePanel::new(&mut font_system, default_classes());
        let death = DeathScreen::new(&mut font_system);

        let mut score_bar_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(18.0, 22.0));

        score_bar_buffer.set_size(Some(physical_width as f32), Some(physical_height as f32));

        score_bar_buffer.set_text("", &bold_attrs(), Shaping::Basic, None);
        score_bar_buffer.shape_until_scroll(&mut font_system, false);

        let mut level_bar_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(20.0, 24.0));

        level_bar_buffer.set_size(Some(physical_width as f32), Some(physical_height as f32));

        level_bar_buffer.set_text("", &bold_attrs(), Shaping::Basic, None);
        level_bar_buffer.shape_until_scroll(&mut font_system, false);

        let net_buffers = (0..2)
            .map(|_| glyphon::Buffer::new(&mut font_system, Metrics::new(14.0, 18.0)))
            .collect();

        let mut name_bar_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(40.0, 46.0));

        name_bar_buffer.set_size(Some(physical_width as f32), Some(physical_height as f32));

        name_bar_buffer.set_text("", &bold_attrs(), Shaping::Basic, None);
        name_bar_buffer.shape_until_scroll(&mut font_system, false);

        let mut debug_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(26.0, 30.0));

        debug_buffer.set_size(Some(physical_width as f32), Some(physical_height as f32));

        debug_buffer.set_text(
            "",
            &Attrs::new().family(glyphon::Family::SansSerif),
            Shaping::Basic,
            None,
        );

        debug_buffer.shape_until_scroll(&mut font_system, false);

        let mut camera = CameraController::new();
        camera.snap_zoom(PLAYER_ZOOM);

        Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            window,
            render_pipeline,
            instance_buffer,
            camera_buffer,
            camera_bind_group,
            num_instances: 0,
            world_instances: 0,
            scratch_instances: Vec::with_capacity(1024),
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            world_text_renderer,
            text_buffer,
            stats_text: String::with_capacity(64),
            last_stats_text: String::with_capacity(64),
            score_bar_buffer,
            score_text: String::with_capacity(32),
            last_score_bar_text: String::with_capacity(32),
            level_bar_buffer,
            level_text: String::with_capacity(16),
            last_level_text: String::with_capacity(16),
            name_bar_buffer,
            name_text: String::with_capacity(32),
            last_name_text: String::with_capacity(32),
            score_fill: 0.0,
            level_fill: 0.0,
            show_debug: false,
            notices: Notices::default(),
            net_buffers,
            net_text: Default::default(),
            last_net_text: Default::default(),
            spectate_follow: SpringPos::new(SPECTATE_FOLLOW_TIME),
            debug_buffer,
            debug_text: String::with_capacity(160),
            debug_dirty: true,
            debug_text_changed: false,
            debug_frames: 0,
            debug_fps: 0.0,
            debug_frame_ms: 0.0,
            debug_last: 0.0,
            last_upgrade_levels: [0; 8],
            scoreboard,
            upgrade_panel,
            chat,
            class_panel,
            death,
            player_display: SpringPos::new(PLAYER_DISPLAY_SMOOTH_TIME),
            camera,
            camera_enabled: true,
            had_player: false,
            last_frame_time: None,
            cursor_pos: None,
            debug_render_mode: 0,
            log_frames: 0,
            last_log_time: 0.0,
            game_state: game,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            self.is_surface_configured = false;
            return;
        }

        let max_dim = self.device.limits().max_texture_dimension_2d;
        let width = width.clamp(1, max_dim);
        let height = height.clamp(1, max_dim);

        if self.is_surface_configured && self.config.width == width && self.config.height == height
        {
            return;
        }

        self.config.width = width;
        self.config.height = height;

        self.surface.configure(&self.device, &self.config);
        self.is_surface_configured = true;

        self.viewport
            .update(&self.queue, Resolution { width, height });

        self.text_buffer
            .set_size(Some(width as f32), Some(height as f32));

        self.score_bar_buffer
            .set_size(Some(width as f32), Some(height as f32));

        self.level_bar_buffer
            .set_size(Some(width as f32), Some(height as f32));

        self.name_bar_buffer
            .set_size(Some(width as f32), Some(height as f32));

        self.debug_buffer
            .set_size(Some(width as f32), Some(height as f32));

        self.update_camera([self.camera.pos.x, self.camera.pos.y], self.camera.zoom);
    }

    pub fn update(&mut self, instances: &[EntityInstance]) {
        self.num_instances = instances.len() as u32;
        self.world_instances = self.num_instances;

        if instances.is_empty() {
            return;
        }

        let raw_data = bytemuck::cast_slice(instances);

        Self::upload_instances(
            &self.device,
            &self.queue,
            &mut self.instance_buffer,
            raw_data,
        );
    }

    fn update_camera(&self, camera_pos: [f32; 2], zoom: f32) {
        let aspect_ratio = self.config.width as f32 / self.config.height.max(1) as f32;

        let safe_zoom = if zoom.is_finite() && zoom > 0.0 {
            zoom
        } else {
            PLAYER_ZOOM
        };

        let safe_aspect = if aspect_ratio.is_finite() && aspect_ratio > 0.0 {
            aspect_ratio
        } else {
            1.0
        };

        let camera = CameraUniform {
            view_proj: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            camera_pos,
            zoom: safe_zoom,
            aspect_ratio: safe_aspect,
            screen_size: [self.config.width as f32, self.config.height.max(1) as f32],
            _pad: [0.0, 0.0],
        };

        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
    }

    pub fn render_entities_with_text(
        &mut self,
        entities: &[RenderEntity],
        bubble_anchors: &[(u32, Vec2, f32)],
        camera_pos: [f32; 2],
        zoom: f32,
        upgrade_points: u32,
        upgrade_levels: &[u8; 8],
        show_hud: bool,
    ) {
        self.window.request_redraw();

        if !self.is_surface_configured {
            return;
        }

        let mode = self.debug_render_mode;

        if mode < 2 {
            self.update_camera(camera_pos, zoom);
        }

        let screen_w = self.config.width as f32;
        let screen_h = self.config.height as f32;
        let screen = Vec2::new(screen_w, screen_h);

        let (ui_instances, scoreboard_areas) = if mode == 0 && show_hud {
            let me = Some(self.name_text.as_str()).filter(|n| !n.is_empty());

            self.scoreboard.render_data(screen, me)
        } else {
            (Vec::new(), Vec::new())
        };

        let (panel_instances, panel_areas) = if mode == 0 && show_hud {
            self.upgrade_panel
                .render_data(screen, upgrade_levels, upgrade_points)
        } else {
            (Vec::new(), Vec::new())
        };

        let (chat_instances, chat_areas) = if mode == 0 && show_hud {
            self.chat.render_data(screen)
        } else {
            (Vec::new(), Vec::new())
        };

        let (class_instances, class_areas) = if mode == 0 && show_hud {
            self.class_panel.render_data(screen, camera_pos, zoom)
        } else {
            (Vec::new(), Vec::new())
        };

        let mut all_instances = std::mem::take(&mut self.scratch_instances);

        all_instances.clear();

        all_instances.reserve(
            entities.len()
                + ui_instances.len()
                + panel_instances.len()
                + chat_instances.len()
                + class_instances.len()
                + 64,
        );

        all_instances.extend(entities.iter().map(|e| e.instance));

        let (death_instances, death_areas) = if mode == 0 {
            self.death.render_data(screen, camera_pos, zoom)
        } else {
            (Vec::new(), Vec::new())
        };

        let world_text = !self.death.is_active();

        let mut bubble_rects: Vec<(usize, f32, f32, f32, f32, f32)> = Vec::new();

        if mode == 0 && world_text {
            let bubbles = self.chat.bubbles();

            if !bubbles.is_empty() {
                let aspect_ratio = if screen_h > 0.0 {
                    screen_w / screen_h
                } else {
                    1.0
                };

                let mut bubble_stack_offsets: HashMap<u32, f32> = HashMap::new();

                for bi in (0..bubbles.len()).rev() {
                    let bubble = &bubbles[bi];
                    let alpha = self.chat.bubble_alpha(bi).clamp(0.0, 1.0);

                    if alpha <= 0.01 {
                        continue;
                    }

                    let Some((_, player_pos, player_scale)) = bubble_anchors
                        .iter()
                        .find(|(id, _, _)| *id == bubble.sender_id)
                    else {
                        continue;
                    };

                    let (sx, sy) = Renderer::world_to_screen(
                        [player_pos.x, player_pos.y],
                        camera_pos,
                        zoom,
                        aspect_ratio,
                        screen_w,
                        screen_h,
                    );

                    let body_r_px = 21.0 * player_scale.max(0.001) * zoom * screen_h * 0.5;
                    let label_scale = Renderer::label_scale(body_r_px);
                    let label_top = sy - body_r_px - NAME_LABEL_GAP - NAME_LABEL_LINE * label_scale;

                    let (text_w, text_h) = Renderer::text_dimensions(&bubble.body);

                    if !text_w.is_finite() || !text_h.is_finite() || text_w <= 0.0 || text_h <= 0.0
                    {
                        continue;
                    }

                    const BUBBLE_PAD_X: f32 = 16.0;
                    const BUBBLE_PAD_Y: f32 = 10.0;
                    const BUBBLE_GAP: f32 = 6.0;
                    const BUBBLE_MIN_W: f32 = 32.0;
                    const BUBBLE_MIN_H: f32 = 24.0;
                    const BUBBLE_RISE: f32 = 10.0;
                    const SCREEN_MARGIN: f32 = 8.0;

                    let max_w = (screen_w - SCREEN_MARGIN * 2.0).max(BUBBLE_MIN_W);

                    let max_h = (screen_h - SCREEN_MARGIN * 2.0).max(BUBBLE_MIN_H);

                    let w = (text_w + BUBBLE_PAD_X).max(BUBBLE_MIN_W).min(max_w);

                    let h = (text_h + BUBBLE_PAD_Y).max(BUBBLE_MIN_H).min(max_h);

                    let pop = self.chat.bubble_pop(bi);

                    let stack_offset = bubble_stack_offsets.entry(bubble.sender_id).or_insert(0.0);

                    let bottom = label_top - BUBBLE_GAP - *stack_offset + (1.0 - pop) * BUBBLE_RISE;

                    let top = (bottom - h).clamp(
                        SCREEN_MARGIN,
                        (screen_h - h - SCREEN_MARGIN).max(SCREEN_MARGIN),
                    );

                    let left = (sx - w * 0.5).clamp(
                        SCREEN_MARGIN,
                        (screen_w - w - SCREEN_MARGIN).max(SCREEN_MARGIN),
                    );

                    all_instances.push(rounded_ui_instance(
                        Vec2::new(left + w * 0.5, top + h * 0.5),
                        Vec2::new(w, h),
                        screen,
                        with_alpha(DARK_THEME.bar_background, alpha),
                        with_alpha(DARK_THEME.scoreboard_row_border, alpha),
                        3.0,
                        8.0,
                    ));

                    bubble_rects.push((bi, left, top, w, h, alpha));

                    *stack_offset += (h + 4.0) * pop;
                }
            }
        }

        self.world_instances = all_instances.len() as u32;

        if mode == 0 && show_hud {
            for notice in self.notices.entries.iter() {
                let (w, _) = Renderer::text_dimensions(&notice.buffer);
                let alpha = notice.alpha();

                all_instances.push(rounded_ui_instance(
                    Vec2::new(screen_w * 0.5, notice.y),
                    Vec2::new(w + 32.0, NOTICE_H),
                    screen,
                    with_alpha(DARK_THEME.bar_background, alpha),
                    with_alpha(DARK_THEME.scoreboard_row_border, alpha),
                    3.0,
                    8.0,
                ));
            }
        }
        all_instances.extend(ui_instances);
        all_instances.extend(panel_instances);
        all_instances.extend(chat_instances);
        all_instances.extend(class_instances);
        all_instances.extend(death_instances);

        self.num_instances = all_instances.len() as u32;

        if mode < 2 && !all_instances.is_empty() {
            let raw_data = bytemuck::cast_slice(&all_instances);

            Self::upload_instances(
                &self.device,
                &self.queue,
                &mut self.instance_buffer,
                raw_data,
            );
        }

        self.scratch_instances = all_instances;

        let output = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture)
            | CurrentSurfaceTexture::Suboptimal(texture) => texture,

            CurrentSurfaceTexture::Timeout
            | CurrentSurfaceTexture::Occluded
            | CurrentSurfaceTexture::Validation => {
                return;
            }

            CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                self.is_surface_configured = true;
                return;
            }

            CurrentSurfaceTexture::Lost => {
                self.is_surface_configured = false;
                return;
            }
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format),
            ..Default::default()
        });

        if mode == 0 && self.stats_text != self.last_stats_text {
            self.text_buffer.set_text(
                &self.stats_text,
                &Attrs::new().family(glyphon::Family::SansSerif),
                Shaping::Basic,
                None,
            );

            self.text_buffer
                .shape_until_scroll(&mut self.font_system, false);

            self.last_stats_text.clone_from(&self.stats_text);
        }

        if mode == 0 && self.score_text != self.last_score_bar_text {
            self.score_bar_buffer
                .set_text(&self.score_text, &bold_attrs(), Shaping::Basic, None);

            self.score_bar_buffer
                .shape_until_scroll(&mut self.font_system, false);

            self.last_score_bar_text.clone_from(&self.score_text);
        }

        for i in 0..self.net_buffers.len() {
            if mode == 0 && self.net_text[i] != self.last_net_text[i] {
                self.net_buffers[i].set_text(
                    &self.net_text[i],
                    &bold_attrs(),
                    Shaping::Basic,
                    None,
                );

                self.net_buffers[i].shape_until_scroll(&mut self.font_system, false);

                self.last_net_text[i].clone_from(&self.net_text[i]);
            }
        }

        if mode == 0 && self.name_text != self.last_name_text {
            self.name_bar_buffer
                .set_text(&self.name_text, &bold_attrs(), Shaping::Basic, None);

            self.name_bar_buffer
                .shape_until_scroll(&mut self.font_system, false);

            self.last_name_text.clone_from(&self.name_text);
        }

        if mode == 0 && self.level_text != self.last_level_text {
            self.level_bar_buffer
                .set_text(&self.level_text, &bold_attrs(), Shaping::Basic, None);

            self.level_bar_buffer
                .shape_until_scroll(&mut self.font_system, false);

            self.last_level_text.clone_from(&self.level_text);
        }

        if mode == 0 && self.debug_text_changed {
            self.debug_buffer.set_text(
                &self.debug_text,
                &Attrs::new().family(glyphon::Family::SansSerif),
                Shaping::Basic,
                None,
            );

            self.debug_buffer
                .shape_until_scroll(&mut self.font_system, false);

            self.debug_text_changed = false;
        }

        let mut world_areas = Vec::with_capacity(256);
        let mut text_areas = Vec::with_capacity(256);

        if mode == 0 {
            let text_border_thickness = 3.0;
            let text_border_color = to_glyphon(DARK_THEME.fill_border);

            let aspect_ratio = if screen_h > 0.0 {
                screen_w / screen_h
            } else {
                1.0
            };

            for entity in entities.iter().filter(|_| world_text) {
                if let Some(text_comp) = entity.text {
                    let alpha = entity.instance.fill_color[3].clamp(0.0, 1.0);

                    if alpha <= 0.02 {
                        continue;
                    }

                    let (sx, sy) = Renderer::world_to_screen(
                        entity.instance.position,
                        camera_pos,
                        zoom,
                        aspect_ratio,
                        screen_w,
                        screen_h,
                    );

                    let body_r_px = entity.instance.size[0] * zoom * screen_h * 0.25;

                    let scale = Renderer::label_scale(body_r_px);

                    let base_left = sx + text_comp.offset[0] * scale;

                    let base_top = sy - body_r_px - NAME_LABEL_GAP - NAME_LABEL_LINE * scale;

                    let bounds = TextBounds {
                        left: 0,
                        top: 0,
                        right: self.config.width as i32,
                        bottom: self.config.height as i32,
                    };

                    let outline = to_glyphon(with_alpha(DARK_THEME.fill_border, alpha));

                    let fill = Color::rgba(
                        text_comp.color.r(),
                        text_comp.color.g(),
                        text_comp.color.b(),
                        (alpha * 255.0).round() as u8,
                    );

                    let thickness = (text_border_thickness * scale).max(2.0);

                    for (dx, dy) in OUTLINE_DIRS {
                        world_areas.push(TextArea {
                            buffer: &text_comp.buffer,
                            left: base_left + dx * thickness,
                            top: base_top + dy * thickness,
                            scale,
                            bounds,
                            default_color: outline,
                            custom_glyphs: &[],
                        });
                    }

                    world_areas.push(TextArea {
                        buffer: &text_comp.buffer,
                        left: base_left,
                        top: base_top,
                        scale,
                        bounds,
                        default_color: fill,
                        custom_glyphs: &[],
                    });
                }
            }

            for (bi, left, top, w, h, alpha) in bubble_rects.iter() {
                let Some(bubble) = self.chat.bubble(*bi) else {
                    continue;
                };

                let (_, text_h) = Renderer::text_dimensions(&bubble.body);

                if !text_h.is_finite() || text_h <= 0.0 {
                    continue;
                }

                let text_top = *top + ((*h - text_h).max(0.0) * 0.5);

                let bounds = TextBounds {
                    left: (*left + 6.0).round() as i32,
                    top: (*top + 4.0).round() as i32,
                    right: (*left + *w - 6.0).round() as i32,
                    bottom: (*top + *h - 4.0).round() as i32,
                };

                let alpha_u8 = ((*alpha).clamp(0.0, 1.0) * 255.0).round() as u8;

                let fill = Color::rgba(255, 255, 255, alpha_u8);

                let outline = to_glyphon(with_alpha(DARK_THEME.fill_border, *alpha));

                for (dx, dy) in OUTLINE_DIRS {
                    world_areas.push(TextArea {
                        buffer: &bubble.body,
                        left: *left + 8.0 + dx * 1.5,
                        top: text_top + dy * 1.5,
                        scale: 1.0,
                        bounds,
                        default_color: outline,
                        custom_glyphs: &[],
                    });
                }

                world_areas.push(TextArea {
                    buffer: &bubble.body,
                    left: *left + 8.0,
                    top: text_top,
                    scale: 1.0,
                    bounds,
                    default_color: fill,
                    custom_glyphs: &[],
                });
            }

            let bounds = TextBounds {
                left: 0,
                top: 0,
                right: self.config.width as i32,
                bottom: self.config.height as i32,
            };

            let white = Color::rgb(255, 255, 255);
            let hud = Renderer::hud_layout(screen);

            if show_hud {
                for notice in self.notices.entries.iter() {
                    let alpha = notice.alpha();

                    push_centered_text(
                        &mut text_areas,
                        &notice.buffer,
                        hud.cx,
                        notice.y,
                        SCORE_TEXT_OUTLINE_PX,
                        to_glyphon(with_alpha(DARK_THEME.fill_border, alpha)),
                        Color::rgba(255, 255, 255, (alpha * 255.0) as u8),
                        bounds,
                    );
                }

                if !self.name_text.is_empty() {
                    push_centered_text(
                        &mut text_areas,
                        &self.name_bar_buffer,
                        hud.cx,
                        hud.name_cy,
                        3.0,
                        text_border_color,
                        white,
                        bounds,
                    );
                }

                push_centered_text(
                    &mut text_areas,
                    &self.score_bar_buffer,
                    hud.cx,
                    hud.score_cy,
                    SCORE_TEXT_OUTLINE_PX,
                    text_border_color,
                    white,
                    bounds,
                );

                push_centered_text(
                    &mut text_areas,
                    &self.level_bar_buffer,
                    hud.cx,
                    hud.level_cy,
                    SCORE_TEXT_OUTLINE_PX,
                    text_border_color,
                    white,
                    bounds,
                );

                let net_right = screen_w - minimap::MARGIN;
                let mut net_bottom = screen_h - minimap::MARGIN - minimap::SIZE - 6.0;

                for buffer in self.net_buffers.iter().rev() {
                    let (w, h) = Renderer::text_dimensions(buffer);

                    net_bottom -= h;

                    for (dx, dy) in OUTLINE_DIRS {
                        text_areas.push(TextArea {
                            buffer,
                            left: net_right - w + dx * 1.5,
                            top: net_bottom + dy * 1.5,
                            scale: 1.0,
                            bounds,
                            default_color: text_border_color,
                            custom_glyphs: &[],
                        });
                    }

                    text_areas.push(TextArea {
                        buffer,
                        left: net_right - w,
                        top: net_bottom,
                        scale: 1.0,
                        bounds,
                        default_color: white,
                        custom_glyphs: &[],
                    });
                }
            }

            if self.show_debug {
                text_areas.push(TextArea {
                    buffer: &self.text_buffer,
                    left: 20.0,
                    top: 20.0,
                    scale: 1.0,
                    bounds,
                    default_color: white,
                    custom_glyphs: &[],
                });

                let (w, _) = Renderer::text_dimensions(&self.debug_buffer);

                let left = screen_w - 20.0 - w;
                let top = screen_h / 2.0;

                for (dx, dy) in OUTLINE_DIRS {
                    text_areas.push(TextArea {
                        buffer: &self.debug_buffer,
                        left: left + dx,
                        top: top + dy,
                        scale: 1.0,
                        bounds,
                        default_color: text_border_color,
                        custom_glyphs: &[],
                    });
                }

                text_areas.push(TextArea {
                    buffer: &self.debug_buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds,
                    default_color: white,
                    custom_glyphs: &[],
                });
            }

            text_areas.extend(scoreboard_areas);
            text_areas.extend(panel_areas);
            text_areas.extend(chat_areas);
            text_areas.extend(class_areas);
            text_areas.extend(death_areas);

            let _ = self.world_text_renderer.prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                world_areas,
                &mut self.swash_cache,
            );

            let _ = self.text_renderer.prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            );
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Entities + Text Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Entities + Text Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: DARK_THEME.background[0] as f64,
                            g: DARK_THEME.background[1] as f64,
                            b: DARK_THEME.background[2] as f64,
                            a: DARK_THEME.background[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            let world = self.world_instances.min(self.num_instances);

            if mode < 2 {
                render_pass.set_pipeline(&self.render_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                render_pass.draw(0..6, 0..world);
            }

            if mode == 0 {
                self.world_text_renderer
                    .render(&self.atlas, &self.viewport, &mut render_pass)
                    .unwrap();
            }

            if mode < 2 && world < self.num_instances {
                render_pass.set_pipeline(&self.render_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                render_pass.draw(0..6, world..self.num_instances);
            }

            if mode == 0 {
                self.text_renderer
                    .render(&self.atlas, &self.viewport, &mut render_pass)
                    .unwrap();
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        self.queue.present(output);

        if mode == 0 {
            self.atlas.trim();
        }
    }
}

pub struct Renderer {
    proxy: Option<EventLoopProxy<RenderState>>,
    state: Option<RenderState>,
    game_state: Rc<RefCell<GameState>>,
    labels: HashMap<u32, TextComponent>,

    browser_input_handlers: Vec<Closure<dyn FnMut(web_sys::Event)>>,
    browser_input_hooks_installed: bool,
}

impl Renderer {
    pub fn new(event_loop: &EventLoop<RenderState>, game_state: Rc<RefCell<GameState>>) -> Self {
        Self {
            proxy: Some(event_loop.create_proxy()),
            state: None,
            game_state,
            labels: HashMap::new(),
            browser_input_handlers: Vec::new(),
            browser_input_hooks_installed: false,
        }
    }

    fn install_browser_input_hooks(&mut self, window: Arc<Window>) {
        if self.browser_input_hooks_installed {
            return;
        }

        let Some(browser_window) = web_sys::window() else {
            return;
        };

        let Some(document) = browser_window.document() else {
            return;
        };

        let releases: [(&web_sys::EventTarget, &str); 5] = [
            (&document, "pointerup"),
            (&document, "pointercancel"),
            (&document, "mouseup"),
            (&browser_window, "blur"),
            (&document, "visibilitychange"),
        ];

        for (target, name) in releases {
            let game_state = Rc::clone(&self.game_state);
            let redraw_window = Arc::clone(&window);

            let closure =
                Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                    let mut game = game_state.borrow_mut();

                    let buttons = event
                        .dyn_ref::<web_sys::MouseEvent>()
                        .map(|e| e.buttons())
                        .unwrap_or(0);

                    if buttons & 1 == 0 {
                        game.fire_held = false;
                        game.auto_fire = false;
                    }

                    if buttons & 2 == 0 {
                        game.repel_held = false;
                        game.repel = false;
                    }

                    if event.dyn_ref::<web_sys::MouseEvent>().is_none() {
                        game.shift_held = false;
                    }

                    redraw_window.request_redraw();
                });

            let _ = target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref());

            self.browser_input_handlers.push(closure);
        }

        if let Some(canvas) = document.get_element_by_id("gameCanvas") {
            let closure = Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| {
                event.prevent_default();
            });

            let _ = canvas
                .add_event_listener_with_callback("contextmenu", closure.as_ref().unchecked_ref());

            self.browser_input_handlers.push(closure);
        }

        self.browser_input_hooks_installed = true;
    }

    pub fn world_to_screen(
        world_pos: [f32; 2],
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
        screen_width: f32,
        screen_height: f32,
    ) -> (f32, f32) {
        if !zoom.is_finite()
            || zoom <= 0.0
            || !aspect_ratio.is_finite()
            || aspect_ratio <= 0.0
            || !screen_width.is_finite()
            || !screen_height.is_finite()
            || screen_width <= 0.0
            || screen_height <= 0.0
        {
            return (screen_width * 0.5, screen_height * 0.5);
        }

        let ndc_x = ((world_pos[0] - camera_pos[0]) * zoom) / aspect_ratio;

        let ndc_y = (world_pos[1] - camera_pos[1]) * zoom;

        let screen_x = (ndc_x + 1.0) * (screen_width / 2.0);

        let screen_y = (1.0 - ndc_y) * (screen_height / 2.0);

        (screen_x, screen_y)
    }

    pub fn screen_to_world(
        screen_pos: [f32; 2],
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
        screen_width: f32,
        screen_height: f32,
    ) -> [f32; 2] {
        if !zoom.is_finite()
            || zoom <= 0.0
            || !aspect_ratio.is_finite()
            || aspect_ratio <= 0.0
            || screen_width <= 0.0
            || screen_height <= 0.0
        {
            return camera_pos;
        }

        let ndc_x = (screen_pos[0] / (screen_width / 2.0)) - 1.0;

        let ndc_y = 1.0 - (screen_pos[1] / (screen_height / 2.0));

        let rel_x = ndc_x * aspect_ratio / zoom;

        let rel_y = ndc_y / zoom;

        [camera_pos[0] + rel_x, camera_pos[1] + rel_y]
    }

    pub fn cursor_to_world(
        cursor: [f32; 2],
        window_width: f32,
        window_height: f32,
        surface_width: f32,
        surface_height: f32,
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
    ) -> [f32; 2] {
        if window_width <= 0.0
            || window_height <= 0.0
            || surface_width <= 0.0
            || surface_height <= 0.0
            || !window_width.is_finite()
            || !window_height.is_finite()
            || !surface_width.is_finite()
            || !surface_height.is_finite()
            || !zoom.is_finite()
            || zoom <= 0.0
            || !aspect_ratio.is_finite()
            || aspect_ratio <= 0.0
        {
            return camera_pos;
        }

        let surface_cursor = [
            cursor[0] * surface_width / window_width,
            cursor[1] * surface_height / window_height,
        ];

        Self::screen_to_world(
            surface_cursor,
            camera_pos,
            zoom,
            aspect_ratio,
            surface_width,
            surface_height,
        )
    }

    fn text_dimensions(buffer: &glyphon::Buffer) -> (f32, f32) {
        let mut width = 0.0f32;
        let mut height = 0.0f32;

        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            height += run.line_height;
        }

        (width, height)
    }

    fn label_scale(body_r_px: f32) -> f32 {
        (body_r_px * 0.6 / NAME_LABEL_FONT).clamp(0.45, 1.6)
    }

    fn hud_layout(screen: Vec2) -> HudLayout {
        let level_w = (screen.x * LEVEL_BAR_W_FRAC).min(LEVEL_BAR_MAX_W).max(1.0);
        let score_w = level_w * SCORE_BAR_W_RATIO;

        let level_cy = screen.y - LEVEL_BAR_BOTTOM - LEVEL_BAR_H * 0.5;
        let score_cy = level_cy - LEVEL_BAR_H * 0.5 - LEVEL_BAR_GAP - SCORE_BAR_H * 0.5;
        let name_cy = score_cy - SCORE_BAR_H * 0.5 - 28.0;

        HudLayout {
            cx: screen.x * 0.5,
            level_w,
            level_cy,
            score_w,
            score_cy,
            name_cy,
        }
    }

    fn view_bounds(
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
        margin: f32,
    ) -> (Vec2, Vec2) {
        let safe_zoom = if zoom.is_finite() && zoom > 0.0 {
            zoom
        } else {
            PLAYER_ZOOM
        };

        let safe_aspect = if aspect_ratio.is_finite() && aspect_ratio > 0.0 {
            aspect_ratio
        } else {
            1.0
        };

        let safe_margin = if margin.is_finite() {
            margin.max(0.0)
        } else {
            0.0
        };

        let half_w = safe_aspect / safe_zoom + safe_margin;

        let half_h = 1.0 / safe_zoom + safe_margin;

        let c = Vec2::new(camera_pos[0], camera_pos[1]);

        (c - Vec2::new(half_w, half_h), c + Vec2::new(half_w, half_h))
    }

    #[inline]
    fn is_visible(pos: Vec2, radius: f32, min: Vec2, max: Vec2) -> bool {
        let radius = radius.max(0.0);

        pos.x + radius >= min.x
            && pos.x - radius <= max.x
            && pos.y + radius >= min.y
            && pos.y - radius <= max.y
    }

    fn begin_frame(state: &mut RenderState) -> (f64, f32) {
        let now = window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);

        let dt = match state.last_frame_time {
            Some(last) if now.is_finite() && last.is_finite() && now >= last => {
                (((now - last) / 1000.0).clamp(0.0, 0.1)) as f32
            }

            _ => 1.0 / 60.0,
        };

        state.last_frame_time = Some(now);

        state.debug_frame_ms = state.debug_frame_ms * 0.9 + dt * 1000.0 * 0.1;

        state.debug_frames += 1;

        if state.debug_last == 0.0 {
            state.debug_last = now;
        } else if now - state.debug_last >= 500.0 {
            let elapsed = ((now - state.debug_last) / 1000.0).max(1e-3);

            state.debug_fps = state.debug_frames as f64 / elapsed;

            state.debug_frames = 0;
            state.debug_last = now;
            state.debug_dirty = true;
        }

        (now, dt)
    }

    fn log_stats(_state: &mut RenderState, _game: &GameState, _now: f64) {}

    fn advance_entities(state: &mut RenderState, game: &mut GameState, now: f64, dt: f32) {
        game.tick_render(dt);

        for b in game.bullets.iter_mut() {
            if b.dead {
                if now - b.last_update_time >= 100.0 {
                    b.dying = true;
                }
            } else if now - b.last_update_time > BULLET_STALE_FADE_MS {
                b.dying = true;

                b.render_alpha = (b.render_alpha - dt * 3.0).max(0.0);
            }
        }

        game.bullets
            .retain(|b| b.render_alpha > 0.0 && (now - b.last_update_time) < BULLET_STALE_CULL_MS);

        for b in game.drones.iter_mut() {
            if now - b.last_update_time > BULLET_STALE_FADE_MS {
                b.dying = true;

                b.render_alpha = (b.render_alpha - dt * 3.0).max(0.0);
            }
        }

        game.drones
            .retain(|b| b.render_alpha > 0.0 && (now - b.last_update_time) < BULLET_STALE_CULL_MS);

        // (owner conn id << 8) | (barrel index + 1)
        for b in game.bullets.iter_mut() {
            if !b.is_new {
                continue;
            }

            b.is_new = false;

            if b.recoil == 0 {
                continue;
            }

            let owner_id = b.recoil >> 8;
            let barrel_raw = b.recoil & 0xff;

            if barrel_raw == 0 {
                continue;
            }

            let barrel_idx = (barrel_raw - 1) as usize;

            if let Some(p) = game.players.iter_mut().find(|p| p.id == owner_id) {
                p.kick_barrel(barrel_idx);
            }
        }

        const SHAPE_STALE_MS: f64 = 3000.0;
        const SHAPE_FADE_OUT: f32 = 0.30;

        for s in game.shapes.iter_mut() {
            if !s.dying && now - s.last_update_time > SHAPE_STALE_MS {
                s.dying = true;
            }

            if s.dying {
                s.render_alpha = (s.render_alpha - dt / SHAPE_FADE_OUT).max(0.0);
            }
        }

        game.shapes.retain(|s| s.render_alpha > 0.0 || !s.dying);

        let my_player_id = game.my_player_id;
        let spinning = game.auto_spin;

        for p in game.players.iter_mut() {
            if !p.dying
                && Some(p.id) != my_player_id
                && p.last_update_time > 0.0
                && now - p.last_update_time > PLAYER_STALE_MS
            {
                p.dying = true;
            }
        }

        game.players.retain(|p| !p.dying || p.render_alpha > 0.0);

        for p in game.players.iter_mut() {
            let alpha = ((now - p.last_update_time) / 100.0).clamp(0.0, 1.0) as f32;

            p.render_pos = p.last_pos.lerp(p.pos, alpha);

            let health_lerp = 1.0 - (-12.0 * dt).exp();

            p.render_health += (p.health as f32 - p.render_health) * health_lerp;

            let bar_target = if p.render_health < p.max_health as f32 - 0.5 {
                1.0
            } else {
                0.0
            };

            p.health_bar_alpha +=
                (bar_target - p.health_bar_alpha) * (1.0 - (-HEALTH_BAR_FADE_SPEED * dt).exp());

            if Some(p.id) != my_player_id || spinning {
                let mut diff = p.rot - p.last_rot;

                if diff > std::f32::consts::PI {
                    diff -= std::f32::consts::TAU;
                }

                if diff < -std::f32::consts::PI {
                    diff += std::f32::consts::TAU;
                }

                p.render_rot = p.last_rot + diff * alpha;
            }

            p.render_turret_aims.clear();
            for (i, &aim) in p.turret_aims.iter().enumerate() {
                let last = p.last_turret_aims.get(i).copied().unwrap_or(aim);
                let diff = (aim - last + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;

                p.render_turret_aims.push(last + diff * alpha);
            }
        }

        for s in game.shapes.iter_mut() {
            let alpha = ((now - s.last_update_time) / 100.0).clamp(0.0, 1.0) as f32;

            s.render_pos = s.last_pos.lerp(s.pos, alpha);

            let health_lerp = 1.0 - (-12.0 * dt).exp();

            s.render_health += (s.health as f32 - s.render_health) * health_lerp;

            let bar_target = if s.render_health < s.max_health as f32 - 0.5 {
                1.0
            } else {
                0.0
            };

            s.health_bar_alpha +=
                (bar_target - s.health_bar_alpha) * (1.0 - (-HEALTH_BAR_FADE_SPEED * dt).exp());

            let mut diff = s.rot - s.last_rot;

            if diff > std::f32::consts::PI {
                diff -= std::f32::consts::TAU;
            }

            if diff < -std::f32::consts::PI {
                diff += std::f32::consts::TAU;
            }

            s.render_rot = s.last_rot + diff * alpha;
        }

        for b in game.bullets.iter_mut() {
            let alpha = ((now - b.last_update_time) / 100.0).clamp(0.0, 1.0) as f32;

            b.render_pos = b.last_pos.lerp(b.pos, alpha);

            if b.dying && !b.dead {
                let vel = (b.pos - b.last_pos) / 0.1;

                b.render_pos += vel * dt * b.render_alpha;
            }

            let mut diff = b.rot - b.last_rot;

            if diff > std::f32::consts::PI {
                diff -= std::f32::consts::TAU;
            }

            if diff < -std::f32::consts::PI {
                diff += std::f32::consts::TAU;
            }

            b.render_rot = if b.trap {
                (now / 1000.0 * TRAP_SPIN) as f32 + b.id as f32
            } else {
                b.last_rot + diff * alpha
            };
        }

        for b in game.drones.iter_mut() {
            let alpha = ((now - b.last_update_time) / 100.0).clamp(0.0, 1.0) as f32;

            b.render_pos = b.last_pos.lerp(b.pos, alpha);

            if b.dying {
                let vel = (b.pos - b.last_pos) / 0.1;

                b.render_pos += vel * dt * b.render_alpha;
            }

            let diff = (b.rot - b.last_rot + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;

            b.render_rot = b.last_rot + diff * alpha;
        }

        if let Some(p) = game.my_player_mut() {
            let smoothed = state.player_display.update(p.render_pos, dt);

            p.render_pos = smoothed;
        }
    }

    fn update_camera_and_aim(state: &mut RenderState, game: &mut GameState, dt: f32) {
        let has_player = game.my_player().is_some();

        let target_zoom = if has_player {
            camera_zoom(game.level, game.tank_info.field_factor)
        } else {
            PLAYER_ZOOM * SPECTATE_ZOOM_MULT
        };

        if has_player && !state.had_player {
            if let Some(p) = game.my_player() {
                state.player_display.snap_to(p.render_pos);

                state.camera.snap_to(p.render_pos);
            }

            state.camera.snap_zoom(target_zoom);
        }

        state.had_player = has_player;

        state.camera.update_zoom(target_zoom, dt);

        if let Some(p) = game.my_player() {
            if state.camera_enabled {
                state.camera.update(p.render_pos, dt);
            } else {
                state.camera.pos = p.render_pos;
            }

            state.spectate_follow.snap_to(p.render_pos);
        } else if let Some(target) = game.spectate {
            let pos = state.spectate_follow.update(target, dt);

            state.camera.snap_to(pos);
        }

        if let Some(cursor) = state.cursor_pos {
            let win = state.window.inner_size();

            let win_w = win.width as f32;

            let win_h = win.height as f32;

            let surface_w = state.config.width as f32;

            let surface_h = state.config.height.max(1) as f32;

            if win_w > 0.0 && win_h > 0.0 {
                let aspect_ratio = surface_w / surface_h;

                let cursor_world = Self::cursor_to_world(
                    [cursor.x, cursor.y],
                    win_w,
                    win_h,
                    surface_w,
                    surface_h,
                    [state.camera.pos.x, state.camera.pos.y],
                    state.camera.zoom,
                    aspect_ratio,
                );

                game.mouse_screen = Some([
                    cursor.x.clamp(0.0, win_w) as u16,
                    cursor.y.clamp(0.0, win_h) as u16,
                    win_w.min(u16::MAX as f32) as u16,
                    win_h.min(u16::MAX as f32) as u16,
                ]);

                if let Some(p) = game.my_player() {
                    let delta = Vec2::new(
                        cursor_world[0] - p.render_pos.x,
                        cursor_world[1] - p.render_pos.y,
                    );

                    if delta.length_squared() > 0.000001 {
                        game.mouse_angle = Some(delta.y.atan2(delta.x));
                    }
                }
            }
        } else {
            game.mouse_angle = None;
            game.mouse_screen = None;
        }

        if let Some(target) = game.mouse_angle.filter(|_| !game.auto_spin) {
            if let Some(p) = game.my_player_mut() {
                p.render_rot = target;
            }
        }
    }

    fn update_scoreboard(state: &mut RenderState, game: &GameState, dt: f32) {
        state
            .scoreboard
            .sync(&mut state.font_system, &game.leaderboard);

        state.scoreboard.tick(&mut state.font_system, dt);
    }

    fn update_labels(
        state: &mut RenderState,
        labels: &mut HashMap<u32, TextComponent>,
        game: &GameState,
    ) {
        for p in game.players.iter() {
            match labels.get_mut(&p.id) {
                Some(tc) if tc.source_text == p.name => {}

                Some(tc) => {
                    tc.update_text(&mut state.font_system, &p.name);
                }

                None => {
                    let mut tc = TextComponent::new(&mut state.font_system, &p.name);

                    tc.center_horizontally();

                    labels.insert(p.id, tc);
                }
            }
        }

        labels.retain(|id, _| game.players.iter().any(|p| p.id == *id));
    }

    fn update_chat(state: &mut RenderState, game: &mut GameState, now: f64, dt: f32) {
        let incoming = std::mem::take(&mut game.incoming_chat);

        for msg in incoming {
            let text = msg.text.trim();

            if text.is_empty() {
                continue;
            }

            let sender = msg.sender.trim();

            state.chat.receive(
                &mut state.font_system,
                msg.channel,
                msg.team,
                msg.sender_id,
                sender,
                text,
                msg.timestamp,
            );
        }

        game.chat_channel = state.chat.active_channel();

        state.chat.tick(now, dt);
    }

    fn build_world_instances<'a>(
        game: &GameState,
        labels: &'a HashMap<u32, TextComponent>,
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
    ) -> Vec<RenderEntity<'a>> {
        const OUTSIDE_SIZE: f32 = 30000.0;
        const ZONE_ALPHA: f32 = 0.16;
        const WALL_BORDER: f32 = 4.0;

        let map_bound = game.arena.bound;

        let outside = DARK_THEME.map_outside;

        let mut instances: Vec<RenderEntity<'a>> = Vec::with_capacity(
            64 + game.bullets.len() * 2 + game.shapes.len() * 3 + game.players.len() * 8,
        );

        instances.push(RenderEntity {
            instance: EntityInstance {
                position: [0.0, 0.0],
                size: [OUTSIDE_SIZE, OUTSIDE_SIZE],
                rotation: 0.0,
                shape_type: 1,
                sides: 4,
                fill_color: outside,
                border_color: outside,
                border_thickness: 0.0,
                extra_param: 1.0,
            },
            text: None,
        });

        instances.push(RenderEntity {
            instance: EntityInstance {
                position: [0.0, 0.0],
                size: [map_bound * 2.0, map_bound * 2.0],
                rotation: 0.0,
                shape_type: 2,
                sides: 0,
                fill_color: DARK_THEME.background,
                border_color: DARK_THEME.grid,
                border_thickness: 2.0,
                extra_param: 64.0,
            },
            text: None,
        });

        let (cull_min, cull_max) = Self::view_bounds(camera_pos, zoom, aspect_ratio, CULL_MARGIN);

        for z in game.arena.zones.iter() {
            let center = Vec2::new(z.x, z.y);

            if !Self::is_visible(center, z.w.max(z.h) * 0.5, cull_min, cull_max) {
                continue;
            }

            let tint = if z.team == NO_TEAM {
                DARK_THEME.fallen_boss
            } else {
                team_color(z.team)
            };

            instances.push(RenderEntity {
                instance: EntityInstance {
                    position: [z.x, z.y],
                    size: [z.w, z.h],
                    rotation: 0.0,
                    shape_type: 1,
                    sides: 4,
                    fill_color: with_alpha(tint, ZONE_ALPHA),
                    border_color: with_alpha(tint, ZONE_ALPHA),
                    border_thickness: 0.0,
                    extra_param: 1.0,
                },
                text: None,
            });
        }

        for b in game.bullets.iter() {
            if !Self::is_visible(b.render_pos, BULLET_CULL_RADIUS, cull_min, cull_max) {
                continue;
            }

            for inst in b.get_render_instances() {
                instances.push(RenderEntity {
                    instance: inst,
                    text: None,
                });
            }
        }

        for d in game.drones.iter() {
            if !Self::is_visible(d.render_pos, BULLET_CULL_RADIUS, cull_min, cull_max) {
                continue;
            }

            for inst in d.get_render_instances() {
                instances.push(RenderEntity {
                    instance: inst,
                    text: None,
                });
            }
        }

        for s in game.shapes.iter() {
            if !Self::is_visible(s.render_pos, s.size, cull_min, cull_max) {
                continue;
            }

            for inst in s.get_render_instances() {
                instances.push(RenderEntity {
                    instance: inst,
                    text: None,
                });
            }

            if s.dying {
                continue;
            }

            let bar_alpha = s.health_bar_alpha;

            if bar_alpha > 0.01 {
                let bar_w = s.size * 0.7;

                let bar_h = 7.0;

                let bar_y = s.render_pos.y - s.size * 0.5;

                let inset = 1.5;

                let fg_h = (bar_h - inset * 2.0);

                let inner_w = (bar_w - inset * 2.0).max(0.0);

                let max_health = s.max_health.max(1) as f32;

                let health_percent = (s.render_health / max_health).clamp(0.0, 1.0);

                let fg_w = inner_w * health_percent;

                let inner_left = s.render_pos.x - bar_w * 0.5 + inset;

                instances.push(RenderEntity {
                    instance: EntityInstance {
                        position: [s.render_pos.x, bar_y],
                        size: [bar_w.max(0.0), bar_h],
                        rotation: 0.0,
                        shape_type: 4,
                        sides: 4,
                        fill_color: with_alpha(DARK_THEME.health_bar_background, bar_alpha),
                        border_color: with_alpha(
                            DARK_THEME.outline_for(DARK_THEME.health_bar_background),
                            bar_alpha,
                        ),
                        border_thickness: 0.0,
                        extra_param: 1.0,
                    },
                    text: None,
                });

                if fg_w > 0.1 && fg_h > 0.0 {
                    instances.push(RenderEntity {
                        instance: EntityInstance {
                            position: [inner_left + fg_w * 0.5, bar_y],
                            size: [fg_w, fg_h],
                            rotation: 0.0,
                            shape_type: 4,
                            sides: 4,
                            fill_color: with_alpha(DARK_THEME.health_bar_foreground, bar_alpha),
                            border_color: with_alpha(
                                DARK_THEME.outline_for(DARK_THEME.health_bar_foreground),
                                bar_alpha,
                            ),
                            border_thickness: HEALTH_BAR_BORDER,
                            extra_param: 1.0,
                        },
                        text: None,
                    });
                }
            }
        }

        for w in game.arena.walls.iter() {
            let center = Vec2::new(w.x, w.y);

            if !Self::is_visible(center, w.w.max(w.h) * 0.5, cull_min, cull_max) {
                continue;
            }

            instances.push(RenderEntity {
                instance: EntityInstance {
                    position: [w.x, w.y],
                    size: [w.w, w.h],
                    rotation: 0.0,
                    shape_type: 1,
                    sides: 4,
                    fill_color: DARK_THEME.maze_walls,
                    border_color: DARK_THEME.outline_for(DARK_THEME.maze_walls),
                    border_thickness: WALL_BORDER,
                    extra_param: 1.0,
                },
                text: None,
            });
        }

        for p in game.players.iter() {
            let label = labels.get(&p.id);

            let (tank_instances, body_idx) = p.render_with_body();

            for (i, inst) in tank_instances.into_iter().enumerate() {
                instances.push(RenderEntity {
                    instance: inst,
                    text: if i == body_idx { label } else { None },
                });
            }
        }

        for p in game.players.iter() {
            if p.dying {
                continue;
            }

            let bar_alpha = p.health_bar_alpha;

            if bar_alpha > 0.01 {
                let scale = p.scale.max(0.001);

                let bar_w = 44.0 * scale;

                let bar_h = 10.0 * scale;

                let bar_y = p.render_pos.y - 32.0 * scale;

                let inset = HEALTH_BAR_INSET * scale;

                let fg_h = (bar_h - inset * 2.0).max(0.0);

                let inner_w = (bar_w - inset * 2.0).max(0.0);

                let max_health = p.max_health.max(1) as f32;

                let health_percent = (p.render_health / max_health).clamp(0.0, 1.0);

                let fg_w = inner_w * health_percent;

                let inner_left = p.render_pos.x - bar_w * 0.5 + inset;

                instances.push(RenderEntity {
                    instance: EntityInstance {
                        position: [p.render_pos.x, bar_y],
                        size: [bar_w, bar_h],
                        rotation: 0.0,
                        shape_type: 4,
                        sides: 4,
                        fill_color: with_alpha(DARK_THEME.health_bar_background, bar_alpha),
                        border_color: with_alpha(
                            DARK_THEME.outline_for(DARK_THEME.health_bar_background),
                            bar_alpha,
                        ),
                        border_thickness: 0.0,
                        extra_param: 1.0,
                    },
                    text: None,
                });

                if fg_w > 0.1 && fg_h > 0.0 {
                    instances.push(RenderEntity {
                        instance: EntityInstance {
                            position: [inner_left + fg_w * 0.5, bar_y],
                            size: [fg_w, fg_h],
                            rotation: 0.0,
                            shape_type: 4,
                            sides: 4,
                            fill_color: with_alpha(DARK_THEME.health_bar_foreground, bar_alpha),
                            border_color: with_alpha(
                                DARK_THEME.outline_for(DARK_THEME.health_bar_foreground),
                                bar_alpha,
                            ),
                            border_thickness: HEALTH_BAR_BORDER,
                            extra_param: 1.0,
                        },
                        text: None,
                    });
                }
            }
        }

        instances
    }

    fn my_score(game: &GameState) -> u32 {
        let my_name = game.my_player().map(|p| p.name.as_str());

        game.leaderboard
            .iter()
            .find(|(n, _)| Some(n.as_str()) == my_name)
            .map(|e| e.1)
            .unwrap_or(game.score)
            .max(game.score)
    }

    fn build_hud_instances<'a>(
        state: &mut RenderState,
        game: &GameState,
        screen: Vec2,
        dt: f32,
        instances: &mut Vec<RenderEntity<'a>>,
    ) {
        let hud = Self::hud_layout(screen);

        let level_target = if game.xp_to_next > 0 {
            (game.xp as f32 / game.xp_to_next as f32).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let score = Self::my_score(game);

        let top = game
            .leaderboard
            .iter()
            .map(|e| e.1)
            .max()
            .unwrap_or(0)
            .max(score);

        let score_target = if top > 0 {
            (score as f32 / top as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let t = 1.0 - (-HUD_BAR_SPEED * dt).exp();

        state.level_fill += (level_target - state.level_fill) * t;

        state.score_fill += (score_target - state.score_fill) * t;

        let bars = [
            (
                hud.score_w,
                SCORE_BAR_H,
                hud.score_cy,
                state.score_fill,
                DARK_THEME.score_bar_fill,
            ),
            (
                hud.level_w,
                LEVEL_BAR_H,
                hud.level_cy,
                state.level_fill,
                DARK_THEME.xp_bar_fill,
            ),
        ];

        for (w, h, cy, fill, color) in bars {
            let left = hud.cx - w * 0.5 + HUD_BAR_OUTLINE;

            instances.push(RenderEntity {
                instance: pill_instance(
                    Vec2::new(hud.cx, cy),
                    Vec2::new(w, h),
                    screen,
                    HUD_BAR_TRACK,
                    HUD_BAR_EDGE,
                    HUD_BAR_OUTLINE,
                ),
                text: None,
            });

            let inner_w = w - HUD_BAR_OUTLINE * 2.0;
            let inner_h = h - HUD_BAR_OUTLINE * 2.0;
            let fg_w = inner_w * fill;

            if fg_w > 1.0 {
                let fg_w = fg_w.max(inner_h);

                instances.push(RenderEntity {
                    instance: pill_instance(
                        Vec2::new(left + fg_w * 0.5, cy),
                        Vec2::new(fg_w, inner_h),
                        screen,
                        color,
                        color,
                        0.0,
                    ),
                    text: None,
                });
            }
        }

        instances.extend(
            minimap::render_data(game, screen)
                .into_iter()
                .map(|instance| RenderEntity {
                    instance,
                    text: None,
                }),
        );
    }

    fn fill_hud_text(state: &mut RenderState, game: &GameState) {
        use std::fmt::Write;

        state.stats_text.clear();

        let _ = write!(
            state.stats_text,
            "Lvl {} | XP: {}/{} | HP: {}/{}",
            game.level, game.xp, game.xp_to_next, game.health, game.max_health
        );

        state.score_text.clear();

        let _ = write!(state.score_text, "Score: {}", Self::my_score(game));

        state.level_text.clear();

        let _ = write!(
            state.level_text,
            "Lvl {} {}",
            game.level, game.tank_info.name
        );

        state.name_text.clear();

        if let Some(p) = game.my_player() {
            state.name_text.push_str(&p.name);
        }

        for line in state.net_text.iter_mut() {
            line.clear();
        }

        match game.ping_ms {
            Some(ping) => {
                let _ = write!(state.net_text[0], "Ping {ping:.0} ms");
            }
            None => state.net_text[0].push_str("Ping -- ms"),
        }

        let _ = write!(state.net_text[0], "  |  Load {:.0}%", game.server_load);

        let _ = write!(
            state.net_text[1],
            "Players {}  |  Bots {}  |  Shapes {}",
            game.server_players, game.server_bots, game.server_shapes
        );
    }

    fn fill_debug_text(state: &mut RenderState, game: &GameState) {
        use std::fmt::Write;

        state.debug_text.clear();

        let _ = write!(
            state.debug_text,
            "fps: {:.0} ({:.1} ms)\nmode: {}\nzoom: {:.4}\nplayers: {}\nshapes: {}\nbullets: {}\ninstances: {}",
            state.debug_fps,
            state.debug_frame_ms,
            state.debug_render_mode,
            state.camera.zoom,
            game.players.len(),
            game.shapes.len(),
            game.bullets.len(),
            state.num_instances,
        );

        state.debug_text_changed = true;
    }
}

impl ApplicationHandler<RenderState> for Renderer {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let browser_window = wgpu::web_sys::window().unwrap_throw();

        let document = browser_window.document().unwrap_throw();

        let canvas: web_sys::HtmlCanvasElement = document
            .get_element_by_id("gameCanvas")
            .unwrap_throw()
            .unchecked_into();

        let dpr = browser_window.device_pixel_ratio().max(1.0);

        let client_width = canvas.client_width().max(1) as f64;

        let client_height = canvas.client_height().max(1) as f64;

        let physical_width = (client_width * dpr).round().max(1.0) as u32;

        let physical_height = (client_height * dpr).round().max(1.0) as u32;

        canvas.set_width(physical_width);

        canvas.set_height(physical_height);

        let window_attribs = Window::default_attributes().with_canvas(Some(canvas));

        let window = Arc::new(event_loop.create_window(window_attribs).unwrap());

        self.install_browser_input_hooks(Arc::clone(&window));

        if let Some(proxy) = self.proxy.take() {
            let game_state = Rc::clone(&self.game_state);

            spawn_local(async move {
                assert!(
                    proxy
                        .send_event(RenderState::new(window, game_state,).await,)
                        .is_ok()
                );
            });
        }
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        mut event: RenderState,
    ) {
        event.window.request_redraw();

        let size = event.window.inner_size();

        event.resize(size.width, size.height);

        self.state = Some(event);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let Renderer { state, labels, .. } = self;

        let state = match state.as_mut() {
            Some(s) => s,
            None => return,
        };

        match event {
            WindowEvent::CloseRequested => {
                state.cursor_pos = None;

                let mut game = state.game_state.borrow_mut();

                release_inputs(&mut game);

                game.mouse_angle = None;

                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
            }

            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(browser_window) = web_sys::window() {
                    if let Some(document) = browser_window.document() {
                        if let Some(element) = document.get_element_by_id("gameCanvas") {
                            let canvas: web_sys::HtmlCanvasElement = element.unchecked_into();

                            let dpr = browser_window.device_pixel_ratio().max(1.0);

                            let width = canvas.client_width().max(1) as f64;

                            let height = canvas.client_height().max(1) as f64;

                            let physical_width = (width * dpr).round().max(1.0) as u32;

                            let physical_height = (height * dpr).round().max(1.0) as u32;

                            canvas.set_width(physical_width);

                            canvas.set_height(physical_height);

                            state.resize(physical_width, physical_height);
                        }
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_pos = Some(Vec2::new(position.x as f32, position.y as f32));
            }

            WindowEvent::CursorLeft { .. } => {
                state.cursor_pos = None;

                let mut game = state.game_state.borrow_mut();

                release_inputs(&mut game);

                game.mouse_angle = None;
            }

            WindowEvent::Focused(false) => {
                state.cursor_pos = None;

                let mut game = state.game_state.borrow_mut();

                release_inputs(&mut game);

                game.shift_held = false;

                game.mouse_angle = None;
            }

            WindowEvent::MouseInput {
                state: mouse_state,
                button,
                ..
            } => {
                if button == MouseButton::Right {
                    let mut game = state.game_state.borrow_mut();

                    game.repel_held = mouse_state.is_pressed() && !state.chat.is_open();

                    return;
                }

                if button != MouseButton::Left {
                    return;
                }

                match mouse_state {
                    winit::event::ElementState::Pressed => {
                        let Some(cursor) = state.cursor_pos else {
                            state.game_state.borrow_mut().fire_held = false;

                            return;
                        };

                        if state.death.is_active() {
                            let win = state.window.inner_size();
                            let window_size = Vec2::new(win.width as f32, win.height as f32);
                            let screen = Vec2::new(
                                state.config.width as f32,
                                state.config.height.max(1) as f32,
                            );

                            if state.death.hit_continue(cursor, window_size, screen) {
                                continue_from_death(&mut state.death);
                            }

                            return;
                        }

                        let win = state.window.inner_size();

                        let window_size = Vec2::new(win.width as f32, win.height as f32);

                        let screen =
                            Vec2::new(state.config.width as f32, state.config.height.max(1) as f32);

                        let mut game = state.game_state.borrow_mut();

                        if state.chat.is_open() {
                            game.fire_held = false;

                            return;
                        }

                        if state.debug_render_mode == 0 {
                            if let Some(channel) =
                                state.chat.hit_test_tab(cursor, window_size, screen)
                            {
                                state.chat.set_channel(channel);

                                game.chat_channel = channel;

                                game.fire_held = false;

                                return;
                            }

                            if state.class_panel.hit_ignore(cursor, window_size, screen) {
                                state.class_panel.dismiss();

                                game.fire_held = false;

                                return;
                            }

                            if let Some(idx) =
                                state.class_panel.hit_test(cursor, window_size, screen)
                            {
                                game.class_choice = Some(idx as u8 + 1);

                                game.fire_held = false;

                                state.class_panel.set_pinned(false);

                                web_sys::console::log_1(
                                    &format!("class choice: {}", idx + 1).into(),
                                );

                                return;
                            }

                            if let Some(idx) =
                                state.upgrade_panel.hit_test(cursor, window_size, screen)
                            {
                                game.upgrade_request = Some(idx as u8 + 1);

                                game.fire_held = false;

                                web_sys::console::log_1(
                                    &format!("upgrade requested: [{}]", idx + 1).into(),
                                );

                                return;
                            }
                        }

                        game.fire_held = game.my_player().is_some();

                        game.fire_clicked = game.fire_held;
                    }

                    winit::event::ElementState::Released => {
                        let mut game = state.game_state.borrow_mut();

                        game.fire_held = false;
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                {
                    let mut game = state.game_state.borrow_mut();

                    let alive = game.my_player().is_some();

                    if alive {
                        keep_canvas_focused();
                    }

                    let can_act = alive && !state.chat.is_open();

                    game.auto_fire =
                        (game.fire_held && can_act) || (game.auto_fire_toggle && alive);

                    game.repel = (game.repel_held || game.shift_held) && can_act;
                }

                let (now, dt) = Self::begin_frame(state);

                state.notices.tick(dt);

                let screen =
                    Vec2::new(state.config.width as f32, state.config.height.max(1) as f32);

                let aspect_ratio = screen.x / screen.y;

                let game_rc = Rc::clone(&state.game_state);

                let mut game = game_rc.borrow_mut();

                Self::log_stats(state, &game, now);

                Self::advance_entities(state, &mut game, now, dt);

                Self::update_camera_and_aim(state, &mut game, dt);

                let camera_pos = [state.camera.pos.x, state.camera.pos.y];

                let zoom = state.camera.zoom;

                let win = state.window.inner_size();

                let window_size = Vec2::new(win.width as f32, win.height as f32);

                state.upgrade_panel.sync(
                    &mut state.font_system,
                    &game.tank_info.stat_names,
                    &game.tank_info.stat_maxes,
                    game.upgrade_points,
                );

                state.upgrade_panel.tick(
                    dt,
                    state.cursor_pos,
                    window_size,
                    screen,
                    game.upgrade_points,
                );

                state.class_panel.make_room(state.notices.bottom());

                state.class_panel.tick(
                    &mut state.font_system,
                    dt,
                    game.class_upgrades_available,
                    state.cursor_pos,
                    window_size,
                    screen,
                );

                if let Some(info) = game.death.take() {
                    state.death.show(&mut state.font_system, info);
                    game.died_at = None;
                }

                if game.my_player().is_some() {
                    state.death.hide();
                }

                if let Some(at) = game.died_at {
                    if now - at > DEATH_FALLBACK_MS && !state.death.is_active() {
                        game.died_at = None;
                        crate::socket::show_home_screen();
                        crate::socket::show_death_notice(true);
                    }
                }

                state.death.tick(
                    &mut state.font_system,
                    dt,
                    state.cursor_pos,
                    window_size,
                    screen,
                );

                Self::update_scoreboard(state, &game, dt);

                Self::update_labels(state, labels, &game);

                Self::update_chat(state, &mut game, now, dt);

                let playing = game.my_player().is_some();

                let instances = {
                    let mut instances =
                        Self::build_world_instances(&game, labels, camera_pos, zoom, aspect_ratio);

                    if playing {
                        Self::build_hud_instances(state, &game, screen, dt, &mut instances);
                    }

                    instances
                };

                Self::fill_hud_text(state, &game);

                if state.debug_dirty {
                    state.debug_dirty = false;

                    Self::fill_debug_text(state, &game);
                }

                let upgrade_levels = game.upgrade_levels;

                let upgrade_points = game.upgrade_points;

                for (i, &lvl) in upgrade_levels.iter().enumerate() {
                    if lvl > state.last_upgrade_levels[i] {
                        state.upgrade_panel.flash(i);
                    }
                }

                state.last_upgrade_levels = upgrade_levels;

                let bubble_anchors: Vec<(u32, Vec2, f32)> = game
                    .players
                    .iter()
                    .filter(|p| !p.dying)
                    .map(|p| (p.id, p.render_pos, p.scale))
                    .collect();

                drop(game);

                state.render_entities_with_text(
                    &instances,
                    &bubble_anchors,
                    camera_pos,
                    zoom,
                    upgrade_points,
                    &upgrade_levels,
                    playing,
                );
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        text,
                        state: key_state,
                        repeat,
                        ..
                    },
                ..
            } => {
                let pressed = key_state.is_pressed();

                if state.chat.is_open() {
                    let mut game = state.game_state.borrow_mut();

                    if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                        game.shift_held = pressed;
                    }

                    match code {
                        KeyCode::Enter | KeyCode::NumpadEnter => {
                            if pressed && !repeat {
                                if let Some(msg) = state.chat.submit(&mut state.font_system) {
                                    let msg = msg.trim();

                                    if !msg.is_empty() {
                                        game.chat_message = Some(msg.to_owned());
                                    }
                                }
                            }
                        }

                        KeyCode::Tab => {
                            if pressed && !repeat {
                                state.chat.switch_channel();

                                game.chat_channel = state.chat.active_channel();
                            }
                        }

                        KeyCode::Escape => {
                            if pressed && !repeat {
                                state.chat.close_input(&mut state.font_system);

                                release_inputs(&mut game);
                            }
                        }

                        KeyCode::Backspace => {
                            if pressed {
                                state.chat.backspace(&mut state.font_system);
                            }
                        }

                        _ => {
                            if pressed {
                                if let Some(t) = text.as_deref() {
                                    if !t.is_empty() {
                                        state.chat.type_text(&mut state.font_system, t);
                                    }
                                }
                            }
                        }
                    }

                    game.update_movement_dir();

                    return;
                }

                let mut game = state.game_state.borrow_mut();

                match code {
                    KeyCode::KeyW | KeyCode::ArrowUp => {
                        game.move_up = pressed;
                    }

                    KeyCode::KeyS | KeyCode::ArrowDown => {
                        game.move_down = pressed;
                    }

                    KeyCode::KeyA | KeyCode::ArrowLeft => {
                        game.move_left = pressed;
                    }

                    KeyCode::KeyD | KeyCode::ArrowRight => {
                        game.move_right = pressed;
                    }

                    KeyCode::Enter | KeyCode::NumpadEnter if state.death.is_active() => {
                        if pressed && !repeat {
                            continue_from_death(&mut state.death);
                        }
                    }

                    KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyT => {
                        if pressed && !repeat {
                            state.chat.open_input();

                            game.move_up = false;

                            game.move_down = false;

                            game.move_left = false;

                            game.move_right = false;

                            release_inputs(&mut game);
                        }
                    }

                    KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                        game.shift_held = pressed;
                    }

                    KeyCode::KeyE => {
                        if pressed && !repeat {
                            game.auto_fire_toggle = !game.auto_fire_toggle;

                            state.notices.push(
                                &mut state.font_system,
                                if game.auto_fire_toggle {
                                    "Auto Fire: ON"
                                } else {
                                    "Auto Fire: OFF"
                                },
                            );
                        }
                    }

                    KeyCode::KeyC => {
                        if pressed && !repeat {
                            game.auto_spin = !game.auto_spin;

                            state.notices.push(
                                &mut state.font_system,
                                if game.auto_spin {
                                    "Auto Spin: ON"
                                } else {
                                    "Auto Spin: OFF"
                                },
                            );
                        }
                    }

                    KeyCode::Backquote => {
                        if pressed && !repeat {
                            state.show_debug = !state.show_debug;

                            state.debug_dirty = true;
                        }
                    }

                    KeyCode::KeyU => {
                        if pressed && !repeat {
                            state.upgrade_panel.toggle();

                            web_sys::console::log_1(
                                &format!(
                                    "upgrade panel: {}",
                                    if state.upgrade_panel.is_pinned() {
                                        "PINNED OPEN"
                                    } else {
                                        "AUTOMATIC (corner/hover)"
                                    }
                                )
                                .into(),
                            );
                        }
                    }

                    KeyCode::KeyK => {
                        if pressed && !repeat {
                            state.class_panel.toggle();

                            web_sys::console::log_1(
                                &format!(
                                    "class panel: {}",
                                    if state.class_panel.is_pinned() {
                                        "PINNED OPEN"
                                    } else {
                                        "AUTOMATIC (flag-driven)"
                                    }
                                )
                                .into(),
                            );
                        }
                    }

                    KeyCode::Digit1 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(1);
                        }
                    }

                    KeyCode::Digit2 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(2);
                        }
                    }

                    KeyCode::Digit3 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(3);
                        }
                    }

                    KeyCode::Digit4 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(4);
                        }
                    }

                    KeyCode::Digit5 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(5);
                        }
                    }

                    KeyCode::Digit6 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(6);
                        }
                    }

                    KeyCode::Digit7 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(7);
                        }
                    }

                    KeyCode::Digit8 => {
                        if pressed && !repeat {
                            game.upgrade_request = Some(8);
                        }
                    }

                    KeyCode::KeyM => {
                        if pressed && !repeat {
                            state.debug_render_mode = (state.debug_render_mode + 1) % 3;

                            state.debug_dirty = true;

                            web_sys::console::log_1(
                                &format!(
                                    "render mode: {}",
                                    match state.debug_render_mode {
                                        0 => "0: full (entities + UI + text)",
                                        1 => "1: no text (glyphon skipped)",
                                        _ => "2: present-only (nothing drawn)",
                                    }
                                )
                                .into(),
                            );
                        }
                    }

                    KeyCode::Backslash => {
                        if pressed && !repeat {
                            state.camera_enabled = !state.camera_enabled;

                            if state.camera_enabled {
                                let target = game.my_player().map(|p| p.render_pos);

                                if let Some(target) = target {
                                    state.camera.snap_to(target);
                                }
                            }

                            web_sys::console::log_1(
                                &format!(
                                    "camera: {}",
                                    if state.camera_enabled {
                                        "LERP (spring)"
                                    } else {
                                        "HARD-LOCK (to smoothed player)"
                                    }
                                )
                                .into(),
                            );
                        }
                    }

                    _ => {}
                }

                game.update_movement_dir();
            }

            _ => {}
        }
    }
}

fn continue_from_death(death: &mut DeathScreen) {
    death.hide();
    crate::socket::show_home_screen();
    crate::socket::show_death_notice(true);
}

fn keep_canvas_focused() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };

    if document
        .active_element()
        .is_some_and(|e| e.id() == "gameCanvas")
    {
        return;
    }

    if let Some(canvas) = document
        .get_element_by_id("gameCanvas")
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = canvas.focus();
    }
}

fn release_inputs(game: &mut GameState) {
    game.fire_held = false;
    game.repel_held = false;
    game.auto_fire = false;
    game.repel = false;
}

pub struct TextComponent {
    pub buffer: glyphon::Buffer,
    pub color: Color,
    pub offset: [f32; 2],
    pub source_text: String,
}

impl TextComponent {
    pub fn new(font_system: &mut FontSystem, initial_text: &str) -> Self {
        let mut buffer = glyphon::Buffer::new(font_system, Metrics::new(24.0, 28.0));

        buffer.set_text(initial_text, &bold_attrs(), Shaping::Basic, None);

        buffer.shape_until_scroll(font_system, false);

        Self {
            buffer,
            color: Color::rgb(255, 255, 255),
            offset: [0.0, 0.0],
            source_text: initial_text.to_string(),
        }
    }

    pub fn measure(&self) -> (f32, f32) {
        let mut width = 0.0f32;
        let mut height = 0.0f32;

        for run in self.buffer.layout_runs() {
            width = width.max(run.line_w);

            height += run.line_height;
        }

        (width, height)
    }

    pub fn center_horizontally(&mut self) {
        let (width, _) = self.measure();

        self.offset[0] = -width / 2.0;
    }

    pub fn update_text(&mut self, font_system: &mut FontSystem, new_text: &str) {
        self.buffer
            .set_text(new_text, &bold_attrs(), Shaping::Basic, None);

        self.buffer.shape_until_scroll(font_system, false);

        self.source_text = new_text.to_string();

        self.center_horizontally();
    }
}
