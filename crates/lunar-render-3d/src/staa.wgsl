// STAA — spatial-temporal anti-aliasing.
//
// a selective hybrid of spatial (smaa/fxaa-style) and temporal (taa) AA.
// per pixel it chooses between, and blends, two resolves and applies neither to
// already-clean surfaces. goal: the edge quality of smaa + the sub-pixel stability
// of taa, without the full-frame blur either produces on its own.
//
// "minimal destruction": smooth, stable regions pass through 100% unchanged.
// msaa handles per-frame geometric edges; this pass handles what msaa leaves:
//   - shimmer: specular flicker, thin-geometry aliasing, sub-pixel instability
//   - moving edges: spatial directional resolve every frame (no jitter required)
//   - static edges: temporal accumulation of the camera jitter offset → true ssaa
//
// per-pixel decision:
//   1. edge_conf     = smooth ramp on 3×3 luma contrast (exactly 0 on flat areas)
//   2. shimmer_weight = temporal luma delta vs clipped history
//   3. spatial resolve runs on edges, weighted UP under motion (when temporal jitter
//      accumulation can't work) and DOWN when static-jitter ssaa is doing the job
//   4. temporal resolve blends clipped history toward the spatially-resolved current
//   5. output = mix(spatial_current, temporal, max(edge_conf, shimmer_weight))
//      → clean regions: mask 0 → spatial_current → center → untouched
//
// no post-sharpening: nothing is blurred globally, so no corrective filter is needed.

struct TaaParams {
    prev_vp:     mat4x4<f32>,   // previous-frame view-projection (jittered, matches history)
    inv_vp:      mat4x4<f32>,   // inverse of current jittered view-projection
    jitter:      vec2<f32>,     // current frame jitter in uv space (zero while moving)
    rcp_frame:   vec2<f32>,     // (1/display_w, 1/display_h)
    // blend_alpha: base temporal blend weight within the masked region (default 0.1)
    blend_alpha: f32,
    frame_index: u32,           // 0 = cold start: skip temporal blend
    // depth_scale: render_resolution / display_resolution.
    // staa runs at display resolution but depth_tex is at render resolution.
    depth_scale: vec2<f32>,
    // prev_jitter: previous frame jitter in uv space, to fully un-jitter velocity.
    prev_jitter: vec2<f32>,
}

@group(0) @binding(0) var<uniform> params:      TaaParams;
@group(0) @binding(1) var current_tex: texture_2d<f32>;  // composite ldr output
@group(0) @binding(2) var depth_tex:   texture_2d<f32>;  // non-msaa depth (unfilterable)
@group(0) @binding(3) var history_tex: texture_2d<f32>;  // previous frame taa output
@group(0) @binding(4) var linear_smp:  sampler;
@group(0) @binding(5) var nearest_smp: sampler;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct FragOut {
    @location(0) present: vec4<f32>,  // swapchain
    @location(1) history: vec4<f32>,  // persistent history for next frame
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
    return dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(c, vec3<f32>( 0.25,  0.5,  0.25)),
        dot(c, vec3<f32>( 0.5,   0.0, -0.5 )),
        dot(c, vec3<f32>(-0.25,  0.5, -0.25)),
    );
}

fn ycocg_to_rgb(ycocg: vec3<f32>) -> vec3<f32> {
    let t = ycocg.x - ycocg.z;
    return clamp(vec3<f32>(t + ycocg.y, ycocg.x + ycocg.z, t - ycocg.y), vec3<f32>(0.0), vec3<f32>(1.0));
}

// clips history into the neighborhood aabb in ycocg space.
// prevents ghosting on moving objects without harsh color discontinuities.
fn clip_aabb(history: vec3<f32>, aabb_min: vec3<f32>, aabb_max: vec3<f32>) -> vec3<f32> {
    let center = (aabb_min + aabb_max) * 0.5;
    let half_e = (aabb_max - aabb_min) * 0.5 + 0.00001;
    let v = history - center;
    let max_t = max(max(abs(v.x) / half_e.x, abs(v.y) / half_e.y), abs(v.z) / half_e.z);
    if max_t > 1.0 {
        return center + v / max_t;
    }
    return history;
}

