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

const PI: f32 = 3.14159265359;

fn draw_grid(
    world_pos: vec2<f32>,
    cell_size: f32,
    line_width: f32,
    bg_color: vec4<f32>,
    line_color: vec4<f32>,
    line_alpha: f32,
    line_aa: f32,
) -> vec4<f32> {
    let grid_coord = abs(fract(world_pos / cell_size - 0.5) - 0.5) * cell_size;

    let half_width = line_width * 0.5;
    let aa = line_aa;

    let factor_x = 1.0 - smoothstep(half_width - aa, half_width + aa, grid_coord.x);
    let factor_y = 1.0 - smoothstep(half_width - aa, half_width + aa, grid_coord.y);

    let line_factor = max(factor_x, factor_y);

    let factor = line_factor * line_alpha;

    return vec4<f32>(
        mix(bg_color.rgb, line_color.rgb, factor),
        1.0,
    );
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv_fwidth = fwidth(in.uv);
    let delta = max(uv_fwidth.x, uv_fwidth.y);

    let world_fwidth = fwidth(in.world_pos);
    let grid_line_aa = max(world_fwidth.x, world_fwidth.y);

    let half_px = in.size * (camera.screen_size * 0.25);
    let p = in.uv * half_px;
    let p_fwidth = fwidth(p);
    let ui_aa = max(p_fwidth.x, p_fwidth.y);

    if (in.shape_type == 5u || in.shape_type == 6u || in.shape_type == 7u) {
        var dist: f32;
        if (in.shape_type == 5u) {
            let r = min(half_px.x, half_px.y);
            let q = abs(p) - (half_px - vec2<f32>(r, r));
            dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
        } else if (in.shape_type == 6u) {
            dist = length(p) - min(half_px.x, half_px.y);
        } else {
            let r = clamp(in.extra_param, 0.0, min(half_px.x, half_px.y));
            let q = abs(p) - (half_px - vec2<f32>(r, r));
            dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
        }

        let alpha = 1.0 - smoothstep(-ui_aa, ui_aa, dist);
        if (alpha < 0.001) {
            discard;
        }

        let t = min(in.border_thickness, min(half_px.x, half_px.y));
        let border_aa = max(min(ui_aa, t) * 0.5, 1e-5);
        let border_mix = smoothstep(-t - border_aa, -t + border_aa, dist);

        let final_color = mix(in.fill_color, in.border_color, border_mix);
        return vec4<f32>(final_color.rgb, final_color.a * alpha);
    }

    if (in.shape_type == 2u) {
        let cell_size = in.extra_param;
        return draw_grid(
            in.world_pos,
            cell_size,
            in.border_thickness,
            in.fill_color,
            in.border_color,
            in.border_color.a,
            grid_line_aa,
        );
    }

    let min_size = min(in.size.x, in.size.y);
    let border_uv_width = in.border_thickness * (2.0 / min_size);

    if (in.shape_type == 0u) {
        let dist_circle = length(in.uv);
        let alpha = 1.0 - smoothstep(1.0 - delta, 1.0 + delta, dist_circle);
        if (alpha < 0.001) {
            discard;
        }

        // crisp
        let border_aa = max(min(delta, border_uv_width) * 0.5, 1e-5);
        let border_mix = smoothstep(1.0 - border_uv_width - border_aa, 1.0 - border_uv_width + border_aa, dist_circle);

        let final_color = mix(in.fill_color, in.border_color, border_mix);
        return vec4<f32>(final_color.rgb, final_color.a * alpha);
    }

    if (in.shape_type == 1u) {
        let half = in.size * 0.5;
        let p_w = in.uv * half;
        let dist_box = max(abs(p_w.x) - half.x, abs(p_w.y) - half.y);

        let delta_w = max(uv_fwidth.x * half.x, uv_fwidth.y * half.y);
        let border_w = min(in.border_thickness, min(half.x, half.y));

        let alpha = 1.0 - smoothstep(-delta_w, delta_w, dist_box);
        if (alpha < 0.001) {
            discard;
        }

        // crisp
        let border_aa = max(min(delta_w, border_w) * 0.5, 1e-5);
        let border_mix = smoothstep(-border_w - border_aa, -border_w + border_aa, dist_box);

        let final_color = mix(in.fill_color, in.border_color, border_mix);
        return vec4<f32>(final_color.rgb, final_color.a * alpha);
    }

    if (in.shape_type == 4u) {
        let half = in.size * 0.5;
        let p_w = in.uv * half;
        let r = min(in.extra_param, 1.0) * min(half.x, half.y);
        let q = abs(p_w) - (half - vec2<f32>(r, r));
        let dist_rounded = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;

        let delta_w = max(uv_fwidth.x * half.x, uv_fwidth.y * half.y);

        let alpha = 1.0 - smoothstep(-delta_w, delta_w, dist_rounded);
        if (alpha < 0.001) {
            discard;
        }

        let border_w = min(in.border_thickness, min(half.x, half.y));
        // crisp
        let border_aa = max(min(delta_w, border_w) * 0.5, 1e-5);
        // let border_mix = smoothstep(-border_w - border_aa, -border_w + border_aa, dist_rounded);

        let final_color = mix(in.fill_color, in.border_color, in.border_color);
        return vec4<f32>(final_color.rgb, final_color.a * alpha);
    }

    if (in.shape_type == 3u && in.sides >= 3u) {
        let sides_f = f32(in.sides);
        let angle = atan2(in.uv.y, in.uv.x);
        let slice = (2.0 * PI) / sides_f;

        let apothem = cos(PI / sides_f);
        let dist_poly = (cos(floor(0.5 + angle / slice) * slice - angle) * length(in.uv)) / apothem;

        let alpha = 1.0 - smoothstep(1.0 - delta, 1.0 + delta, dist_poly);
        if (alpha < 0.001) {
            discard;
        }

        // crisp
        let border_aa = max(min(delta, border_uv_width) * 0.5, 1e-5);
        let border_mix = smoothstep(1.0 - border_uv_width - border_aa, 1.0 - border_uv_width + border_aa, dist_poly);

        let final_color = mix(in.fill_color, in.border_color, border_mix);
        return vec4<f32>(final_color.rgb, final_color.a * alpha);
    }

    return in.fill_color;
}
