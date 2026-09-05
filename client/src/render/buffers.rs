use bytemuck::{Pod, Zeroable};
use glyphon::{Attrs, Color, FontSystem, Metrics};
use wgpu::{BufferAddress, VertexAttribute, VertexBufferLayout, VertexStepMode};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
}

impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct EntityInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub rotation: f32,

    /// 0 = Circle
    /// 1 = Box
    /// 2 = Grid
    /// 3 = Polygon
    /// 4 = Rounded box (filled only)
    /// 5 = screen pill
    /// 6 = screen circle
    pub shape_type: u32,

    /// 3 = Triangle
    /// 4 = Square
    /// 5 = Pentagon
    pub sides: u32,
    pub fill_color: [f32; 4],
    pub border_color: [f32; 4],
    pub border_thickness: f32,
    pub extra_param: f32,
}

impl EntityInstance {
    pub fn desc() -> VertexBufferLayout<'static> {
        const ATTRIBS: [VertexAttribute; 9] = wgpu::vertex_attr_array![
            0 => Float32x2, // pos
            1 => Float32x2, // size
            2 => Float32,   // rotation
            3 => Uint32,    // shape_type
            4 => Uint32,    // sides
            5 => Float32x4, // fill_color
            6 => Float32x4, // border_color
            7 => Float32,   // border_thickness
            8 => Float32,   // extra_param
        ];

        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as BufferAddress,
            step_mode: VertexStepMode::Instance,
            attributes: &ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 2],
    pub zoom: f32,
    pub aspect_ratio: f32,
    pub screen_size: [f32; 2],
    pub _pad: [f32; 2],
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            view_proj: [[0.0; 4]; 4],
            camera_pos: [0.0; 2],
            zoom: 1.0,
            aspect_ratio: 1.0,
            screen_size: [1.0, 1.0],
            _pad: [0.0; 2],
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
        let mut buffer = glyphon::Buffer::new(font_system, Metrics::new(16.0, 20.0));

        buffer.set_text(
            initial_text,
            &Attrs::new().family(glyphon::Family::SansSerif),
            glyphon::Shaping::Basic,
            None,
        );
        buffer.shape_until_scroll(font_system, false);

        Self {
            buffer,
            color: Color::rgb(0, 0, 0),
            offset: [0.0, -30.0],
        }
    }

    pub fn update_text(&mut self, font_system: &mut FontSystem, new_text: &str) {
        self.buffer.set_text(
            new_text,
            &Attrs::new().family(glyphon::Family::SansSerif),
            glyphon::Shaping::Basic,
            None,
        );

        self.buffer.shape_until_scroll(font_system, false);
    }
}