// fxaa-style directional edge resolve. estimates the edge tangent from the diagonal
// luma gradients, then blends ALONG that tangent — smoothing the staircase with
// minimal cross-edge blur. 4 taps, run only on edge pixels.
const FXAA_SPAN_MAX:   f32 = 8.0;
const FXAA_REDUCE_MUL: f32 = 1.0 / 8.0;
const FXAA_REDUCE_MIN: f32 = 1.0 / 128.0;

fn fxaa_resolve(uv: vec2<f32>, rc: vec2<f32>,
                luma_nw: f32, luma_ne: f32, luma_sw: f32, luma_se: f32,
                luma_min: f32, luma_max: f32) -> vec3<f32> {
    var dir = vec2<f32>(
        -((luma_nw + luma_ne) - (luma_sw + luma_se)),
         ((luma_nw + luma_sw) - (luma_ne + luma_se)),
    );
    let reduce  = max((luma_nw + luma_ne + luma_sw + luma_se) * 0.25 * FXAA_REDUCE_MUL, FXAA_REDUCE_MIN);
    let rcp_min = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * rcp_min, vec2<f32>(-FXAA_SPAN_MAX), vec2<f32>(FXAA_SPAN_MAX)) * rc;

    let rgb_a = 0.5 * (
        textureSample(current_tex, linear_smp, uv + dir * (1.0 / 3.0 - 0.5)).rgb +
        textureSample(current_tex, linear_smp, uv + dir * (2.0 / 3.0 - 0.5)).rgb);
    let rgb_b = rgb_a * 0.5 + 0.25 * (
        textureSample(current_tex, linear_smp, uv + dir * -0.5).rgb +
        textureSample(current_tex, linear_smp, uv + dir *  0.5).rgb);

    // if the wider blend overshoots the local luma range it has bled across the
    // edge — fall back to the narrower (lower-blur) blend.
    let luma_b = luma(rgb_b);
    return select(rgb_a, rgb_b, luma_b >= luma_min && luma_b <= luma_max);
}

// catmull-rom (bicubic) history sample — 9 bilinear taps via the standard weight
// trick. preserves the high frequencies a single bilinear tap low-passes away, so
// repeated reprojection keeps the accumulated image sharp instead of softening it.
// negative side lobes can ring, but the aabb clip on the result tames that.
fn sample_history_catmull_rom(uv: vec2<f32>, rc: vec2<f32>) -> vec3<f32> {
    let tex_size   = 1.0 / rc;
    let sample_pos = uv * tex_size;
    let tex_pos1   = floor(sample_pos - 0.5) + 0.5;
    let f          = sample_pos - tex_pos1;

    let w0  = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1  = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2  = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3  = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;

    let off0  = (tex_pos1 - 1.0)      * rc;
    let off12 = (tex_pos1 + w2 / w12) * rc;
    let off3  = (tex_pos1 + 2.0)      * rc;

    var c = vec3<f32>(0.0);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off0.x,  off0.y ), 0.0).rgb * (w0.x  * w0.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off12.x, off0.y ), 0.0).rgb * (w12.x * w0.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off3.x,  off0.y ), 0.0).rgb * (w3.x  * w0.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off0.x,  off12.y), 0.0).rgb * (w0.x  * w12.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off12.x, off12.y), 0.0).rgb * (w12.x * w12.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off3.x,  off12.y), 0.0).rgb * (w3.x  * w12.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off0.x,  off3.y ), 0.0).rgb * (w0.x  * w3.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off12.x, off3.y ), 0.0).rgb * (w12.x * w3.y);
    c += textureSampleLevel(history_tex, linear_smp, vec2<f32>(off3.x,  off3.y ), 0.0).rgb * (w3.x  * w3.y);
    return c;
}

