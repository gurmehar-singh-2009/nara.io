use std::{cell::RefCell, rc::Rc, sync::Arc};

use glyphon::{
    Attrs, Cache, Color, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport,
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
    event::{KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{KeyCode, PhysicalKey},
    platform::web::*,
    window::Window,
};

use crate::{
    entities::Entity,
    render::{
        buffers::{CameraUniform, EntityInstance},
        colours::DARK_THEME,
    },
    structs::game_state::GameState,
};

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
    leaderboard_buffer: glyphon::Buffer,

    render_zoom: f32,

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
        };
v
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

        let sample_instances: &[EntityInstance] = &[];

        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(sample_instances),
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
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

        let mut leaderboard_buffer =
            glyphon::Buffer::new(&mut font_system, Metrics::new(72.0, 84.0));
        leaderboard_buffer.set_size(Some(physical_width as f32), Some(physical_height as f32));

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
            leaderboard_buffer,
            render_zoom: 0.005,
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

            self.leaderboard_buffer
                .set_size(Some(physical_width as f32), Some(physical_height as f32));

            let aspect_ratio = physical_width as f32 / physical_height as f32;

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
            };

            self.queue.write_buffer(
                &self.camera_buffer,
                0,
                bytemuck::cast_slice(&[camera_uniform]),
            );
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
        leaderboard_text: &str,
    ) {
        self.window.request_redraw();

        if !self.is_surface_configured || entities.is_empty() {
            return;
        }

        self.update_camera(camera_pos, zoom);

        let instances: Vec<EntityInstance> = entities.iter().map(|e| e.instance).collect();
        self.update(&instances);

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

        let screen_w = self.config.width as f32;
        let screen_h = self.config.height as f32;
        let aspect_ratio = if screen_h > 0.0 {
            screen_w / screen_h
        } else {
            1.0
        };

        let mut text_areas = Vec::new();

        let text_border_thickness = 3.0;
        let text_border_color = Color::rgb(0, 0, 0);

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

                for x in -text_border_thickness as i32..=text_border_thickness as i32 {
                    for y in -text_border_thickness as i32..=text_border_thickness as i32 {
                        if x == 0 && y == 0 {
                            continue;
                        }

                        text_areas.push(TextArea {
                            buffer: &text_comp.buffer,
                            left: base_left + x as f32,
                            top: base_top + y as f32,
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

        self.text_buffer.set_text(
            stats_text,
            &Attrs::new().family(glyphon::Family::SansSerif),
            Shaping::Basic,
            None,
        );
        self.text_buffer
            .shape_until_scroll(&mut self.font_system, false);

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

        self.leaderboard_buffer.set_text(
            leaderboard_text,
            &Attrs::new().family(glyphon::Family::SansSerif),
            Shaping::Basic,
            None,
        );
        self.leaderboard_buffer
            .shape_until_scroll(&mut self.font_system, false);

        text_areas.push(TextArea {
            buffer: &self.leaderboard_buffer,
            left: screen_w - 600.0,
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

        let _ = self.text_renderer.prepare(
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        );

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
            render_pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            render_pass.draw(0..6, 0..self.num_instances);

            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut render_pass)
                .unwrap();
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(output);

        self.atlas.trim();
    }
}

pub struct Renderer {
    proxy: Option<EventLoopProxy<RenderState>>,
    state: Option<RenderState>,
    game_state: Rc<RefCell<GameState>>,
}

impl Renderer {
    pub fn new(event_loop: &EventLoop<RenderState>, game_state: Rc<RefCell<GameState>>) -> Self {
        Self {
            proxy: Some(event_loop.create_proxy()),
            state: None,
            game_state,
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
        let state = match &mut self.state {
            Some(c) => c,
            None => return,
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::CursorMoved { position, .. } => {
                let size = state.window.inner_size();
                if size.width > 0 && size.height > 0 {
                    let center_x = size.width as f32 / 2.0;
                    let center_y = size.height as f32 / 2.0;

                    let dx = position.x as f32 - center_x;
                    let dy = center_y - position.y as f32;

                    let angle = dy.atan2(dx);

                    let mut game = state.game_state.borrow_mut();
                    game.mouse_angle = Some(angle);
                    drop(game);
                }
            }
            WindowEvent::RedrawRequested => {
                let game_state = Rc::clone(&state.game_state);
                let mut game = game_state.borrow_mut();

                let labels: Vec<TextComponent> = game
                    .players
                    .iter()
                    .map(|x| TextComponent::new(&mut state.font_system, x.name.as_str()))
                    .collect();

                let current_time = window().unwrap().performance().unwrap().now();

                game.bullets
                    .retain(|b| current_time - b.last_update_time < 500.0);

                let my_player_id = game.my_player_id;
                let mouse_angle = game.mouse_angle;

                game.players.iter_mut().for_each(|p| {
                    let time_elapsed = current_time - p.last_update_time;
                    let alpha = (time_elapsed / 100.0).min(1.0) as f32;
                    p.render_pos = p.last_pos.lerp(p.pos, alpha as f32);
                    p.render_health += (p.health as f32 - p.render_health) * 0.1;

                    if Some(p.id) == my_player_id {
                        if let Some(target_rot) = mouse_angle {
                            let mut diff = target_rot - p.render_rot;
                            if diff > std::f32::consts::PI {
                                diff -= std::f32::consts::TAU;
                            }
                            if diff < -std::f32::consts::PI {
                                diff += std::f32::consts::TAU;
                            }
                            p.render_rot += diff * 0.3;
                        }
                    } else {
                        let mut diff = p.rot - p.last_rot;
                        if diff > std::f32::consts::PI {
                            diff -= std::f32::consts::TAU;
                        }
                        if diff < -std::f32::consts::PI {
                            diff += std::f32::consts::TAU;
                        }
                        p.render_rot = p.last_rot + diff * alpha;
                    }

                    if p.dying {
                        p.render_alpha -= 0.05;
                    }
                });
                game.players.retain(|p| p.render_alpha > 0.0);

                game.shapes.iter_mut().for_each(|s| {
                    let time_elapsed = current_time - s.last_update_time;
                    let alpha = (time_elapsed / 100.0).min(1.0) as f32;
                    s.render_pos = s.last_pos.lerp(s.pos, alpha as f32);
                    s.render_health += (s.health as f32 - s.render_health) * 0.1;

                    let mut diff = s.rot - s.last_rot;
                    if diff > std::f32::consts::PI {
                        diff -= std::f32::consts::TAU;
                    }
                    if diff < -std::f32::consts::PI {
                        diff += std::f32::consts::TAU;
                    }
                    s.render_rot = s.last_rot + diff * alpha;

                    if s.dying {
                        s.render_alpha -= 0.05;
                    }
                });
                game.shapes.retain(|s| s.render_alpha > 0.0);

                game.bullets.iter_mut().for_each(|b| {
                    let time_elapsed = current_time - b.last_update_time;
                    let alpha = (time_elapsed / 100.0).min(1.0) as f32;
                    b.render_pos = b.last_pos.lerp(b.pos, alpha as f32);

                    let mut diff = b.rot - b.last_rot;
                    if diff > std::f32::consts::PI {
                        diff -= std::f32::consts::TAU;
                    }
                    if diff < -std::f32::consts::PI {
                        diff += std::f32::consts::TAU;
                    }
                    b.render_rot = b.last_rot + diff * alpha;
                });

                let entities: Vec<RenderEntity> = game
                    .players
                    .iter()
                    .zip(labels.iter())
                    .flat_map(|(x, label)| {
                        let mut renders = Vec::new();
                        let instances = x.get_render_instances();
                        let body_idx = instances.len() - 1;
                        for (i, instance) in instances.iter().enumerate() {
                            renders.push(RenderEntity {
                                instance: *instance,
                                text: if i == body_idx { Some(label) } else { None },
                            });
                        }
                        renders
                    })
                    .collect();

                let shape_entities: Vec<RenderEntity> = game
                    .shapes
                    .iter()
                    .flat_map(|s| {
                        s.get_render_instances()
                            .iter()
                            .map(|inst| RenderEntity {
                                instance: *inst,
                                text: None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();

                let bullet_entities: Vec<RenderEntity> = game
                    .bullets
                    .iter()
                    .flat_map(|b| {
                        b.get_render_instances()
                            .iter()
                            .map(|inst| RenderEntity {
                                instance: *inst,
                                text: None,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                const MAP_BOUND: f32 = 2500.0;
                let border_color = DARK_THEME.maze_walls;

                let mut instances_to_draw: Vec<RenderEntity> = vec![
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

                instances_to_draw.extend(bullet_entities);
                instances_to_draw.extend(shape_entities.clone());
                instances_to_draw.extend(entities.clone());

                for p in game.players.iter() {
                    if p.dying {
                        continue;
                    }
                    let bar_width = 40.0 * p.scale;
                    let bar_height = 6.0 * p.scale;
                    let bar_y = p.render_pos.y - 30.0 * p.scale;

                    let health_percent = (p.render_health / p.max_health as f32).max(0.0).min(1.0);
                    let fg_width = bar_width * health_percent;
                    let fg_x = p.render_pos.x - bar_width * 0.5 + fg_width * 0.5;

                    instances_to_draw.push(RenderEntity {
                        instance: EntityInstance {
                            position: [p.render_pos.x, bar_y],
                            size: [bar_width, bar_height],
                            rotation: 0.0,
                            shape_type: 4,
                            sides: 4,
                            fill_color: DARK_THEME.health_bar_background,
                            border_color: [0.0, 0.0, 0.0, 0.0],
                            border_thickness: 0.0,
                            extra_param: 0.3,
                        },
                        text: None,
                    });

                    if fg_width > 0.1 {
                        instances_to_draw.push(RenderEntity {
                            instance: EntityInstance {
                                position: [fg_x, bar_y],
                                size: [fg_width, bar_height],
                                rotation: 0.0,
                                shape_type: 4,
                                sides: 4,
                                fill_color: DARK_THEME.health_bar_foreground,
                                border_color: [0.0, 0.0, 0.0, 0.0],
                                border_thickness: 0.0,
                                extra_param: 0.3,
                            },
                            text: None,
                        });
                    }
                }

                for s in game.shapes.iter() {
                    if s.dying {
                        continue;
                    }
                    let bar_width = s.size * 0.6;
                    let bar_height = 4.0;
                    let bar_y = s.render_pos.y - s.size * 0.5;

                    let health_percent = (s.render_health / s.max_health as f32).max(0.0).min(1.0);
                    let fg_width = bar_width * health_percent;
                    let fg_x = s.render_pos.x - bar_width * 0.5 + fg_width * 0.5;

                    instances_to_draw.push(RenderEntity {
                        instance: EntityInstance {
                            position: [s.render_pos.x, bar_y],
                            size: [bar_width, bar_height],
                            rotation: 0.0,
                            shape_type: 4,
                            sides: 4,
                            fill_color: DARK_THEME.health_bar_background,
                            border_color: [0.0, 0.0, 0.0, 0.0],
                            border_thickness: 0.0,
                            extra_param: 0.5,
                        },
                        text: None,
                    });

                    if fg_width > 0.1 {
                        instances_to_draw.push(RenderEntity {
                            instance: EntityInstance {
                                position: [fg_x, bar_y],
                                size: [fg_width, bar_height],
                                rotation: 0.0,
                                shape_type: 4,
                                sides: 4,
                                fill_color: DARK_THEME.health_bar_foreground,
                                border_color: [0.0, 0.0, 0.0, 0.0],
                                border_thickness: 0.0,
                                extra_param: 0.5,
                            },
                            text: None,
                        });
                    }
                }

                let screen_w = state.config.width as f32;
                let screen_h = state.config.height as f32;
                let aspect_ratio = screen_w / screen_h.max(1.0);
                let my_player_scale = game.my_player().map(|p| p.scale).unwrap_or(1.0);

                let target_zoom = 0.005 / my_player_scale;
                state.render_zoom += (target_zoom - state.render_zoom) * 0.1;
                let zoom = state.render_zoom;

                let camera_pos = game
                    .my_player()
                    .map(|p| p.render_pos.to_array())
                    .unwrap_or([0.0, 0.0]);

                let xp_bar_width_px = screen_w * 0.5;
                let xp_bar_height_px = 20.0;
                let xp_bar_x_px = screen_w * 0.5;
                let xp_bar_y_px = screen_h - 40.0;

                let xp_bar_world = Renderer::screen_to_world(
                    [xp_bar_x_px, xp_bar_y_px],
                    camera_pos,
                    zoom,
                    aspect_ratio,
                    screen_w,
                    screen_h,
                );
                let xp_bar_world_w = (xp_bar_width_px / (screen_w / 2.0)) * aspect_ratio / zoom;
                let xp_bar_world_h = (xp_bar_height_px / (screen_h / 2.0)) / zoom;

                instances_to_draw.push(RenderEntity {
                    instance: EntityInstance {
                        position: xp_bar_world,
                        size: [xp_bar_world_w, xp_bar_world_h],
                        rotation: 0.0,
                        shape_type: 4,
                        sides: 4,
                        fill_color: DARK_THEME.bar_background,
                        border_color: [0.0, 0.0, 0.0, 0.0],
                        border_thickness: 0.0,
                        extra_param: 0.5,
                    },
                    text: None,
                });

                let xp_percent = if game.xp_to_next > 0 {
                    game.xp as f32 / game.xp_to_next as f32
                } else {
                    0.0
                };
                let fg_w = xp_bar_world_w * xp_percent;
                let fg_x = xp_bar_world[0] - xp_bar_world_w * 0.5 + fg_w * 0.5;

                if fg_w > 0.0 {
                    instances_to_draw.push(RenderEntity {
                        instance: EntityInstance {
                            position: [fg_x, xp_bar_world[1]],
                            size: [fg_w, xp_bar_world_h],
                            rotation: 0.0,
                            shape_type: 4,
                            sides: 4,
                            fill_color: DARK_THEME.xp_bar_fill,
                            border_color: [0.0, 0.0, 0.0, 0.0],
                            border_thickness: 0.0,
                            extra_param: 0.5,
                        },
                        text: None,
                    });
                }

                let stats_text = format!(
                    "Lvl {} | XP: {}/{} | HP: {}/{}",
                    game.level, game.xp, game.xp_to_next, game.health, game.max_health
                );

                let mut lb_text = String::from("Leaderboard\n");
                for (i, (name, xp)) in game.leaderboard.iter().enumerate() {
                    let xp_str = if *xp >= 1000 {
                        format!("{:.1}k", *xp as f32 / 1000.0)
                    } else {
                        format!("{}", xp)
                    };
                    lb_text.push_str(&format!("{}. {} - {}\n", i + 1, name, xp_str));
                }

                drop(game);

                state.render_entities_with_text(
                    &instances_to_draw,
                    camera_pos,
                    zoom,
                    &stats_text,
                    &lb_text,
                );
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: key_state,
                        ..
                    },
                ..
            } => {
                let pressed = key_state.is_pressed();
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
    }
}
