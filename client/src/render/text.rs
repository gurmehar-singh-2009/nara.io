//! SDF text rendering.
//!
//! cosmic-text still does shaping and layout. Each glyph is rasterised once at
//! `BASE_SIZE`, turned into a signed distance field and packed into a
//! single-channel atlas. Every font size and the outline are drawn from that
//! one entry, in a single quad per glyph.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use cosmic_text::{
    Buffer, CacheKey, CacheKeyFlags, FontSystem, LayoutGlyph, SwashCache, SwashContent,
};
use wgpu::{
    BindGroup, BindGroupLayout, BufferUsages, Device, Queue, RenderPass, RenderPipeline,
    TextureFormat, util::DeviceExt,
};

use crate::render::colours::Color;

/// Size glyphs are rasterised at before the distance transform.
const BASE_SIZE: f32 = 48.0;
/// Distance range (in atlas texels) the 0..1 texel value spans. Must match
/// text.wgsl.
const RANGE: f32 = 16.0;
/// Texel value of the glyph edge. Must match text.wgsl.
const EDGE: f32 = 0.75;
/// Empty border around each glyph. Covers the whole outward range (`EDGE *
/// RANGE` texels), which is a quarter of an em: enough for the thickest outline
/// we draw.
const PAD: u32 = 12;
/// Spacing between atlas cells so bilinear sampling never bleeds into a
/// neighbour.
const GAP: u32 = 1;

const ATLAS_WIDTH: u32 = 1024;
const ATLAS_START_HEIGHT: u32 = 512;

/// Far-away distance used to seed the distance transform.
const FAR: f64 = 1e20;

#[derive(Clone, Copy, Debug)]
pub struct TextBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// One buffer to draw. Replaces glyphon's `TextArea`, with the outline built
/// in.
#[derive(Clone, Copy)]
pub struct TextArea<'a> {
    pub buffer: &'a Buffer,
    pub left: f32,
    pub top: f32,
    pub scale: f32,
    pub bounds: TextBounds,
    pub color: Color,
    pub outline: Color,
    /// Outline width in screen pixels. 0 draws plain text.
    pub outline_px: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum Layer {
    /// Drawn between world entities and UI entities.
    World = 0,
    /// Drawn on top of everything.
    Ui = 1,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphInstance {
    rect: [f32; 4],
    uv: [f32; 4],
    fill: [f32; 4],
    outline: [f32; 4],
    clip: [f32; 4],
    params: [f32; 2],
}

impl GlyphInstance {
    const ATTRIBS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x2,
    ];

    const fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, PartialEq)]
struct Globals {
    sizes: [f32; 4],
}

/// Where a glyph lives in the atlas and how it sits relative to the pen
/// position.
#[derive(Clone, Copy)]
struct GlyphEntry {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    /// Offset of the cell's top-left from the pen, in `BASE_SIZE` pixels (y
    /// down).
    left: f32,
    top: f32,
}

/// The atlas ran out of room and can't grow any further.
struct AtlasFull;

struct Atlas {
    width: u32,
    height: u32,
    max_height: u32,
    pixels: Vec<u8>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// Rows that changed since the last upload.
    dirty: Option<(u32, u32)>,
    /// Texture must be recreated (it grew).
    resized: bool,
}

impl Atlas {
    fn new(max_dim: u32) -> Self {
        let height = ATLAS_START_HEIGHT.min(max_dim);

        Self {
            width: ATLAS_WIDTH.min(max_dim),
            height,
            max_height: max_dim,
            pixels: vec![0; (ATLAS_WIDTH.min(max_dim) * height) as usize],
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            dirty: None,
            resized: false,
        }
    }

    fn clear(&mut self) {
        self.pixels.fill(0);
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.shelf_h = 0;
        self.dirty = Some((0, self.height));
    }

    fn alloc(&mut self, w: u32, h: u32) -> Result<(u32, u32), AtlasFull> {
        let (cw, ch) = (w + GAP, h + GAP);

        if cw > self.width {
            return Err(AtlasFull);
        }

        if self.shelf_x + cw > self.width {
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }

        while self.shelf_y + ch > self.height {
            if self.height >= self.max_height {
                return Err(AtlasFull);
            }

            self.height = (self.height * 2).min(self.max_height);
            self.pixels.resize((self.width * self.height) as usize, 0);
            self.resized = true;
        }

        let pos = (self.shelf_x, self.shelf_y);

        self.shelf_x += cw;
        self.shelf_h = self.shelf_h.max(ch);

        Ok(pos)
    }

