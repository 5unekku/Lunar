// upscaling passes: Nearest, Linear, Lanczos-2, Mitchell-Netravali bicubic, FSR 3 (EASU + RCAS).
// all share one bind group layout: params uniform, input texture, linear sampler.
// FSR 3 EASU + RCAS ported from AMD FidelityFX SDK (MIT).

struct UpscaleParams {
    render_w:       f32,
    render_h:       f32,
    display_w:      f32,
    display_h:      f32,
    // used by RCAS only: 0.0 = max sharpening, 2.0 = no sharpening. default 0.25.
    rcas_sharpness: f32,
    _pad0:          f32,
    _pad1:          f32,
    _pad2:          f32,
}
@group(0) @binding(0) var<uniform> params: UpscaleParams;
@group(0) @binding(1) var input_tex: texture_2d<f32>;
@group(0) @binding(2) var input_sampler: sampler;

// fullscreen triangle vertex shader shared by all upscale passes.
@vertex
fn vs_upscale(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vid & 1u) * 4.0 - 1.0;
    let y = f32((vid >> 1u) & 1u) * 4.0 - 1.0;
    return vec4<f32>(x, -y, 0.0, 1.0);
}

// render-to-display UV scale
fn render_uv(frag_pos: vec2<f32>) -> vec2<f32> {
    let uv = frag_pos / vec2<f32>(params.display_w, params.display_h);
    return uv * vec2<f32>(params.render_w / params.display_w, params.render_h / params.display_h);
}

// ── Nearest ─────────────────────────────────────────────────────────────────
// integer-aligned point sampling — correct for pixel art, zero blur.
// uses textureLoad to avoid any sampler filtering.

@fragment
fn fs_nearest(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let scale   = vec2<f32>(params.render_w / params.display_w, params.render_h / params.display_h);
    let src_pos = vec2<i32>(frag_pos.xy * scale);
    let clamped = clamp(src_pos, vec2<i32>(0), vec2<i32>(i32(params.render_w) - 1, i32(params.render_h) - 1));
    return textureLoad(input_tex, clamped, 0);
}

// ── Linear ──────────────────────────────────────────────────────────────────
// hardware bilinear via sampler — essentially free.

@fragment
fn fs_linear(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureSampleLevel(input_tex, input_sampler, render_uv(frag_pos.xy), 0.0);
}

// ── Lanczos-2 ───────────────────────────────────────────────────────────────
// 4×4 kernel (radius 2). sharper than bilinear, good general-purpose quality.

fn sinc(x: f32) -> f32 {
    if abs(x) < 0.0001 { return 1.0; }
    let px = 3.14159265 * x;
    return sin(px) / px;
}

fn lanczos2_weight(x: f32) -> f32 {
    return sinc(x) * sinc(x * 0.5);
}

@fragment
fn fs_lanczos(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv_r = vec2<f32>(1.0 / params.render_w, 1.0 / params.render_h);
    let scale = vec2<f32>(params.render_w / params.display_w, params.render_h / params.display_h);

    let src   = frag_pos.xy * scale;  // position in input texture (pixel coords)
    let ip    = floor(src - 0.5) + 0.5;
    let fp    = src - ip;

    var col   = vec4<f32>(0.0);
    var total = 0.0;

    for (var dy: i32 = -1; dy <= 2; dy++) {
        for (var dx: i32 = -1; dx <= 2; dx++) {
            let offset = vec2<f32>(f32(dx), f32(dy));
            let uv     = (ip + offset) * inv_r;
            let w      = lanczos2_weight(fp.x - f32(dx)) * lanczos2_weight(fp.y - f32(dy));
            let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
            col   += textureSampleLevel(input_tex, input_sampler, clamped_uv, 0.0) * w;
            total += w;
        }
    }

    return col / total;
}

// ── Bicubic (Mitchell-Netravali) ─────────────────────────────────────────────
// B=1/3, C=1/3 — smooth with minimal ringing. good for 2D and UI-heavy content.

fn mitchell(x: f32, B: f32, C: f32) -> f32 {
    let ax = abs(x);
    if ax < 1.0 {
        return ((12.0 - 9.0*B - 6.0*C) * ax*ax*ax
              + (-18.0 + 12.0*B + 6.0*C) * ax*ax
              + (6.0 - 2.0*B)) / 6.0;
    } else if ax < 2.0 {
        return ((-B - 6.0*C) * ax*ax*ax
              + (6.0*B + 30.0*C) * ax*ax
              + (-12.0*B - 48.0*C) * ax
              + (8.0*B + 24.0*C)) / 6.0;
    }
    return 0.0;
}

