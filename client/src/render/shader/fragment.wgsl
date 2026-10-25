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
const HALF_PI: f32 = 1.57079632679;
const TAU: f32 = 6.28318530718;
const INV_TAU: f32 = 0.15915494309;
const MIN_ALPHA: f32 = 0.001;
const MIN_AA: f32 = 2e-5; // floor for AA widths so reciprocals stay finite

// Shape type ids
const SHAPE_CIRCLE: u32 = 0u;
const SHAPE_BOX: u32 = 1u;
const SHAPE_GRID: u32 = 2u;
const SHAPE_POLYGON: u32 = 3u;
const SHAPE_ROUNDED: u32 = 4u;
const SHAPE_UI_PILL: u32 = 5u;
const SHAPE_UI_CIRCLE: u32 = 6u;
const SHAPE_UI_ROUNDED: u32 = 7u;
const SHAPE_TRAPEZOID: u32 = 8u;

// ---------------------------------------------------------------------------
// Cheap math helpers
// ---------------------------------------------------------------------------

// Hermite curve, same shape as smoothstep() but takes an already-normalised t.
fn hermite(t: f32) -> f32 {
    return t * t * (3.0 - 2.0 * t);
}

// cos(x) for |x| <= ~1.1 rad (the polygon sector range, max pi/3).
// Taylor to x^8, error < 5e-7 over that range. No range reduction needed.
fn cos_small(x: f32) -> f32 {
    let u = x * x;
    return 1.0 + u * (-0.5 + u * (0.0416666667 + u * (-0.00138888889 + u * 0.0000248015873)));
}

// atan2 with one divide and a 5-term polynomial (Abramowitz & Stegun 4.4.49),
// max error ~1e-5 rad. Branch-free.
fn fast_atan2(y: f32, x: f32) -> f32 {
    let ax = abs(x);
    let ay = abs(y);
    let a = min(ax, ay) / max(max(ax, ay), 1e-20);
    let s = a * a;
    var r = (((0.0208351 * s - 0.0851330) * s + 0.1801410) * s - 0.3302995) * s + 0.9998660;
    r = r * a;
    r = select(r, HALF_PI - r, ay > ax);
    r = select(r, PI - r, x < 0.0);
    return select(r, -r, y < 0.0);
}

// ---------------------------------------------------------------------------
// Shared coverage + crisp border path
// ---------------------------------------------------------------------------
// `dist`: signed distance, edge at 0, negative inside.
// `aa`: AA half-width, `thickness`: border width (same units as dist).
// Math is identical to the smoothstep version, but the divides are done
// once and shared: in the common case (thickness >= aa) the border smoothstep
// has exactly twice the slope of the edge one, so it reuses the same
// reciprocal. That's 1 reciprocal per pixel instead of 2.
fn shade(
    dist: f32,
    aa: f32,
    thickness: f32,
    fill: vec4<f32>,
    border: vec4<f32>,
) -> vec4<f32> {
    let w = max(aa, MIN_AA);
    let inv_w = 1.0 / w;

    // alpha = 1 - smoothstep(-aa, aa, dist)
    let alpha = 1.0 - hermite(saturate(dist * (0.5 * inv_w) + 0.5));
    if (alpha < MIN_ALPHA) {
        discard;
    }

    // border_mix = smoothstep(-t - baa, -t + baa, dist), baa = max(min(aa, t) * 0.5, 1e-5)
    var inv_b = inv_w;
    if (thickness < w) {
        inv_b = 1.0 / max(thickness, MIN_AA);
    }
    let border_mix = hermite(saturate((dist + thickness) * inv_b + 0.5));

    let c = mix(fill, border, border_mix);
    return vec4<f32>(c.rgb, c.a * alpha);
}

