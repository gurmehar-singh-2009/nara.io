use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use glam::Vec2;
use glyphon::{
    Attrs, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, cosmic_text::Weight,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::window;
use wgpu::{
    Backends, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, Buffer, BufferBindingType, BufferUsages, ColorTargetState,
    ColorWrites, CurrentSurfaceTexture, Device, DeviceDescriptor, ExperimentalFeatures, Features,
    FragmentState, Instance, InstanceDescriptor, Limits, MultisampleState,
    PipelineCompilationOptions, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, ShaderModuleDescriptor, ShaderStages, Surface,
    SurfaceConfiguration, VertexState, util::DeviceExt,
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
        colours::{DARK_THEME, to_glyphon},
        scoreboard::{Scoreboard, bar_ui_instance},
        tank_upgrades::{TankUpgradePanel, default_classes},
        upgrade_panel::UpgradePanel,
    },
    structs::game_state::GameState,
};

const PLAYER_DISPLAY_SMOOTH_TIME: f32 = 0.06;
const INSTANCE_CAPACITY: u64 = 4096; // seems like a big enough amount :thumbsup:
const LOG_EVERY_FRAMES: u32 = 120;

const CULL_MARGIN: f32 = 150.0;
const BULLET_CULL_RADIUS: f32 = 64.0;

const SCORE_BAR_W_FRAC: f32 = 0.5;
const SCORE_BAR_H: f32 = 36.0;
const SCORE_BAR_BOTTOM: f32 = 48.0;
const SCORE_TEXT_OUTLINE_PX: f32 = 2.0;

