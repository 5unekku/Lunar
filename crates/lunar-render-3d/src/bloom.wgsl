// bloom: progressive downsample + tent-filter upsample
// reference: Jorge Jiménez "Next Generation Post Processing in CoD: Advanced Warfare"
//
// downsample pass: 13-tap Kawase filter reduces 2× each step
// upsample pass:   3×3 tent filter additively blends each level back up

struct BloomParams {
    texel_size: vec2<f32>,   // 1 / source dimensions
    filter_radius: f32,       // tent filter radius in texels (upsample only)
    threshold: f32,           // luminance threshold below which light is cut (downsample only)
}
@group(0) @binding(0) var<uniform> params: BloomParams;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;

// ── shared fullscreen vertex shader ───────────────────────────────────────

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    // triangle that covers the entire clip space
    let pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
    );
    let uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
    );
    var out: VertOut;
    out.clip_pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uvs[vi];
    return out;
}

// ── luminance helper ───────────────────────────────────────────────────────

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// soft knee threshold: smoothly cut content below threshold
fn apply_threshold(color: vec3<f32>, threshold: f32) -> vec3<f32> {
    let knee = threshold * 0.5;
    let lum = luminance(color);
    let ramp = clamp((lum - (threshold - knee)) / (2.0 * knee), 0.0, 1.0);
    return color * ramp;
}

// ── 13-tap Kawase downsample ───────────────────────────────────────────────
// samples a 2× block using a pattern that prevents aliasing better than a
// simple 4-tap box. each half-pixel offset sample represents a 2×2 bilinear tap.

@fragment
fn fs_downsample(in: VertOut) -> @location(0) vec4<f32> {
    let t = params.texel_size;
    let uv = in.uv;

    // centre 4 cross-shaped samples (half-pixel offset → 2×2 bilinear each)
    var col = textureSample(src, smp, uv).rgb * 4.0;

    // 4 inner 2×2 group samples
    col += textureSample(src, smp, uv + vec2<f32>(-2.0, -2.0) * t).rgb;
    col += textureSample(src, smp, uv + vec2<f32>( 2.0, -2.0) * t).rgb;
    col += textureSample(src, smp, uv + vec2<f32>(-2.0,  2.0) * t).rgb;
    col += textureSample(src, smp, uv + vec2<f32>( 2.0,  2.0) * t).rgb;

    // 8 outer cross samples (weight 2 each)
    col += textureSample(src, smp, uv + vec2<f32>(-4.0,  0.0) * t).rgb * 2.0;
    col += textureSample(src, smp, uv + vec2<f32>( 4.0,  0.0) * t).rgb * 2.0;
    col += textureSample(src, smp, uv + vec2<f32>( 0.0, -4.0) * t).rgb * 2.0;
    col += textureSample(src, smp, uv + vec2<f32>( 0.0,  4.0) * t).rgb * 2.0;

    // normalise: total weight = 4 + 4×1 + 4×2 = 16
    col /= 16.0;

    // apply threshold on the first downsample (threshold > 0 means it's the prefilter step)
    if params.threshold > 0.0 {
        col = apply_threshold(col, params.threshold);
    }

    return vec4<f32>(col, 1.0);
}

// ── 3×3 tent filter upsample ───────────────────────────────────────────────
// samples a 3×3 grid with a tent kernel. the result is ADDED to the destination
// (additive blend state on the render pipeline).

@fragment
fn fs_upsample(in: VertOut) -> @location(0) vec4<f32> {
    let r = params.filter_radius;
    let t = params.texel_size;
    let uv = in.uv;

    // 3×3 tent kernel weights: corner=1, edge=2, centre=4; total=16
    var col = textureSample(src, smp, uv + vec2<f32>(-r, -r) * t).rgb * 1.0;
    col    += textureSample(src, smp, uv + vec2<f32>( 0.0, -r) * t).rgb * 2.0;
    col    += textureSample(src, smp, uv + vec2<f32>( r, -r) * t).rgb * 1.0;
    col    += textureSample(src, smp, uv + vec2<f32>(-r,  0.0) * t).rgb * 2.0;
    col    += textureSample(src, smp, uv).rgb * 4.0;
    col    += textureSample(src, smp, uv + vec2<f32>( r,  0.0) * t).rgb * 2.0;
    col    += textureSample(src, smp, uv + vec2<f32>(-r,  r) * t).rgb * 1.0;
    col    += textureSample(src, smp, uv + vec2<f32>( 0.0,  r) * t).rgb * 2.0;
    col    += textureSample(src, smp, uv + vec2<f32>( r,  r) * t).rgb * 1.0;
    col    /= 16.0;

    return vec4<f32>(col, 1.0);
}