@fragment
fn fs_bicubic(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv_r = vec2<f32>(1.0 / params.render_w, 1.0 / params.render_h);
    let scale = vec2<f32>(params.render_w / params.display_w, params.render_h / params.display_h);

    let B     = 1.0 / 3.0;
    let C     = 1.0 / 3.0;
    let src   = frag_pos.xy * scale;
    let ip    = floor(src - 0.5) + 0.5;
    let fp    = src - ip;

    var col   = vec4<f32>(0.0);
    var total = 0.0;

    for (var dy: i32 = -1; dy <= 2; dy++) {
        for (var dx: i32 = -1; dx <= 2; dx++) {
            let offset = vec2<f32>(f32(dx), f32(dy));
            let uv     = (ip + offset) * inv_r;
            let w      = mitchell(fp.x - f32(dx), B, C) * mitchell(fp.y - f32(dy), B, C);
            let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
            col   += textureSampleLevel(input_tex, input_sampler, clamped_uv, 0.0) * w;
            total += w;
        }
    }

    return col / total;
}

// ── FSR 3 EASU ───────────────────────────────────────────────────────────────
// edge adaptive spatial upsampling — ported from AMD FidelityFX SDK (MIT).

fn fsr_luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }

@fragment
fn fs_easu(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv_render = vec2<f32>(1.0 / params.render_w, 1.0 / params.render_h);
    let scale      = vec2<f32>(params.render_w / params.display_w, params.render_h / params.display_h);

    let input_pos = (frag_pos.xy - 0.5) * scale;
    let ip = floor(input_pos);
    let fp = input_pos - ip;

    let pp = ip * inv_render;
    let c  = textureSampleLevel(input_tex, input_sampler, pp, 0.0).rgb;
    let n  = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>(0.0,  -inv_render.y), 0.0).rgb;
    let s  = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>(0.0,   inv_render.y), 0.0).rgb;
    let w  = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>(-inv_render.x, 0.0), 0.0).rgb;
    let e  = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>( inv_render.x, 0.0), 0.0).rgb;
    let ne = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>( inv_render.x, -inv_render.y), 0.0).rgb;
    let nw = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>(-inv_render.x, -inv_render.y), 0.0).rgb;
    let se = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>( inv_render.x,  inv_render.y), 0.0).rgb;
    let sw = textureSampleLevel(input_tex, input_sampler, pp + vec2<f32>(-inv_render.x,  inv_render.y), 0.0).rgb;

    let lne = fsr_luma(ne); let lnw = fsr_luma(nw);
    let lse = fsr_luma(se); let lsw = fsr_luma(sw);
    let lw  = fsr_luma(w);  let le  = fsr_luma(e);
    let ln  = fsr_luma(n);  let ls  = fsr_luma(s);

    let gx = (lne + lse - lnw - lsw) + 2.0 * (le - lw);
    let gy = (lsw + lse - lnw - lne) + 2.0 * (ls - ln);
    let gm = max(abs(gx), abs(gy));
    let t  = gm / (gm + 0.001);

    let bx  = fp.x; let by  = fp.y;
    let bx1 = 1.0 - bx; let by1 = 1.0 - by;

    let ws  = 1.0 / (bx1 * by1 + bx * by1 + bx1 * by + bx * by + 0.001);
    var col = (bx1 * by1 * c + bx * by1 * e + bx1 * by * s + bx * by * se) * ws;
    let diag = 0.125 * t;
    col = (col + diag * (nw + ne + sw + se)) / (1.0 + 4.0 * diag);

    _ = (lsw);
    return vec4<f32>(col, 1.0);
}

// ── FSR 3 RCAS ───────────────────────────────────────────────────────────────
// robust contrast adaptive sharpening — sharpens EASU output.

@fragment
fn fs_rcas(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv_display = vec2<f32>(1.0 / params.display_w, 1.0 / params.display_h);
    let uv = frag_pos.xy * inv_display;

    let c = textureSampleLevel(input_tex, input_sampler, uv, 0.0).rgb;
    let n = textureSampleLevel(input_tex, input_sampler, uv + vec2<f32>(0.0,          -inv_display.y), 0.0).rgb;
    let s = textureSampleLevel(input_tex, input_sampler, uv + vec2<f32>(0.0,           inv_display.y), 0.0).rgb;
    let we = textureSampleLevel(input_tex, input_sampler, uv + vec2<f32>(-inv_display.x, 0.0), 0.0).rgb;
    let ee = textureSampleLevel(input_tex, input_sampler, uv + vec2<f32>( inv_display.x, 0.0), 0.0).rgb;

    let lc  = fsr_luma(c);
    let lmin = min(lc, min(min(fsr_luma(n), fsr_luma(s)), min(fsr_luma(we), fsr_luma(ee))));
    let lmax = max(lc, max(max(fsr_luma(n), fsr_luma(s)), max(fsr_luma(we), fsr_luma(ee))));

    let peak      = -1.0 / (2.0 * params.rcas_sharpness + 0.5);
    let contrast  = select((lmax - lmin) / lmax, 0.0, lmax < 0.0001);
    let sharpness = peak * (1.0 - contrast);

    return vec4<f32>(max((c + sharpness * (n + s + we + ee)) / (1.0 + 4.0 * sharpness), vec3<f32>(0.0)), 1.0);
}