const HEALTH_BAR_BORDER: f32 = 1.5;
const HEALTH_BAR_INSET: f32 = 2.5;

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

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    text_buffer: glyphon::Buffer,
    last_stats_text: String,
    score_bar_buffer: glyphon::Buffer,
    last_score_bar_text: String,
    scoreboard: Scoreboard,
    upgrade_panel: UpgradePanel,
    chat: ChatPanel,
    class_panel: TankUpgradePanel,

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
    pub async fn new(window: Arc<Window>, game: Rc<RefCell<GameState>>) -> Self {
        let size = window.inner_size();

        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::BROWSER_WEBGPU,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: true,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: None,
                required_features: Features::empty(),
                experimental_features: ExperimentalFeatures::disabled(),
                required_limits: Limits::defaults(),
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();

        let scale_factor = window.scale_factor();
        let inner_size = window.inner_size();

        let physical_width = (inner_size.width as f64 * scale_factor) as u32;
        let physical_height = (inner_size.height as f64 * scale_factor) as u32;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: physical_width,
            height: physical_height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };

        let aspect_ratio = if size.height > 0 {
            size.width as f32 / size.height as f32
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
            zoom: 0.005,
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

        let mut score_bar_buffer = glyphon::Buffer::new(&mut font_system, Metrics::new(26.0, 30.0));
        score_bar_buffer.set_size(Some(physical_width as f32), Some(physical_height as f32));
        score_bar_buffer.set_text("", &bold_attrs(), Shaping::Basic, None);
        score_bar_buffer.shape_until_scroll(&mut font_system, false);

        let mut camera = CameraController::new();
        camera.zoom = 0.005;

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
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            text_buffer,
            last_stats_text: String::new(),
            score_bar_buffer,
            last_score_bar_text: String::new(),
            scoreboard,
            upgrade_panel,
            chat,
            class_panel,
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
        let scale_factor = self.window.scale_factor();

        let physical_width = (width as f64 * scale_factor) as u32;
        let physical_height = (height as f64 * scale_factor) as u32;

        if physical_width > 0 && physical_height > 0 {
            self.config.width = physical_width;
            self.config.height = physical_height;
            self.surface.configure(&self.device, &self.config);
            self.is_surface_configured = true;

            self.viewport.update(
                &self.queue,
                Resolution {
                    width: physical_width,
                    height: physical_height,
                },
            );

            self.text_buffer
                .set_size(Some(physical_width as f32), Some(physical_height as f32));
            self.score_bar_buffer
                .set_size(Some(physical_width as f32), Some(physical_height as f32));

            self.update_camera([self.camera.pos.x, self.camera.pos.y], self.camera.zoom);
        }
    }

    pub fn update(&mut self, instances: &[EntityInstance]) {
        self.num_instances = instances.len() as u32;

        if instances.is_empty() {
            return;
        }

        let raw_data = bytemuck::cast_slice(instances);
        let required_size = raw_data.len() as u64;

        if required_size > self.instance_buffer.size() {
            self.instance_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Dynamic Instance Buffer"),
                        contents: raw_data,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    });
        } else {
            self.queue.write_buffer(&self.instance_buffer, 0, raw_data);
        }
    }

    fn update_camera(&self, camera_pos: [f32; 2], zoom: f32) {
        let aspect_ratio = self.config.width as f32 / self.config.height.max(1) as f32;

        let camera = CameraUniform {
            view_proj: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            camera_pos,
            zoom,
            aspect_ratio,
            screen_size: [self.config.width as f32, self.config.height as f32],
            _pad: [0.0, 0.0],
        };

        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
    }

    pub fn render_entities_with_text(
        &mut self,
        entities: &[RenderEntity],
        camera_pos: [f32; 2],
        zoom: f32,
        stats_text: &str,
        score_text: &str,
        upgrade_levels: &[u8; 8],
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

        let (ui_instances, scoreboard_areas) = if mode == 0 {
            self.scoreboard.render_data(screen)
        } else {
            (Vec::new(), Vec::new())
        };
        let (panel_instances, panel_areas) = if mode == 0 {
            self.upgrade_panel.render_data(screen, upgrade_levels)
        } else {
            (Vec::new(), Vec::new())
        };
        let (chat_instances, chat_areas) = if mode == 0 {
            self.chat.render_data(screen)
        } else {
            (Vec::new(), Vec::new())
        };
        let (class_instances, class_areas) = if mode == 0 {
            self.class_panel.render_data(screen)
        } else {
            (Vec::new(), Vec::new())
        };

        let mut all_instances: Vec<EntityInstance> = entities.iter().map(|e| e.instance).collect();
        all_instances.extend(ui_instances);
        all_instances.extend(panel_instances);
        all_instances.extend(chat_instances);
        all_instances.extend(class_instances);

        self.num_instances = all_instances.len() as u32;
        if mode < 2 && !all_instances.is_empty() {
            let raw_data = bytemuck::cast_slice(&all_instances);
            let required_size = raw_data.len() as u64;

            if required_size > self.instance_buffer.size() {
                self.instance_buffer =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Dynamic Instance Buffer"),
                            contents: raw_data,
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        });
            } else {
                self.queue.write_buffer(&self.instance_buffer, 0, raw_data);
            }
        }

        let output = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture)
            | CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation
            | wgpu::CurrentSurfaceTexture::Lost => return,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format),
            ..Default::default()
        });

        if mode == 0 && stats_text != self.last_stats_text {
            self.text_buffer.set_text(
                stats_text,
                &Attrs::new().family(glyphon::Family::SansSerif),
                Shaping::Basic,
                None,
            );
            self.text_buffer
                .shape_until_scroll(&mut self.font_system, false);
            self.last_stats_text = stats_text.to_string();
        }

        if mode == 0 && score_text != self.last_score_bar_text {
            self.score_bar_buffer
                .set_text(score_text, &bold_attrs(), Shaping::Basic, None);
            self.score_bar_buffer
                .shape_until_scroll(&mut self.font_system, false);
            self.last_score_bar_text = score_text.to_string();
        }

        let mut text_areas = Vec::new();

        if mode == 0 {
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
            let text_border_thickness = 3.0;
            let text_border_color = to_glyphon(DARK_THEME.fill_border);

            let aspect_ratio = if screen_h > 0.0 {
                screen_w / screen_h
            } else {
                1.0
            };

            for entity in entities {
                if let Some(text_comp) = entity.text {
                    let (screen_x, screen_y) = Renderer::world_to_screen(
                        entity.instance.position,
                        camera_pos,
                        zoom,
                        aspect_ratio,
                        screen_w,
                        screen_h,
                    );

                    let base_left = screen_x + text_comp.offset[0];
                    let base_top = screen_y + text_comp.offset[1];

                    for (dx, dy) in OUTLINE_DIRS {
                        text_areas.push(TextArea {
                            buffer: &text_comp.buffer,
                            left: base_left + dx * text_border_thickness,
                            top: base_top + dy * text_border_thickness,
                            scale: 1.0,
                            bounds: TextBounds {
                                left: 0,
                                top: 0,
                                right: self.config.width as i32,
                                bottom: self.config.height as i32,
                            },
                            default_color: text_border_color,
                            custom_glyphs: &[],
                        });
                    }

                    text_areas.push(TextArea {
                        buffer: &text_comp.buffer,
                        left: base_left,
                        top: base_top,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: 0,
                            top: 0,
                            right: self.config.width as i32,
                            bottom: self.config.height as i32,
                        },
                        default_color: text_comp.color,
                        custom_glyphs: &[],
                    });
                }
            }

            text_areas.push(TextArea {
                buffer: &self.text_buffer,
                left: 20.0,
                top: 20.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: self.config.width as i32,
                    bottom: self.config.height as i32,
                },
                default_color: Color::rgb(255, 255, 255),
                custom_glyphs: &[],
            });

            {
                let bar_cx = screen_w * 0.5;
                let bar_cy = screen_h - SCORE_BAR_BOTTOM;
                let text_w = Renderer::text_width(&self.score_bar_buffer);
                let left = bar_cx - text_w * 0.5;
                let top = bar_cy - 15.0; // line height 30 on a 36px bar

                for (dx, dy) in OUTLINE_DIRS {
                    text_areas.push(TextArea {
                        buffer: &self.score_bar_buffer,
                        left: left + dx * SCORE_TEXT_OUTLINE_PX,
                        top: top + dy * SCORE_TEXT_OUTLINE_PX,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: 0,
                            top: 0,
                            right: self.config.width as i32,
                            bottom: self.config.height as i32,
                        },
                        default_color: text_border_color,
                        custom_glyphs: &[],
                    });
                }

                text_areas.push(TextArea {
                    buffer: &self.score_bar_buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: self.config.width as i32,
                        bottom: self.config.height as i32,
                    },
                    default_color: Color::rgb(255, 255, 255),
                    custom_glyphs: &[],
                });
            }

            text_areas.extend(scoreboard_areas);
            text_areas.extend(panel_areas);
            text_areas.extend(chat_areas);
            text_areas.extend(class_areas);

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

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

            if mode < 2 {
                render_pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                render_pass.draw(0..6, 0..self.num_instances);
            }

            if mode == 0 {
                self.text_renderer
                    .render(&self.atlas, &self.viewport, &mut render_pass)
                    .unwrap();
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(output);
    }
}