    fn blit(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        for row in 0..h {
            let dst = ((y + row) * self.width + x) as usize;
            let src = (row * w) as usize;

            self.pixels[dst..dst + w as usize].copy_from_slice(&data[src..src + w as usize]);
        }

        self.dirty = Some(match self.dirty {
            Some((a, b)) => (a.min(y), b.max(y + h)),
            None => (y, y + h),
        });
    }
}

pub struct TextRenderer {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
    bind_group: BindGroup,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    globals_buffer: wgpu::Buffer,
    globals: Globals,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instances: Vec<GlyphInstance>,
    ranges: [std::ops::Range<u32>; 2],

    atlas: Atlas,
    glyphs: HashMap<CacheKey, Option<GlyphEntry>>,
    swash: SwashCache,
    sdf: SdfScratch,
}

impl TextRenderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let atlas = Atlas::new(device.limits().max_texture_dimension_2d);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Text Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Text Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let globals = Globals {
            sizes: [1.0, 1.0, atlas.width as f32, atlas.height as f32],
        };

        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text Globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let texture = Self::create_texture(device, &atlas);
        let bind_group = Self::create_bind_group(
            device,
            &bind_group_layout,
            &globals_buffer,
            &texture,
            &sampler,
        );

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Text Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./shader/text.wgsl").into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Text Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Text Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(GlyphInstance::desc())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instance_capacity = 1024;
        let instance_buffer = Self::create_instance_buffer(device, instance_capacity);

        Self {
            pipeline,
            bind_group_layout,
            bind_group,
            sampler,
            texture,
            globals_buffer,
            globals,
            instance_buffer,
            instance_capacity,
            instances: Vec::with_capacity(1024),
            ranges: [0..0, 0..0],
            atlas,
            glyphs: HashMap::new(),
            swash: SwashCache::new(),
            sdf: SdfScratch::default(),
        }
    }

    fn create_texture(device: &Device, atlas: &Atlas) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Text SDF Atlas"),
            size: wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn create_bind_group(
        device: &Device,
        layout: &BindGroupLayout,
        globals: &wgpu::Buffer,
        texture: &wgpu::Texture,
        sampler: &wgpu::Sampler,
    ) -> BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Text Bind Group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    fn create_instance_buffer(device: &Device, capacity: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Text Instances"),
            size: capacity * std::mem::size_of::<GlyphInstance>() as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Lays out both layers and uploads everything the GPU needs for this
    /// frame.
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        font_system: &mut FontSystem,
        screen: (u32, u32),
        world: &[TextArea<'_>],
        ui: &[TextArea<'_>],
    ) {
        // If the atlas fills up at its max size, start it over and lay out once
        // more.
        for attempt in 0..2 {
            self.instances.clear();

            let built = self.push_layer(font_system, world).and_then(|()| {
                let split = self.instances.len() as u32;
                self.push_layer(font_system, ui).map(|()| split)
            });

            match built {
                Ok(split) => {
                    self.ranges = [0..split, split..self.instances.len() as u32];
                    break;
                }
                Err(AtlasFull) => {
                    self.ranges = [0..0, 0..0];

                    if attempt == 0 {
                        log::warn!("text atlas full, rebuilding");
                        self.glyphs.clear();
                        self.atlas.clear();
                    }
                }
            }
        }

        if self.atlas.resized {
            self.atlas.resized = false;
            self.atlas.dirty = Some((0, self.atlas.height));
            self.texture = Self::create_texture(device, &self.atlas);
            self.bind_group = Self::create_bind_group(
                device,
                &self.bind_group_layout,
                &self.globals_buffer,
                &self.texture,
                &self.sampler,
            );
        }

        if let Some((y0, y1)) = self.atlas.dirty.take() {
            let w = self.atlas.width;

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &self.atlas.pixels[(y0 * w) as usize..(y1 * w) as usize],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w),
                    rows_per_image: Some(y1 - y0),
                },
                wgpu::Extent3d {
                    width: w,
                    height: y1 - y0,
                    depth_or_array_layers: 1,
                },
            );
        }

        let globals = Globals {
            sizes: [
                screen.0.max(1) as f32,
                screen.1.max(1) as f32,
                self.atlas.width as f32,
                self.atlas.height as f32,
            ],
        };

        if globals != self.globals {
            self.globals = globals;
            queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        }

        if self.instances.is_empty() {
            return;
        }

        let needed = self.instances.len() as u64;

        if needed > self.instance_capacity {
            self.instance_capacity = needed.next_power_of_two();
            self.instance_buffer = Self::create_instance_buffer(device, self.instance_capacity);
        }

        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );
    }

    pub fn render(&self, pass: &mut RenderPass<'_>, layer: Layer) {
        let range = self.ranges[layer as usize].clone();

        if range.is_empty() {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..4, range);
    }

    fn push_layer(
        &mut self,
        font_system: &mut FontSystem,
        areas: &[TextArea<'_>],
    ) -> Result<(), AtlasFull> {
        for area in areas {
            self.push_area(font_system, area)?;
        }

        Ok(())
    }

    fn push_area(
        &mut self,
        font_system: &mut FontSystem,
        area: &TextArea<'_>,
    ) -> Result<(), AtlasFull> {
        let b = area.bounds;

        if b.left >= b.right || b.top >= b.bottom || area.scale <= 0.0 {
            return Ok(());
        }

        if area.color[3] <= 0.0 && (area.outline[3] <= 0.0 || area.outline_px <= 0.0) {
            return Ok(());
        }

        let clip = [b.left as f32, b.top as f32, b.right as f32, b.bottom as f32];
        let scale = area.scale;
        let fill = linear(area.color);
        let outline = linear(area.outline);
        let outline_px = area.outline_px.max(0.0);

        for run in area.buffer.layout_runs() {
            let line_top = area.top + run.line_top * scale;

            if line_top > clip[3] {
                break;
            }

            if line_top + run.line_height * scale < clip[1] {
                continue;
            }

            let baseline = (area.top + run.line_y * scale).round();

            for glyph in run.glyphs {
                let Some(entry) = self.glyph(font_system, glyph)? else {
                    continue;
                };

                let k = glyph.font_size * scale / BASE_SIZE;
                let pen_x = area.left + glyph.font_size.mul_add(glyph.x_offset, glyph.x) * scale;
                let pen_y = baseline + glyph.font_size.mul_add(-glyph.y_offset, glyph.y) * scale;

                let rect = [
                    entry.left.mul_add(k, pen_x),
                    entry.top.mul_add(k, pen_y),
                    entry.w as f32 * k,
                    entry.h as f32 * k,
                ];

                if rect[0] > clip[2]
                    || rect[1] > clip[3]
                    || rect[0] + rect[2] < clip[0]
                    || rect[1] + rect[3] < clip[1]
                {
                    continue;
                }

                let fill = glyph.color_opt.map_or(fill, |c| {
                    linear([
                        f32::from(c.r()) / 255.0,
                        f32::from(c.g()) / 255.0,
                        f32::from(c.b()) / 255.0,
                        f32::from(c.a()) / 255.0 * area.color[3],
                    ])
                });

                self.instances.push(GlyphInstance {
                    rect,
                    uv: [
                        entry.x as f32,
                        entry.y as f32,
                        entry.w as f32,
                        entry.h as f32,
                    ],
                    fill,
                    outline,
                    clip,
                    params: [outline_px, k],
                });
            }
        }

        Ok(())
    }

    /// Looks a glyph up in the atlas, building its SDF the first time it's
    /// seen.
    fn glyph(
        &mut self,
        font_system: &mut FontSystem,
        glyph: &LayoutGlyph,
    ) -> Result<Option<GlyphEntry>, AtlasFull> {
        // One entry per glyph shape, whatever size it's drawn at.
        let (key, _, _) = CacheKey::new(
            glyph.font_id,
            glyph.glyph_id,
            BASE_SIZE,
            (0.0, 0.0),
            glyph.font_weight,
            glyph.cache_key_flags | CacheKeyFlags::DISABLE_HINTING,
        );

        if let Some(entry) = self.glyphs.get(&key) {
            return Ok(*entry);
        }

        let Some(image) = self.swash.get_image_uncached(font_system, key) else {
            self.glyphs.insert(key, None);
            return Ok(None);
        };

        let p = image.placement;

        // Colour glyphs (emoji) have no outline to build a distance field from.
        if !matches!(image.content, SwashContent::Mask) || p.width == 0 || p.height == 0 {
            self.glyphs.insert(key, None);
            return Ok(None);
        }

        let (w, h) = (p.width + PAD * 2, p.height + PAD * 2);
        let (x, y) = self.atlas.alloc(w, h)?;

        let sdf = self
            .sdf
            .build(&image.data, p.width as usize, p.height as usize);
        self.atlas.blit(x, y, w, h, sdf);

        let entry = GlyphEntry {
            x,
            y,
            w,
            h,
            left: p.left as f32 - PAD as f32,
            top: -(p.top as f32) - PAD as f32,
        };

        self.glyphs.insert(key, Some(entry));

        Ok(Some(entry))
    }
}

