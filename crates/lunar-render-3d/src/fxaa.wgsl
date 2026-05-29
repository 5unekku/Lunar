// FXAA 3.11-style fast approximate anti-aliasing post-process.
//
// single fullscreen pass after the composite pass. reads from the LDR composite
// output (Bgra8Unorm), applies luma-edge-detection + directional blur, outputs
// to swapchain. enabled on low-tier (no MSAA) where this is the only AA path.
// mid/high tier relies on 4× MSAA and skips this pass.
//
// algorithm: Lottes 2009/2012, simplified variant
//   1. compute luma at center and 4 cardinal neighbors
//   2. if local contrast below threshold — skip, output center sample
//   3. detect dominant edge axis (horizontal vs vertical)
//   4. subpixel blend: shift sample by (luma gradient / total span) × 0.5 texel
//   5. output blended color

struct FxaaParams {
    rcp_frame: vec2<f32>,   // (1/width, 1/height) — texel size in UV space
    _pad0:     f32,
    _pad1:     f32,
}
@group(0) @binding(0) var<uniform> params: FxaaParams;
@group(0) @binding(1) var ldr_tex: texture_2d<f32>;   // composite LDR output
@group(0) @binding(2) var smp:     sampler;

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

fn luma(rgb: vec3<f32>) -> f32 {
    return dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let rc = params.rcp_frame;

    let m  = textureSample(ldr_tex, smp, uv).rgb;
    let n  = textureSample(ldr_tex, smp, uv + vec2<f32>( 0.0, -rc.y)).rgb;
    let s  = textureSample(ldr_tex, smp, uv + vec2<f32>( 0.0,  rc.y)).rgb;
    let e  = textureSample(ldr_tex, smp, uv + vec2<f32>( rc.x,  0.0)).rgb;
    let w  = textureSample(ldr_tex, smp, uv + vec2<f32>(-rc.x,  0.0)).rgb;

    let lM = luma(m); let lN = luma(n); let lS = luma(s);
    let lE = luma(e); let lW = luma(w);

    let lMin = min(lM, min(min(lN, lS), min(lE, lW)));
    let lMax = max(lM, max(max(lN, lS), max(lE, lW)));
    let lRange = lMax - lMin;

    // skip pixels below contrast threshold — 1/16 absolute + 1/8 relative
    if lRange < max(0.0625, lMax * 0.125) {
        return vec4<f32>(m, 1.0);
    }

    // diagonal corners for better edge detection
    let ne = textureSample(ldr_tex, smp, uv + vec2<f32>( rc.x, -rc.y)).rgb;
    let nw = textureSample(ldr_tex, smp, uv + vec2<f32>(-rc.x, -rc.y)).rgb;
    let se = textureSample(ldr_tex, smp, uv + vec2<f32>( rc.x,  rc.y)).rgb;
    let sw = textureSample(ldr_tex, smp, uv + vec2<f32>(-rc.x,  rc.y)).rgb;
    let lNE = luma(ne); let lNW = luma(nw); let lSE = luma(se); let lSW = luma(sw);

    // edge axis from cross derivatives
    let edge_h = abs(lNW + lN*2.0 + lNE - lSW - lS*2.0 - lSE);
    let edge_v = abs(lNW + lW*2.0 + lSW - lNE - lE*2.0 - lSE);

    let horiz = edge_h >= edge_v;

    // positive and negative steps along the perpendicular axis
    let step = select(vec2<f32>(rc.x, 0.0), vec2<f32>(0.0, rc.y), horiz);
    let lum_p = luma(textureSample(ldr_tex, smp, uv + step).rgb);
    let lum_n = luma(textureSample(ldr_tex, smp, uv - step).rgb);

    // subpixel aliasing ratio
    let lum_avg = (lN + lS + lE + lW) * 0.25;
    let sub_pixel = clamp(abs(lum_avg - lM) / lRange, 0.0, 1.0);
    let sub_blend = sub_pixel * sub_pixel * 0.75;

    // blend direction — towards the brighter neighbor
    let blend_dir = select(-step, step, lum_p > lum_n) * sub_blend * 0.5;

    let blended = textureSample(ldr_tex, smp, uv + blend_dir).rgb;
    return vec4<f32>(mix(m, blended, 0.5), 1.0);
}
