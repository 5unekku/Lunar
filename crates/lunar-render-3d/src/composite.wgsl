// composite pass: HDR + bloom → ACES tonemap → vignette → chromatic aberration
// → film grain → LDR swapchain output
//
// runs once per frame as a fullscreen triangle after the bloom pipeline.

struct CompositeParams {
    // bloom
    bloom_strength: f32,
    // vignette
    vignette_strength: f32,
    vignette_radius: f32,
    // chromatic aberration
    ca_strength: f32,
    // film grain
    grain_strength: f32,
    // per-frame jitter seed (elapsed time mod 1.0)
    time_seed: f32,
    // feature flags packed as bits:
    //   bit 0 = bloom enabled
    //   bit 1 = vignette enabled
    //   bit 2 = chromatic aberration enabled
    //   bit 3 = film grain enabled
    flags: u32,
    _pad: f32,
}
@group(0) @binding(0) var<uniform> params: CompositeParams;
@group(0) @binding(1) var hdr_tex:   texture_2d<f32>;
@group(0) @binding(2) var bloom_tex: texture_2d<f32>;
@group(0) @binding(3) var smp:       sampler;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
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

// ── ACES filmic tonemap (Narkowicz 2015 approximation) ─────────────────────

fn aces_tonemap(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return saturate((x * (a * x + b)) / (x * (c * x + d) + e));
}

// ── low-quality hash for film grain ───────────────────────────────────────

fn hash21(p: vec2<f32>) -> f32 {
    var h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

// ── fragment ───────────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    var uv = in.uv;

    // chromatic aberration: offset R and B channels outward from centre
    var hdr_color: vec3<f32>;
    if (params.flags & 4u) != 0u && params.ca_strength > 0.0 {
        let centre = vec2<f32>(0.5, 0.5);
        let dir = (uv - centre);
        let off = dir * params.ca_strength * 0.01;
        let r = textureSample(hdr_tex, smp, uv - off).r;
        let g = textureSample(hdr_tex, smp, uv).g;
        let b = textureSample(hdr_tex, smp, uv + off).b;
        hdr_color = vec3<f32>(r, g, b);
    } else {
        hdr_color = textureSample(hdr_tex, smp, uv).rgb;
    }

    // add bloom
    if (params.flags & 1u) != 0u {
        let bloom_color = textureSample(bloom_tex, smp, uv).rgb;
        hdr_color += bloom_color * params.bloom_strength;
    }

    // ACES filmic tonemap (HDR → LDR)
    var ldr = aces_tonemap(hdr_color);

    // vignette: smooth darkening at screen edges
    if (params.flags & 2u) != 0u && params.vignette_strength > 0.0 {
        let vig_uv = uv * (1.0 - uv.yx);
        let vig = vig_uv.x * vig_uv.y * 15.0;
        let vig_factor = clamp(pow(vig, params.vignette_radius), 0.0, 1.0);
        ldr *= mix(1.0 - params.vignette_strength, 1.0, vig_factor);
    }

    // film grain: per-pixel white noise, modulated by luminance
    if (params.flags & 8u) != 0u && params.grain_strength > 0.0 {
        let grain_uv = in.clip_pos.xy + params.time_seed * 7919.0;
        let noise = hash21(grain_uv) * 2.0 - 1.0;
        ldr += noise * params.grain_strength * 0.05;
    }

    return vec4<f32>(ldr, 1.0);
}
