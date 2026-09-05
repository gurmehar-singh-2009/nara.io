struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec2<f32>,
    zoom: f32,
    aspect_ratio: f32,
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct InstanceInput {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) rotation: f32,
    @location(3) shape_type: u32,
    @location(4) sides: u32,
    @location(5) fill_color: vec4<f32>,
    @location(6) border_color: vec4<f32>,
    @location(7) border_thickness: f32,
    @location(8) extra_param: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) world_pos: vec2<f32>,
    @location(2) @interpolate(flat) shape_type: u32,
    @location(3) @interpolate(flat) sides: u32,
    @location(4) @interpolate(flat) fill_color: vec4<f32>,
    @location(5) @interpolate(flat) border_color: vec4<f32>,
    @location(6) @interpolate(flat) border_thickness: f32,
    @location(7) @interpolate(flat) extra_param: f32,
    @location(8) @interpolate(flat) size: vec2<f32>,
};

const QUAD_VERTICES: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>( 1.0,  1.0)
);

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    let local_position = QUAD_VERTICES[vertex_index];

    if (instance.shape_type == 5u || instance.shape_type == 6u || instance.shape_type == 7u) {
        let ndc = instance.pos + local_position * (instance.size * 0.5);
        out.clip_position = vec4<f32>(ndc, 0.0, 1.0);
        out.world_pos = instance.pos;
    } else {
        let cos_r = cos(instance.rotation);
        let sin_r = sin(instance.rotation);

        let rot_mat = mat2x2<f32>(cos_r, sin_r, -sin_r, cos_r);

        let local_scaled = local_position * (instance.size * 0.5);
        let rotated_pos = rot_mat * local_scaled;
        let world_pos = instance.pos + rotated_pos;

        let rel_pos = (world_pos - camera.camera_pos) * camera.zoom;
        out.clip_position = vec4<f32>(rel_pos.x / camera.aspect_ratio, rel_pos.y, 0.0, 1.0);
        out.world_pos = world_pos;
    }

    out.uv = local_position;
    out.shape_type = instance.shape_type;
    out.sides = instance.sides;
    out.fill_color = instance.fill_color;
    out.border_color = instance.border_color;
    out.border_thickness = instance.border_thickness;
    out.extra_param = instance.extra_param;
    out.size = instance.size;

    return out;
}
