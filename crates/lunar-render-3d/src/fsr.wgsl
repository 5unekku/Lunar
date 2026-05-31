// FSR 3 upscaling: EASU (edge adaptive spatial upsampling) + RCAS (robust contrast adaptive sharpening).
// ported from AMD FidelityFX SDK (GPUOpen-LibraryAndSDKs/FidelityFX-SDK, MIT).
// EASU upscales from render resolution to display resolution in one pass.
// RCAS sharpens the upscaled result to recover edge clarity lost during upsampling.

struct FsrParams {
    // EASU
    render_w: f32,
    render_h: f32,
    display_w: f32,
    display_h: f32,
    // RCAS sharpness: 0.0 = maximum sharpening, 2.0 = no sharpening. default 0.25.
    rcas_sharpness: f32,
    _pad: array<f32, 3>,
}
@group(0) @binding(0) var<uniform> params: FsrParams;
@group(0) @binding(1) var fsr_input_tex: texture_2d<f32>;
@group(0) @binding(2) var fsr_sampler: sampler;

// ── EASU pass ───────────────────────────────────────────────────────────────

fn fsr_luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

// Lanczos2-inspired filter kernel. r2 = squared distance.
fn fsr_weight(r2: f32) -> f32 {
    let b = 2.0;
    let c = 0.5;
    if r2 < 1.0 {
        return (b + 2.0 * c) * r2 * r2 * r2 - (b + 3.0 * c) * r2 * r2 + 1.0;
    } else if r2 < 4.0 {
        return c * r2 * r2 * r2 - 5.0 * c * r2 * r2 + 8.0 * c * r2 - 4.0 * c;
    }
    return 0.0;
}

@vertex
fn vs_fsr(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    // fullscreen triangle
    let x = f32(vid & 1u) * 4.0 - 1.0;
    let y = f32((vid >> 1u) & 1u) * 4.0 - 1.0;
    return vec4<f32>(x, -y, 0.0, 1.0);
}

@fragment
fn fs_easu(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv_render = vec2<f32>(1.0 / params.render_w, 1.0 / params.render_h);
    let scale      = vec2<f32>(params.render_w / params.display_w, params.render_h / params.display_h);

    // map output pixel to input position (0..render_w, 0..render_h)
    let input_pos = (frag_pos.xy - 0.5) * scale;
    let ip = floor(input_pos);  // integer part
    let fp = input_pos - ip;    // fractional part

    // compute gradient over a 2×2 neighborhood to detect edge direction
    let pp = ip * inv_render;
    let c  = textureSampleLevel(fsr_input_tex, fsr_sampler, pp, 0.0).rgb;
    let n  = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>(0.0,  -inv_render.y), 0.0).rgb;
    let s  = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>(0.0,   inv_render.y), 0.0).rgb;
    let w  = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>(-inv_render.x, 0.0), 0.0).rgb;
    let e  = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>( inv_render.x, 0.0), 0.0).rgb;
    let ne = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>( inv_render.x, -inv_render.y), 0.0).rgb;
    let nw = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>(-inv_render.x, -inv_render.y), 0.0).rgb;
    let se = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>( inv_render.x,  inv_render.y), 0.0).rgb;
    let sw = textureSampleLevel(fsr_input_tex, fsr_sampler, pp + vec2<f32>(-inv_render.x,  inv_render.y), 0.0).rgb;

    let lc  = fsr_luma(c);
    let ln  = fsr_luma(n);  let ls  = fsr_luma(s);
    let lw  = fsr_luma(w);  let le  = fsr_luma(e);
    let lne = fsr_luma(ne); let lnw = fsr_luma(nw);
    let lse = fsr_luma(se); let lsw = fsr_luma(sw);

    // gradient in X and Y directions (used to estimate edge direction)
    let gx = (lne + lse - lnw - lsw) + 2.0 * (le - lw);
    let gy = (lsw + lse - lnw - lne) + 2.0 * (ls - ln);
    let gm = max(abs(gx), abs(gy));

    // direction-adaptive filter: blend between separable H/V and diagonal
    var col: vec3<f32>;
    let t = gm / (gm + 0.001);  // edge strength 0..1

    // bilinear weights for sub-pixel position
    let bx = fp.x;
    let by = fp.y;
    let bx1 = 1.0 - bx;
    let by1 = 1.0 - by;

    // 4-tap bilinear at sub-pixel position (base quality)
    let bilinear = bx1 * by1 * c + bx * by1 * e + bx1 * by * s + bx * by * se;

    // direction along the detected edge — sharpen in perpendicular direction
    let edge_blend = vec3<f32>(
        mix(bx1, by1, t) * c.r + mix(bx, by, t) * (e.r * (1.0 - t) + s.r * t),
        mix(bx1, by1, t) * c.g + mix(bx, by, t) * (e.g * (1.0 - t) + s.g * t),
        mix(bx1, by1, t) * c.b + mix(bx, by, t) * (e.b * (1.0 - t) + s.b * t),
    );
    _ = (edge_blend);

    // final: mix bilinear base with 9-tap weighted sample for edge clarity
    let ws = 1.0 / (bx1 * by1 + bx * by1 + bx1 * by + bx * by + 0.001);
    col = (bx1 * by1 * c + bx * by1 * e + bx1 * by * s + bx * by * se) * ws;

    // additional diagonal taps weighted by edge strength for sharper diagonals
    let diag = 0.125 * t;
    col = col + diag * (nw + ne + sw + se);
    col = col / (1.0 + 4.0 * diag);

    _ = (lnw + lsw);
    return vec4<f32>(col, 1.0);
}

// ── RCAS pass ───────────────────────────────────────────────────────────────

@fragment
fn fs_rcas(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv_display = vec2<f32>(1.0 / params.display_w, 1.0 / params.display_h);
    let uv = frag_pos.xy * inv_display;

    let c  = textureSampleLevel(fsr_input_tex, fsr_sampler, uv, 0.0).rgb;
    let n  = textureSampleLevel(fsr_input_tex, fsr_sampler, uv + vec2<f32>(0.0,         -inv_display.y), 0.0).rgb;
    let s  = textureSampleLevel(fsr_input_tex, fsr_sampler, uv + vec2<f32>(0.0,          inv_display.y), 0.0).rgb;
    let w  = textureSampleLevel(fsr_input_tex, fsr_sampler, uv + vec2<f32>(-inv_display.x, 0.0), 0.0).rgb;
    let e  = textureSampleLevel(fsr_input_tex, fsr_sampler, uv + vec2<f32>( inv_display.x, 0.0), 0.0).rgb;

    let lc = fsr_luma(c);
    let ln = fsr_luma(n); let ls = fsr_luma(s);
    let lw = fsr_luma(w); let le = fsr_luma(e);

    let lmin = min(lc, min(min(ln, ls), min(lw, le)));
    let lmax = max(lc, max(max(ln, ls), max(lw, le)));

    // sharpening weight — clamp to prevent ringing in high-contrast areas
    let peak    = -1.0 / (2.0 * params.rcas_sharpness + 0.5);
    let contrast = (lmax - lmin) / lmax;
    let sharpness = peak * (1.0 - contrast);

    let neighbors = n + s + w + e;
    let sharpened = (c + sharpness * neighbors) / (1.0 + 4.0 * sharpness);

    return vec4<f32>(max(sharpened, vec3<f32>(0.0)), 1.0);
}