pub struct Renderer {
    proxy: Option<EventLoopProxy<RenderState>>,
    state: Option<RenderState>,
    game_state: Rc<RefCell<GameState>>,
    labels: HashMap<u32, TextComponent>,
}

impl Renderer {
    pub fn new(event_loop: &EventLoop<RenderState>, game_state: Rc<RefCell<GameState>>) -> Self {
        Self {
            proxy: Some(event_loop.create_proxy()),
            state: None,
            game_state,
            labels: HashMap::new(),
        }
    }

    pub fn world_to_screen(
        world_pos: [f32; 2],
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
        screen_width: f32,
        screen_height: f32,
    ) -> (f32, f32) {
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
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
    ) -> [f32; 2] {
        let ndc_x = (cursor[0] / window_width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (cursor[1] / window_height) * 2.0;

        let rel_x = ndc_x * aspect_ratio / zoom;
        let rel_y = ndc_y / zoom;

        [camera_pos[0] + rel_x, camera_pos[1] + rel_y]
    }

    fn text_width(buffer: &glyphon::Buffer) -> f32 {
        buffer.layout_runs().next().map(|r| r.line_w).unwrap_or(0.0)
    }

    fn view_bounds(
        camera_pos: [f32; 2],
        zoom: f32,
        aspect_ratio: f32,
        margin: f32,
    ) -> (Vec2, Vec2) {
        let half_w = aspect_ratio / zoom + margin;
        let half_h = 1.0 / zoom + margin;
        let c = Vec2::new(camera_pos[0], camera_pos[1]);
        (c - Vec2::new(half_w, half_h), c + Vec2::new(half_w, half_h))
    }

    #[inline]
    fn is_visible(pos: Vec2, radius: f32, min: Vec2, max: Vec2) -> bool {
        pos.x + radius >= min.x
            && pos.x - radius <= max.x
            && pos.y + radius >= min.y
            && pos.y - radius <= max.y
    }

    fn begin_frame(state: &mut RenderState) -> (f64, f32) {
        let now = window().unwrap().performance().unwrap().now();
        let dt = match state.last_frame_time {
            Some(last) => (((now - last) / 1000.0).clamp(0.0, 0.1)) as f32,
            None => 1.0 / 60.0,
        };
        state.last_frame_time = Some(now);
        (now, dt)
    }

    fn log_stats(state: &mut RenderState, game: &GameState, now: f64) {
        // if state.last_log_time == 0.0 {
        //     state.last_log_time = now;
        //     return;
        // }

        // state.log_frames += 1;
        // if state.log_frames >= LOG_EVERY_FRAMES {
        //     let elapsed = ((now - state.last_log_time) / 1000.0).max(0.001);
        //     let fps = state.log_frames as f64 / elapsed;
        //     web_sys::console::log_1(
        //         &format!(
        //             "fps {:.0} | players {} | shapes {} | bullets {} | inst
        // {} | mode {}",             fps,
        //             game.players.len(),
        //             game.shapes.len(),
        //             game.bullets.len(),
        //             state.num_instances,
        //             state.debug_render_mode,
        //         )
        //         .into(),
        //     );
        //     state.log_frames = 0;
        //     state.last_log_time = now;
        // }
    }

    fn advance_entities(state: &mut RenderState, game: &mut GameState, now: f64, dt: f32) {
        game.tick_render(dt);

        const BULLET_STALE_FADE_MS: f64 = 450.0;
        const BULLET_STALE_CULL_MS: f64 = 900.0;
        for b in game.bullets.iter_mut() {
            if now - b.last_update_time > BULLET_STALE_FADE_MS {
                b.dying = true;
                b.render_alpha -= dt * 4.0;
            }
        }
        game.bullets
            .retain(|b| b.render_alpha > 0.0 && (now - b.last_update_time) < BULLET_STALE_CULL_MS);

        const SHAPE_STALE_MS: f64 = 3000.0;
        for s in game.shapes.iter_mut() {
            if !s.dying && now - s.last_update_time > SHAPE_STALE_MS {
                s.dying = true;
            }
        }

        let my_player_id = game.my_player_id;

        for p in game.players.iter_mut() {
            let alpha = ((now - p.last_update_time) / 100.0).min(1.0) as f32;
            p.render_pos = p.last_pos.lerp(p.pos, alpha);
            p.render_health += (p.health as f32 - p.render_health) * 0.1;

            if Some(p.id) != my_player_id {
                let mut diff = p.rot - p.last_rot;
                if diff > std::f32::consts::PI {
                    diff -= std::f32::consts::TAU;
                }
                if diff < -std::f32::consts::PI {
                    diff += std::f32::consts::TAU;
                }
                p.render_rot = p.last_rot + diff * alpha;
            }
        }

        for s in game.shapes.iter_mut() {
            let alpha = ((now - s.last_update_time) / 100.0).min(1.0) as f32;
            s.render_pos = s.last_pos.lerp(s.pos, alpha);
            s.render_health += (s.health as f32 - s.render_health) * 0.1;

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
            let alpha = ((now - b.last_update_time) / 100.0).min(1.0) as f32;
            b.render_pos = b.last_pos.lerp(b.pos, alpha);

            let mut diff = b.rot - b.last_rot;
            if diff > std::f32::consts::PI {
                diff -= std::f32::consts::TAU;
            }
            if diff < -std::f32::consts::PI {
                diff += std::f32::consts::TAU;
            }
            b.render_rot = b.last_rot + diff * alpha;
        }

        if let Some(p) = game.my_player_mut() {
            let smoothed = state.player_display.update(p.render_pos, dt);
            p.render_pos = smoothed;
        }
    }

    fn update_camera_and_aim(state: &mut RenderState, game: &mut GameState, dt: f32) {
        let has_player = game.my_player().is_some();
        if has_player && !state.had_player {
            if let Some(p) = game.my_player() {
                state.player_display.snap_to(p.render_pos);
                state.camera.snap_to(p.render_pos);
            }
        }
        state.had_player = has_player;

        let my_player_scale = game.my_player().map(|p| p.scale).unwrap_or(1.0);
        let target_zoom = 0.005 / my_player_scale;
        state.camera.update_zoom(target_zoom, dt);

        if let Some(p) = game.my_player() {
            if state.camera_enabled {
                state.camera.update(p.render_pos, dt);
            } else {
                state.camera.pos = p.render_pos;
            }
        }

        if let Some(cursor) = state.cursor_pos {
            let win = state.window.inner_size();
            let (win_w, win_h) = (win.width as f32, win.height as f32);
            if win_w > 0.0 && win_h > 0.0 {
                let aspect_ratio = state.config.width as f32 / state.config.height.max(1) as f32;
                let cursor_world = Self::cursor_to_world(
                    [cursor.x, cursor.y],
                    win_w,
                    win_h,
                    [state.camera.pos.x, state.camera.pos.y],
                    state.camera.zoom,
                    aspect_ratio,
                );
                if let Some(p) = game.my_player() {
                    let dx = cursor_world[0] - p.render_pos.x;
                    let dy = cursor_world[1] - p.render_pos.y;
                    if dx != 0.0 || dy != 0.0 {
                        game.mouse_angle = Some(dy.atan2(dx));
                    }
                }
            }
        }

        if let Some(target) = game.mouse_angle {
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
                    tc.set_centered_offset(-560.0);
                }
                None => {
                    labels.insert(p.id, TextComponent::new(&mut state.font_system, &p.name));
                }
            }
        }
        labels.retain(|id, _| game.players.iter().any(|p| p.id == *id));
    }

    fn update_chat(state: &mut RenderState, game: &mut GameState, now: f64, dt: f32) {
        let incoming = std::mem::take(&mut game.incoming_chat);
        for (channel, msg) in incoming.iter() {
            state.chat.receive(&mut state.font_system, *channel, msg);
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
        const MAP_BOUND: f32 = 2500.0;
        let border_color = DARK_THEME.maze_walls;

        let mut instances: Vec<RenderEntity<'a>> = vec![
            RenderEntity {
                instance: EntityInstance {
                    position: [0.0, 0.0],
                    size: [10000.0, 10000.0],
                    rotation: 0.0,
                    shape_type: 2,
                    sides: 0,
                    fill_color: DARK_THEME.background,
                    border_color: DARK_THEME.grid,
                    border_thickness: 2.0,
                    extra_param: 64.0,
                },
                text: None,
            },
            RenderEntity {
                instance: EntityInstance {
                    position: [0.0, MAP_BOUND + 25.0],
                    size: [(MAP_BOUND + 50.0) * 2.0, 50.0],
                    rotation: 0.0,
                    shape_type: 1,
                    sides: 4,
                    fill_color: border_color,
                    border_color,
                    border_thickness: 0.0,
                    extra_param: 1.0,
                },
                text: None,
            },
            RenderEntity {
                instance: EntityInstance {
                    position: [0.0, -MAP_BOUND - 25.0],
                    size: [(MAP_BOUND + 50.0) * 2.0, 50.0],
                    rotation: 0.0,
                    shape_type: 1,
                    sides: 4,
                    fill_color: border_color,
                    border_color,
                    border_thickness: 0.0,
                    extra_param: 1.0,
                },
                text: None,
            },
            RenderEntity {
                instance: EntityInstance {
                    position: [MAP_BOUND + 25.0, 0.0],
                    size: [50.0, (MAP_BOUND + 50.0) * 2.0],
                    rotation: 0.0,
                    shape_type: 1,
                    sides: 4,
                    fill_color: border_color,
                    border_color,
                    border_thickness: 0.0,
                    extra_param: 1.0,
                },
                text: None,
            },
            RenderEntity {
                instance: EntityInstance {
                    position: [-MAP_BOUND - 25.0, 0.0],
                    size: [50.0, (MAP_BOUND + 50.0) * 2.0],
                    rotation: 0.0,
                    shape_type: 1,
                    sides: 4,
                    fill_color: border_color,
                    border_color,
                    border_thickness: 0.0,
                    extra_param: 1.0,
                },
                text: None,
            },
        ];

        let (cull_min, cull_max) = Self::view_bounds(camera_pos, zoom, aspect_ratio, CULL_MARGIN);

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

            let bar_w = s.size * 0.7;
            let bar_h = 7.0;
            let bar_y = s.render_pos.y - s.size * 0.5;
            let inset = 1.5;
            let fg_h = bar_h - inset * 2.0;
            let inner_w = bar_w - inset * 2.0;

            let health_percent = (s.render_health / s.max_health as f32).max(0.0).min(1.0);
            let fg_w = inner_w * health_percent;
            let inner_left = s.render_pos.x - bar_w * 0.5 + inset;

            instances.push(RenderEntity {
                instance: EntityInstance {
                    position: [s.render_pos.x, bar_y],
                    size: [bar_w, bar_h],
                    rotation: 0.0,
                    shape_type: 4,
                    sides: 4,
                    fill_color: DARK_THEME.health_bar_background,
                    border_color: DARK_THEME.scoreboard_row,
                    border_thickness: HEALTH_BAR_BORDER,
                    extra_param: 1.0, // full pill
                },
                text: None,
            });

            if fg_w > 0.1 {
                instances.push(RenderEntity {
                    instance: EntityInstance {
                        position: [inner_left + fg_w * 0.5, bar_y],
                        size: [fg_w, fg_h],
                        rotation: 0.0,
                        shape_type: 4,
                        sides: 4,
                        fill_color: DARK_THEME.health_bar_foreground,
                        border_color: [0.0, 0.0, 0.0, 0.0],
                        border_thickness: 0.0,
                        extra_param: 1.0, // full pill
                    },
                    text: None,
                });
            }
        }

        for p in game.players.iter() {
            let label = labels.get(&p.id);
            let tank_instances = p.get_render_instances();
            let body_idx = tank_instances.len().saturating_sub(1);
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
            let bar_w = 44.0 * p.scale;
            let bar_h = 10.0 * p.scale;
            let bar_y = p.render_pos.y - 32.0 * p.scale;
            let inset = HEALTH_BAR_INSET * p.scale;
            let fg_h = bar_h - inset * 2.0;
            let inner_w = bar_w - inset * 2.0;

            let health_percent = (p.render_health / p.max_health as f32).max(0.0).min(1.0);
            let fg_w = inner_w * health_percent;
            let inner_left = p.render_pos.x - bar_w * 0.5 + inset;

            instances.push(RenderEntity {
                instance: EntityInstance {
                    position: [p.render_pos.x, bar_y],
                    size: [bar_w, bar_h],
                    rotation: 0.0,
                    shape_type: 4,
                    sides: 4,
                    fill_color: DARK_THEME.health_bar_background,
                    border_color: DARK_THEME.scoreboard_row,
                    border_thickness: HEALTH_BAR_BORDER * p.scale,
                    extra_param: 1.0, // full pill
                },
                text: None,
            });

            if fg_w > 0.1 {
                instances.push(RenderEntity {
                    instance: EntityInstance {
                        position: [inner_left + fg_w * 0.5, bar_y],
                        size: [fg_w, fg_h],
                        rotation: 0.0,
                        shape_type: 4,
                        sides: 4,
                        fill_color: DARK_THEME.health_bar_foreground,
                        border_color: [0.0, 0.0, 0.0, 0.0],
                        border_thickness: 0.0,
                        extra_param: 1.0, // full pill
                    },
                    text: None,
                });
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
            .unwrap_or(game.xp)
    }

    fn build_hud_instances<'a>(game: &GameState, screen: Vec2) -> Vec<RenderEntity<'a>> {
        let mut instances: Vec<RenderEntity<'a>> = Vec::new();

        let bar_w = screen.x * SCORE_BAR_W_FRAC;
        let bar_h = SCORE_BAR_H;
        let bar_left = screen.x * 0.5 - bar_w * 0.5;
        let bar_cy = screen.y - SCORE_BAR_BOTTOM;

        instances.push(RenderEntity {
            instance: bar_ui_instance(
                Vec2::new(bar_left + bar_w * 0.5, bar_cy),
                Vec2::new(bar_w, bar_h),
                screen,
                DARK_THEME.bar_background,
            ),
            text: None,
        });

        let score = Self::my_score(game);
        let top = game
            .leaderboard
            .iter()
            .map(|e| e.1)
            .max()
            .unwrap_or(0)
            .max(score);
        let fill = if top > 0 {
            (score as f32 / top as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let fg_w = bar_w * fill;
        if fg_w > 2.0 {
            let fg_w = fg_w.max(bar_h);
            instances.push(RenderEntity {
                instance: bar_ui_instance(
                    Vec2::new(bar_left + fg_w * 0.5, bar_cy),
                    Vec2::new(fg_w, bar_h),
                    screen,
                    DARK_THEME.xp_bar_fill,
                ),
                text: None,
            });
        }

        instances
    }

    fn stats_text(game: &GameState) -> String {
        format!(
            "Lvl {} | XP: {}/{} | HP: {}/{}",
            game.level, game.xp, game.xp_to_next, game.health, game.max_health
        )
    }

    fn score_bar_text(game: &GameState) -> String {
        format!("Score: {}", Self::my_score(game))
    }
}

impl ApplicationHandler<RenderState> for Renderer {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = wgpu::web_sys::window().unwrap_throw();
        let document = window.document().unwrap_throw();
        let canvas: web_sys::HtmlCanvasElement = document
            .get_element_by_id("gameCanvas")
            .unwrap_throw()
            .unchecked_into();

        let dpr = window.device_pixel_ratio();

        let client_width = canvas.client_width() as f64;
        let client_height = canvas.client_height() as f64;

        canvas.set_width((client_width * dpr) as u32);
        canvas.set_height((client_height * dpr) as u32);

        let mut window_attribs = Window::default_attributes();

        let window = wgpu::web_sys::window().unwrap_throw();
        let document = window.document().unwrap_throw();
        let canvas = document.get_element_by_id("gameCanvas").unwrap_throw();
        let html_canvas_element = canvas.unchecked_into();
        window_attribs = window_attribs.with_canvas(Some(html_canvas_element));

        let window = Arc::new(event_loop.create_window(window_attribs).unwrap());

        if let Some(proxy) = self.proxy.take() {
            let game_state = Rc::clone(&self.game_state);

            spawn_local(async move {
                assert!(
                    proxy
                        .send_event(RenderState::new(window, game_state).await)
                        .is_ok()
                )
            });
        }
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        mut event: RenderState,
    ) {
        event.window.request_redraw();

        event.resize(
            event.window.inner_size().width,
            event.window.inner_size().height,
        );
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
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => state.resize(size.width, size.height),

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_pos = Some(Vec2::new(position.x as f32, position.y as f32));
            }

            WindowEvent::MouseInput {
                state: mouse_state,
                button,
                ..
            } => {
                if mouse_state.is_pressed() && button == MouseButton::Left {
                    if let Some(cursor) = state.cursor_pos {
                        let win = state.window.inner_size();
                        let window = Vec2::new(win.width as f32, win.height as f32);
                        let screen =
                            Vec2::new(state.config.width as f32, state.config.height.max(1) as f32);

                        if let Some(channel) = state.chat.hit_test_tab(cursor, window, screen) {
                            state.chat.set_channel(channel);
                            let mut game = state.game_state.borrow_mut();
                            game.chat_channel = channel;
                        } else if let Some(idx) = state.class_panel.hit_test(cursor, window, screen)
                        {
                            let mut game = state.game_state.borrow_mut();
                            game.class_choice = Some(idx as u8 + 1);
                            state.class_panel.set_pinned(false);
                            web_sys::console::log_1(&format!("class choice: {}", idx + 1).into());
                        } else if let Some(idx) =
                            state.upgrade_panel.hit_test(cursor, window, screen)
                        {
                            let mut game = state.game_state.borrow_mut();
                            game.upgrade_request = Some(idx as u8 + 1);
                            web_sys::console::log_1(
                                &format!("upgrade requested: [{}]", idx + 1).into(),
                            );
                        }
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                let (now, dt) = Self::begin_frame(state);
                let screen =
                    Vec2::new(state.config.width as f32, state.config.height.max(1) as f32);
                let aspect_ratio = screen.x / screen.y;

                {
                    let game_rc = Rc::clone(&state.game_state);
                    let mut game = game_rc.borrow_mut();

                    Self::log_stats(state, &game, now);

                    Self::advance_entities(state, &mut game, now, dt);

                    Self::update_camera_and_aim(state, &mut game, dt);

                    let camera_pos = [state.camera.pos.x, state.camera.pos.y];
                    let zoom = state.camera.zoom;

                    let win = state.window.inner_size();
                    let window = Vec2::new(win.width as f32, win.height as f32);
                    state
                        .upgrade_panel
                        .tick(dt, state.cursor_pos, window, screen);

                    state.class_panel.tick(dt, game.class_upgrades_available);

                    Self::update_scoreboard(state, &game, dt);

                    Self::update_labels(state, labels, &game);

                    Self::update_chat(state, &mut game, now, dt);

                    let mut instances =
                        Self::build_world_instances(&game, labels, camera_pos, zoom, aspect_ratio);
                    instances.extend(Self::build_hud_instances(&game, screen));
                    let stats_text = Self::stats_text(&game);
                    let score_text = Self::score_bar_text(&game);

                    let upgrade_levels = game.upgrade_levels;

                    drop(game);

                    state.render_entities_with_text(
                        &instances,
                        camera_pos,
                        zoom,
                        &stats_text,
                        &score_text,
                        &upgrade_levels,
                    );
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        text,
                        state: key_state,
                        ..
                    },
                ..
            } => {
                let pressed = key_state.is_pressed();

                if state.chat.is_open() {
                    let mut game = state.game_state.borrow_mut();
                    match code {
                        KeyCode::Enter | KeyCode::NumpadEnter => {
                            if pressed {
                                if let Some(msg) = state.chat.submit(&mut state.font_system) {
                                    game.chat_message = Some(msg.clone());
                                    let channel = game.chat_channel;
                                    state.chat.receive(&mut state.font_system, channel, &msg);
                                }
                            }
                        }
                        KeyCode::Tab => {
                            if pressed {
                                state.chat.switch_channel();
                                game.chat_channel = state.chat.active_channel();
                            }
                        }
                        KeyCode::Escape => {
                            if pressed {
                                state.chat.close_input(&mut state.font_system);
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
                                    state.chat.type_text(&mut state.font_system, t);
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
                    KeyCode::Space => {
                        game.auto_fire = pressed;
                    }
                    KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyT => {
                        if pressed {
                            state.chat.open_input();
                            game.move_up = false;
                            game.move_down = false;
                            game.move_left = false;
                            game.move_right = false;
                            game.auto_fire = false;
                        }
                    }
                    KeyCode::KeyU => {
                        if pressed {
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
                        if pressed {
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
                        if pressed {
                            game.upgrade_request = Some(1);
                        }
                    }
                    KeyCode::Digit2 => {
                        if pressed {
                            game.upgrade_request = Some(2);
                        }
                    }
                    KeyCode::Digit3 => {
                        if pressed {
                            game.upgrade_request = Some(3);
                        }
                    }
                    KeyCode::Digit4 => {
                        if pressed {
                            game.upgrade_request = Some(4);
                        }
                    }
                    KeyCode::Digit5 => {
                        if pressed {
                            game.upgrade_request = Some(5);
                        }
                    }
                    KeyCode::Digit6 => {
                        if pressed {
                            game.upgrade_request = Some(6);
                        }
                    }
                    KeyCode::Digit7 => {
                        if pressed {
                            game.upgrade_request = Some(7);
                        }
                    }
                    KeyCode::Digit8 => {
                        if pressed {
                            game.upgrade_request = Some(8);
                        }
                    }
                    KeyCode::KeyM => {
                        if pressed {
                            state.debug_render_mode = (state.debug_render_mode + 1) % 3;
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
                        if pressed {
                            state.camera_enabled = !state.camera_enabled;
                            if state.camera_enabled {
                                let target = game.my_player().map(|p| p.render_pos);
                                if let Some(t) = target {
                                    state.camera.snap_to(t);
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

pub struct TextComponent {
    pub buffer: glyphon::Buffer,
    pub color: Color,
    pub offset: [f32; 2],
    pub source_text: String,
}

impl TextComponent {
    pub fn new(font_system: &mut FontSystem, initial_text: &str) -> Self {
        let mut buffer = glyphon::Buffer::new(font_system, Metrics::new(72.0, 80.0));

        buffer.set_text(
            initial_text,
            &Attrs::new().family(glyphon::Family::SansSerif),
            Shaping::Basic,
            None,
        );
        buffer.shape_until_scroll(font_system, false);

        let mut component = Self {
            buffer,
            color: Color::rgb(255, 255, 255),
            offset: [0.0, 0.0],
            source_text: initial_text.to_string(),
        };

        component.set_centered_offset(-560.0);
        component
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

    pub fn set_centered_offset(&mut self, y_offset: f32) {
        let (width, _) = self.measure();
        self.offset = [-width / 2.0, y_offset];
    }

    pub fn update_text(&mut self, font_system: &mut FontSystem, new_text: &str) {
        self.buffer.set_text(
            new_text,
            &Attrs::new().family(glyphon::Family::SansSerif),
            Shaping::Basic,
            None,
        );
        self.buffer.shape_until_scroll(font_system, false);
        self.source_text = new_text.to_string();
    }
}
