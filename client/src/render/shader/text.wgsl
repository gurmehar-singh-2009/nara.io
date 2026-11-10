/*
NOTE: this file is completely AI generated!
*/

// SDF text. One quad per glyph; the fill and the outline both come from the same
// distance sample, so outlined text needs a single draw instead of nine.

// Must match `EDGE` and `RANGE` in text.rs.
const EDGE: f32 = 0.75;
const RANGE: f32 = 16.0;

struct Globals {
    // xy = surface size in pixels, zw = atlas size in texels
    sizes: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct GlyphIn {
    // x, y, w, h in screen pixels
    @location(0) rect: vec4<f32>,
    // x, y, w, h in atlas texels
    @location(1) uv: vec4<f32>,
    @location(2) fill: vec4<f32>,
    @location(3) outline: vec4<f32>,
    // left, top, right, bottom in screen pixels
    @location(4) clip: vec4<f32>,
    // x = outline width in pixels, y = screen pixels per atlas texel
    @location(5) params: vec2<f32>,
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fill: vec4<f32>,
    @location(2) outline: vec4<f32>,
    @location(3) clip: vec4<f32>,
    @location(4) params: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, g: GlyphIn) -> VsOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = g.rect.xy + g.rect.zw * corner;

    var out: VsOut;
    out.pos = vec4<f32>(p / globals.sizes.xy * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = (g.uv.xy + g.uv.zw * corner) / globals.sizes.zw;
    out.fill = g.fill;
    out.outline = g.outline;
    out.clip = g.clip;
    out.params = g.params;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let s = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0).r;

    // distance to the glyph edge in screen pixels, positive outside
    let dist = (EDGE - s) * RANGE * in.params.y;

    let fill_cov = clamp(0.5 - dist, 0.0, 1.0);
    let line_cov = clamp(in.params.x + 0.5 - dist, 0.0, 1.0);

    let inside = step(in.clip.x, in.pos.x) * step(in.clip.y, in.pos.y)
        * step(in.pos.x, in.clip.z) * step(in.pos.y, in.clip.w);

    // the outline only covers the ring outside the fill, so a faded label fades evenly
    let fill_a = in.fill.a * fill_cov * inside;
    let ring_a = in.outline.a * max(line_cov - fill_cov, 0.0) * inside;
    let a = fill_a + ring_a;

    if a <= 0.001 {
        discard;
    }

    return vec4<f32>((in.fill.rgb * fill_a + in.outline.rgb * ring_a) / a, a);
}