// Signed distance to a rounded box.
fn sd_round_box(p: vec2<f32>, half_ext: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - (half_ext - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn dot2(v: vec2<f32>) -> f32 {
    return dot(v, v);
}

fn sd_trapezoid(p_in: vec2<f32>, r1: f32, r2: f32, he: f32) -> f32 {
    let k1 = vec2<f32>(r2, he);
    let k2 = vec2<f32>(r2 - r1, 2.0 * he);
    let p = vec2<f32>(abs(p_in.x), p_in.y);
    let ca = vec2<f32>(p.x - min(p.x, select(r2, r1, p.y < 0.0)), abs(p.y) - he);
    let cb = p - k1 + k2 * clamp(dot(k1 - p, k2) / dot2(k2), 0.0, 1.0);
    let s = select(1.0, -1.0, cb.x < 0.0 && ca.y < 0.0);
    return s * sqrt(min(dot2(ca), dot2(cb)));
}

// max(fwidth(uv * extent)) without extra derivative instructions.
fn aa_width(uv_fw: vec2<f32>, extent: vec2<f32>) -> f32 {
    return max(uv_fw.x * extent.x, uv_fw.y * extent.y);
}

fn draw_grid(
    world_pos: vec2<f32>,
    cell_size: f32,
    line_width: f32,
    bg_color: vec4<f32>,
    line_color: vec4<f32>,
    line_alpha: f32,
    line_aa: f32,
) -> vec4<f32> {
    let grid_coord = abs(fract(world_pos * (1.0 / cell_size) - 0.5) - 0.5) * cell_size;

    let aa = max(line_aa, MIN_AA);
    let e0 = line_width * 0.5 - aa;

    // 1 - smoothstep(hw - aa, hw + aa, x) on both axes, one shared reciprocal
    let t = saturate((grid_coord - vec2<f32>(e0)) * (0.5 / aa));
    let f = 1.0 - t * t * (3.0 - 2.0 * t);

    let factor = max(f.x, f.y) * line_alpha;

    return vec4<f32>(mix(bg_color.rgb, line_color.rgb, factor), 1.0);
}

// ---------------------------------------------------------------------------
// Fragment
// ---------------------------------------------------------------------------

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Derivatives must stay in uniform control flow, so compute the two
    // that are needed once, up front, and derive everything else from them.
    let uv_fw = fwidth(in.uv);
    let world_fw = fwidth(in.world_pos);

    let st = in.shape_type;

    // ---- UI shapes (pixel-space SDFs) -----------------------------------
    if (st == SHAPE_UI_PILL || st == SHAPE_UI_CIRCLE || st == SHAPE_UI_ROUNDED) {
        let half_px = in.size * (camera.screen_size * 0.25);
        let m = min(half_px.x, half_px.y);
        let p = in.uv * half_px;
        let ui_aa = aa_width(uv_fw, half_px);

        var dist: f32;
        if (st == SHAPE_UI_CIRCLE) {
            dist = length(p) - m;
        } else {
            let r = select(clamp(in.extra_param, 0.0, m), m, st == SHAPE_UI_PILL);
            dist = sd_round_box(p, half_px, r);
        }

        return shade(dist, ui_aa, min(in.border_thickness, m), in.fill_color, in.border_color);
    }

    // ---- Grid -------------------------------------------------------------
    if (st == SHAPE_GRID) {
        return draw_grid(
            in.world_pos,
            in.extra_param,
            in.border_thickness,
            in.fill_color,
            in.border_color,
            in.border_color.a,
            max(world_fw.x, world_fw.y),
        );
    }

    // ---- Circle -----------------------------------------------------------
    if (st == SHAPE_CIRCLE) {
        let delta = max(uv_fw.x, uv_fw.y);
        let border_uv_width = in.border_thickness * (2.0 / min(in.size.x, in.size.y));
        return shade(length(in.uv) - 1.0, delta, border_uv_width, in.fill_color, in.border_color);
    }

    // ---- Box (Chebyshev distance: no sqrt) --------------------------------
    if (st == SHAPE_BOX) {
        let half_size = in.size * 0.5;
        let p = in.uv * half_size;
        let dist = max(abs(p.x) - half_size.x, abs(p.y) - half_size.y);
        let border_w = min(in.border_thickness, min(half_size.x, half_size.y));
        return shade(dist, aa_width(uv_fw, half_size), border_w, in.fill_color, in.border_color);
    }

    // ---- Rounded box ------------------------------------------------------
    if (st == SHAPE_ROUNDED) {
        let half_size = in.size * 0.5;
        let p = in.uv * half_size;
        let r = min(in.extra_param, 1.0) * min(half_size.x, half_size.y);
        let dist = sd_round_box(p, half_size, r);
        let border_w = min(in.border_thickness, min(half_size.x, half_size.y));
        return shade(dist, aa_width(uv_fw, half_size), border_w, in.fill_color, in.border_color);
    }

    if (st == SHAPE_TRAPEZOID) {
        let half_size = in.size * 0.5;
        let p = in.uv * half_size;
        let r1 = half_size.x;
        let r2 = r1 * saturate(in.extra_param);
        let dist = sd_trapezoid(p, r1, r2, half_size.y);
        let border_w = min(in.border_thickness, min(half_size.x, half_size.y));
        return shade(dist, aa_width(uv_fw, half_size), border_w, in.fill_color, in.border_color);
    }

    // ---- Regular polygon --------------------------------------------------
    if (st == SHAPE_POLYGON && in.sides >= 3u) {
        let n = f32(in.sides);
        let inv_n = 1.0 / n;
        let slice = TAU * inv_n;

        let angle = fast_atan2(in.uv.y, in.uv.x);
        let k = floor(0.5 + angle * (n * INV_TAU));

        // distance to the nearest edge line, normalised by the apothem
        let dist_poly = cos_small(k * slice - angle) * length(in.uv) / cos_small(PI * inv_n);

        let delta = max(uv_fw.x, uv_fw.y);
        let border_uv_width = in.border_thickness * (2.0 / min(in.size.x, in.size.y));
        return shade(dist_poly - 1.0, delta, border_uv_width, in.fill_color, in.border_color);
    }

    return in.fill_color;
}
