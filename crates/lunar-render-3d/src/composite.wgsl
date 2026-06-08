// composite pass: HDR + bloom + SSR + volumetric fog → ACES tonemap → vignette
// → chromatic aberration → film grain → LDR swapchain output.
//
// runs once per frame as a fullscreen triangle after the bloom + SSR + fog pipelines.

struct CompositeParams {
    bloom_strength:    f32,
    vignette_strength: f32,
    vignette_radius:   f32,
    ca_strength:       f32,
    grain_strength:    f32,
    time_seed:         f32,
    // feature flags:
    //   bit 0 = bloom
    //   bit 1 = vignette
    //   bit 2 = chromatic aberration
    //   bit 3 = film grain
    //   bit 4 = GTAO AO
    //   bit 5 = SSR
    //   bit 6 = volumetric fog
    //   bit 7 = contact shadows
    flags: u32,
    _pad: f32,
    // colour quantization (all neutral when color_bits == 0):
    color_bits:    u32,   // bits per channel for final quantization (0 = off / full 8-bit)
    dither_size:   u32,   // ordered-dither Bayer matrix edge (0/2/4/8)
    dither_amount: f32,   // dither strength in [0,1]
    _pad2:         f32,
}
@group(0) @binding(0) var<uniform> params:              CompositeParams;
@group(0) @binding(1) var hdr_tex:              texture_2d<f32>;
@group(0) @binding(2) var bloom_tex:            texture_2d<f32>;
@group(0) @binding(3) var ao_tex:               texture_2d<f32>;
@group(0) @binding(4) var ssr_tex:              texture_2d<f32>;
@group(0) @binding(5) var fog_tex:              texture_2d<f32>;
@group(0) @binding(6) var smp:                  sampler;
@group(0) @binding(7) var contact_shadow_tex:   texture_2d<f32>;

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

fn aces_tonemap(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return saturate((x * (a * x + b)) / (x * (c * x + d) + e));
}

fn hash21(p: vec2<f32>) -> f32 {
    var h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

// ordered (Bayer) dither threshold for a pixel, centred to [-0.5, 0.5).
// size selects the matrix edge: 2, 8, or (default) 4.
fn ordered_dither(px: vec2<i32>, size: u32) -> f32 {
    if size == 2u {
        var m = array<f32, 4>(0.0, 2.0, 3.0, 1.0);
        let i = (px.y & 1) * 2 + (px.x & 1);
        return (m[i] + 0.5) / 4.0 - 0.5;
    }
    if size == 8u {
        var m = array<f32, 64>(
             0.0, 32.0,  8.0, 40.0,  2.0, 34.0, 10.0, 42.0,
            48.0, 16.0, 56.0, 24.0, 50.0, 18.0, 58.0, 26.0,
            12.0, 44.0,  4.0, 36.0, 14.0, 46.0,  6.0, 38.0,
            60.0, 28.0, 52.0, 20.0, 62.0, 30.0, 54.0, 22.0,
             3.0, 35.0, 11.0, 43.0,  1.0, 33.0,  9.0, 41.0,
            51.0, 19.0, 59.0, 27.0, 49.0, 17.0, 57.0, 25.0,
            15.0, 47.0,  7.0, 39.0, 13.0, 45.0,  5.0, 37.0,
            63.0, 31.0, 55.0, 23.0, 61.0, 29.0, 53.0, 21.0,
        );
        let i = (px.y & 7) * 8 + (px.x & 7);
        return (m[i] + 0.5) / 64.0 - 0.5;
    }
    var m = array<f32, 16>(
         0.0,  8.0,  2.0, 10.0,
        12.0,  4.0, 14.0,  6.0,
         3.0, 11.0,  1.0,  9.0,
        15.0,  7.0, 13.0,  5.0,
    );
    let i = (px.y & 3) * 4 + (px.x & 3);
    return (m[i] + 0.5) / 16.0 - 0.5;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    var uv = in.uv;

    // chromatic aberration: offset R and B channels outward from centre
    var hdr_color: vec3<f32>;
    if (params.flags & 4u) != 0u && params.ca_strength > 0.0 {
        let dir = (uv - vec2<f32>(0.5));
        let off = dir * params.ca_strength * 0.01;
        let r = textureSample(hdr_tex, smp, uv - off).r;
        let g = textureSample(hdr_tex, smp, uv).g;
        let b = textureSample(hdr_tex, smp, uv + off).b;
        hdr_color = vec3<f32>(r, g, b);
    } else {
        hdr_color = textureSample(hdr_tex, smp, uv).rgb;
    }

    // contact shadows: darken HDR where screen-space ray march detects occlusion
    if (params.flags & 128u) != 0u {
        let cs = textureSample(contact_shadow_tex, smp, uv).r;
        hdr_color *= 1.0 - cs * 0.8;
    }

    // GTAO ambient occlusion (before tonemap, in HDR space)
    if (params.flags & 16u) != 0u {
        let ao = textureSample(ao_tex, smp, uv).r;
        hdr_color *= ao;
    }

    // SSR: blend reflected color into HDR before tonemap for physically correct blending
    if (params.flags & 32u) != 0u {
        let ssr = textureSample(ssr_tex, smp, uv);
        hdr_color = mix(hdr_color, ssr.rgb / max(ssr.a, 0.001), ssr.a * 0.3);
    }

    // bloom
    if (params.flags & 1u) != 0u {
        hdr_color += textureSample(bloom_tex, smp, uv).rgb * params.bloom_strength;
    }

    // ACES filmic tonemap (HDR → LDR)
    var ldr = aces_tonemap(hdr_color);

    // volumetric fog: blend over LDR (after tonemap so fog isn't HDR-clamped)
    if (params.flags & 64u) != 0u {
        let fog = textureSample(fog_tex, smp, uv);
        // fog.rgb = in-scatter (linear), fog.a = 1 - transmittance
        ldr = ldr * (1.0 - fog.a) + aces_tonemap(fog.rgb);
    }

    // vignette
    if (params.flags & 2u) != 0u && params.vignette_strength > 0.0 {
        let vig_uv = uv * (1.0 - uv.yx);
        let vig = vig_uv.x * vig_uv.y * 15.0;
        let vig_factor = clamp(pow(vig, params.vignette_radius), 0.0, 1.0);
        ldr *= mix(1.0 - params.vignette_strength, 1.0, vig_factor);
    }

    // film grain
    if (params.flags & 8u) != 0u && params.grain_strength > 0.0 {
        let noise = hash21(in.clip_pos.xy + params.time_seed * 7919.0) * 2.0 - 1.0;
        ldr += noise * params.grain_strength * 0.05;
    }

    // ordered dither + per-channel colour quantization.
    // neutral and skipped entirely when color_bits == 0. dither is added in units of one
    // quantization step so it biases which level each pixel snaps to, hiding banding.
    if params.color_bits > 0u {
        let levels = f32((1u << params.color_bits) - 1u);
        var bias = 0.0;
        if params.dither_size > 0u {
            let px = vec2<i32>(floor(in.clip_pos.xy));
            bias = ordered_dither(px, params.dither_size) * params.dither_amount;
        }
        ldr = clamp(floor(ldr * levels + 0.5 + bias) / levels, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    return vec4<f32>(ldr, 1.0);
}