/// glyphon treated text colours as sRGB and converted them to linear before
/// blending. Doing the same keeps every label the exact colour it was before.
fn linear(c: Color) -> [f32; 4] {
    fn ch(v: f32) -> f32 {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }

    [ch(c[0]), ch(c[1]), ch(c[2]), c[3].clamp(0.0, 1.0)]
}

/// Reusable buffers for the distance transform (Felzenszwalb & Huttenlocher, as
/// in TinySDF).
#[derive(Default)]
struct SdfScratch {
    outer: Vec<f64>,
    inner: Vec<f64>,
    f: Vec<f64>,
    z: Vec<f64>,
    v: Vec<usize>,
    out: Vec<u8>,
}

impl SdfScratch {
    /// Turns a `w`x`h` coverage mask into a padded distance field.
    fn build(&mut self, alpha: &[u8], w: usize, h: usize) -> &[u8] {
        let pad = PAD as usize;
        let (gw, gh) = (w + pad * 2, h + pad * 2);
        let n = gw * gh;
        let longest = gw.max(gh);

        self.outer.clear();
        self.outer.resize(n, FAR);
        self.inner.clear();
        self.inner.resize(n, 0.0);
        self.f.resize(longest, 0.0);
        self.z.resize(longest + 1, 0.0);
        self.v.resize(longest, 0);

        for y in 0..h {
            for x in 0..w {
                let a = f64::from(alpha[y * w + x]) / 255.0;

                if a <= 0.0 {
                    continue;
                }

                let i = (y + pad) * gw + x + pad;

                if a >= 1.0 {
                    self.outer[i] = 0.0;
                    self.inner[i] = FAR;
                } else {
                    // edge pixels: estimate the sub-pixel distance from
                    // coverage
                    let d = 0.5 - a;
                    self.outer[i] = if d > 0.0 { d * d } else { 0.0 };
                    self.inner[i] = if d < 0.0 { d * d } else { 0.0 };
                }
            }
        }

        edt(
            &mut self.outer,
            gw,
            gh,
            &mut self.f,
            &mut self.v,
            &mut self.z,
        );
        edt(
            &mut self.inner,
            gw,
            gh,
            &mut self.f,
            &mut self.v,
            &mut self.z,
        );

        self.out.clear();
        self.out
            .extend(self.outer.iter().zip(&self.inner).map(|(o, i)| {
                let d = (o.sqrt() - i.sqrt()) as f32;
                ((EDGE - d / RANGE).clamp(0.0, 1.0) * 255.0).round() as u8
            }));

        &self.out
    }
}

/// 2D squared Euclidean distance transform, in place.
fn edt(grid: &mut [f64], w: usize, h: usize, f: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    for x in 0..w {
        edt_1d(grid, x, w, h, f, v, z);
    }

    for y in 0..h {
        edt_1d(grid, y * w, 1, w, f, v, z);
    }
}

fn edt_1d(
    grid: &mut [f64],
    offset: usize,
    stride: usize,
    len: usize,
    f: &mut [f64],
    v: &mut [usize],
    z: &mut [f64],
) {
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    f[0] = grid[offset];

    let mut k = 0;

    for q in 1..len {
        f[q] = grid[offset + q * stride];

        let qf = q as f64;

        // z[0] is -inf, so this always stops at k == 0
        let s = loop {
            let r = v[k];
            let rf = r as f64;
            let s = (f[q] - f[r] + qf.mul_add(qf, -rf * rf)) / (qf - rf) / 2.0;

            if s <= z[k] {
                k -= 1;
            } else {
                break s;
            }
        };

        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }

    k = 0;

    for q in 0..len {
        while z[k + 1] < q as f64 {
            k += 1;
        }

        let r = v[k];
        let d = q as f64 - r as f64;

        grid[offset + q * stride] = d.mul_add(d, f[r]);
    }
}