@fragment
fn fs_main(in: VertOut) -> FragOut {
    let uv = in.uv;
    let rc = params.rcp_frame;

    // ── current frame 3×3 neighborhood ───────────────────────────────────
    let center = textureSample(current_tex, linear_smp, uv).rgb;
    let n  = textureSample(current_tex, linear_smp, uv + vec2<f32>( 0.0, -rc.y)).rgb;
    let s  = textureSample(current_tex, linear_smp, uv + vec2<f32>( 0.0,  rc.y)).rgb;
    let e  = textureSample(current_tex, linear_smp, uv + vec2<f32>( rc.x,  0.0)).rgb;
    let w  = textureSample(current_tex, linear_smp, uv + vec2<f32>(-rc.x,  0.0)).rgb;
    let ne = textureSample(current_tex, linear_smp, uv + vec2<f32>( rc.x, -rc.y)).rgb;
    let nw = textureSample(current_tex, linear_smp, uv + vec2<f32>(-rc.x, -rc.y)).rgb;
    let se = textureSample(current_tex, linear_smp, uv + vec2<f32>( rc.x,  rc.y)).rgb;
    let sw = textureSample(current_tex, linear_smp, uv + vec2<f32>(-rc.x,  rc.y)).rgb;

    // ── ycocg neighborhood aabb for ghost rejection ───────────────────────
    let c_yc  = rgb_to_ycocg(center);
    let n_yc  = rgb_to_ycocg(n);  let s_yc  = rgb_to_ycocg(s);
    let e_yc  = rgb_to_ycocg(e);  let w_yc  = rgb_to_ycocg(w);
    let ne_yc = rgb_to_ycocg(ne); let nw_yc = rgb_to_ycocg(nw);
    let se_yc = rgb_to_ycocg(se); let sw_yc = rgb_to_ycocg(sw);
    var aabb_min = min(c_yc, min(min(n_yc, s_yc), min(e_yc, w_yc)));
    var aabb_max = max(c_yc, max(max(n_yc, s_yc), max(e_yc, w_yc)));
    aabb_min = min(aabb_min, min(min(ne_yc, nw_yc), min(se_yc, sw_yc)));
    aabb_max = max(aabb_max, max(max(ne_yc, nw_yc), max(se_yc, sw_yc)));

    // ── neighborhood luma (edge detect + fxaa direction) ──────────────────
    let curr_luma = luma(center);
    let luma_n  = luma(n);  let luma_s  = luma(s);  let luma_e  = luma(e);  let luma_w  = luma(w);
    let luma_ne = luma(ne); let luma_nw = luma(nw); let luma_se = luma(se); let luma_sw = luma(sw);
    let luma_min = min(curr_luma, min(min(min(luma_n, luma_s), min(luma_e, luma_w)),
                                      min(min(luma_ne, luma_nw), min(luma_se, luma_sw))));
    let luma_max = max(curr_luma, max(max(max(luma_n, luma_s), max(luma_e, luma_w)),
                                      max(max(luma_ne, luma_nw), max(luma_se, luma_sw))));
    let contrast = luma_max - luma_min;

    // edge confidence: smooth ramp above a relative+absolute threshold (lower than
    // fxaa's to catch sub-pixel edges). stays exactly 0 below threshold, so flat
    // regions are never touched.
    let edge_lo   = max(0.03, luma_max * 0.06);
    let edge_conf = saturate((contrast - edge_lo) / edge_lo);

    // ── depth reprojection to previous-frame uv ───────────────────────────
    // textureLoad avoids needing a comparison sampler for depth. depth_scale maps
    // display-res coords → render-res depth texels (they differ when render_scale<1).
    let texel  = vec2<i32>(in.clip_pos.xy * params.depth_scale);
    let depth  = textureLoad(depth_tex, texel, 0).r;
    let is_sky = depth >= 0.9999;

    // wgpu rasterizes y-up ndc into y-down uv, so y flips on the way in and out.
    let ndc     = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let world_h = params.inv_vp * ndc;
    let world   = world_h.xyz / world_h.w;

    let prev_clip = params.prev_vp * vec4<f32>(world, 1.0);
    let prev_ndc  = prev_clip.xy / prev_clip.w;
    let prev_uv   = vec2<f32>(prev_ndc.x * 0.5 + 0.5, 0.5 - prev_ndc.y * 0.5);

    let in_bounds = all(prev_uv >= vec2<f32>(0.0)) && all(prev_uv <= vec2<f32>(1.0));

    // velocity in screen-pixels, with BOTH frames' jitter removed. prev_uv is sampled
    // from the jittered prev_vp (correct for history), so subtract prev_jitter here to
    // recover true motion — otherwise a static camera reads ~0.5px of phantom speed.
    let curr_uv_unjittered = uv - params.jitter;
    let prev_uv_unjittered = prev_uv - params.prev_jitter;
    let velocity_uv        = curr_uv_unjittered - prev_uv_unjittered;
    let speed_px           = length(velocity_uv) / max(rc.x, rc.y);

    // ── history (cheap bilinear) for shimmer detection ────────────────────
    let history_bilinear = textureSample(history_tex, linear_smp, prev_uv).rgb;
    let detect_clipped   = ycocg_to_rgb(clip_aabb(rgb_to_ycocg(history_bilinear), aabb_min, aabb_max));

    // ── spatial edge resolve (the "S") ────────────────────────────────────
    // only the 4 extra taps run on edges. directional along the edge tangent, so it
    // removes the staircase with minimal cross-edge blur. independent of jitter —
    // this is what anti-aliases moving edges, where temporal accumulation can't.
    var spatial = center;
    if edge_conf > 0.0 {
        spatial = fxaa_resolve(uv, rc, luma_nw, luma_ne, luma_sw, luma_se, luma_min, luma_max);
    }

    // spatial vs temporal balance on edges:
    //   - camera dead-still + jitter active → temporal accumulates true ssaa, so back
    //     spatial off (it would only add blur over an already-resolved edge).
    //   - any motion → jitter is disabled upstream and history reprojects, so spatial
    //     carries the edge AA while temporal just stabilizes it.
    let jitter_active = dot(params.jitter, params.jitter) > 1e-12;
    let ssaa_factor   = select(0.0, saturate(1.0 - speed_px * 0.5), jitter_active);
    // fully hand edges to temporal ssaa when dead-still (no residual fxaa blur);
    // ramp spatial back in as soon as motion starts and jitter accumulation stops.
    let spatial_amt   = edge_conf * (1.0 - ssaa_factor);
    let current_aa    = mix(center, spatial, spatial_amt);

    // ── shimmer detection ─────────────────────────────────────────────────
    // high luma delta between current and clipped history → temporal stabilization.
    let hist_luma      = luma(detect_clipped);
    let temporal_diff  = abs(curr_luma - hist_luma) / max(max(curr_luma, hist_luma), 0.01);
    let shimmer_weight = saturate((temporal_diff - 0.02) * 20.0);   // 0 below 2%, 1 at 7%

    var taa_mask = max(shimmer_weight, edge_conf);
    // no temporal for sky, out-of-frame reprojection, very fast motion (history is
    // probably wrong), or cold start. spatial still applies via current_aa.
    if is_sky || !in_bounds || speed_px > 12.0 || params.frame_index == 0u {
        taa_mask = 0.0;
    }

    // ── temporal resolve (the "T"), only where it contributes ─────────────
    // smooth surfaces: taa_mask 0 → output stays current_aa (= center) → unchanged.
    var output = current_aa;
    if taa_mask > 0.0 {
        // catmull-rom history fetch keeps the accumulation sharp (plain bilinear here
        // is what softens a still image). clip afterwards to tame bicubic ringing.
        let history_sharp   = sample_history_catmull_rom(prev_uv, rc);
        let history_clipped = ycocg_to_rgb(clip_aabb(rgb_to_ycocg(history_sharp), aabb_min, aabb_max));
        // faster motion → more current frame → less ghost risk.
        let motion_alpha    = min(0.5, speed_px * 0.05);
        let alpha           = max(params.blend_alpha, motion_alpha);
        // accumulate against the spatially-resolved current so history converges to an
        // AA'd result rather than re-introducing the aliased center each frame.
        let taa_blend       = mix(history_clipped, current_aa, alpha);
        output              = mix(current_aa, taa_blend, taa_mask);
    }

    var result: FragOut;
    result.present = vec4<f32>(output, 1.0);
    result.history = vec4<f32>(output, 1.0);
    return result;
}
