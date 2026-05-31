// selective temporal anti-aliasing.
//
// designed around "minimal destruction" — only modifies pixels that actually need it.
// smooth surfaces and already-clean regions pass through 100% unchanged.
// msaa handles per-frame geometric edge quality; this pass handles two remaining problems:
//   - shimmer: specular flicker, thin-geometry aliasing, sub-pixel instability
//   - sub-pixel edges: accumulates the camera jitter offset across frames for edge AA
//
// per-pixel decision:
//   1. detect shimmer (temporal luma variance vs history)
//   2. detect edge-adjacent pixels (spatial luma gradient)
//   3. compute taa_mask = max(shimmer_weight, edge_weight)
//   4. smooth static surfaces → taa_mask = 0 → output = current frame unchanged
//   5. masked regions → variance-clipped history blend → temporal stabilization
//
// no post-sharpening: we are not blurring the whole frame, so no corrective filter needed.
// the blend on edge regions averages msaa-resolved frames at different jitter positions,
// which does not increase blur relative to a single msaa frame.

struct TaaParams {
    prev_vp:     mat4x4<f32>,   // unjittered view-projection from previous frame
    inv_vp:      mat4x4<f32>,   // inverse of current jittered view-projection
    jitter:      vec2<f32>,     // current frame jitter in uv space
    rcp_frame:   vec2<f32>,     // (1/display_w, 1/display_h)
    // blend_alpha: base temporal blend weight within the masked region (default 0.1)
    blend_alpha: f32,
    frame_index: u32,           // 0 = cold start: skip temporal blend
    // depth_scale: render_resolution / display_resolution.
    // staa runs at display resolution but depth_tex is at render resolution.
    depth_scale: vec2<f32>,
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
    let yc = rgb_to_ycocg(center);
    var aabb_min = min(yc, min(min(rgb_to_ycocg(n), rgb_to_ycocg(s)), min(rgb_to_ycocg(e), rgb_to_ycocg(w))));
    var aabb_max = max(yc, max(max(rgb_to_ycocg(n), rgb_to_ycocg(s)), max(rgb_to_ycocg(e), rgb_to_ycocg(w))));
    aabb_min = min(aabb_min, min(min(rgb_to_ycocg(ne), rgb_to_ycocg(nw)), min(rgb_to_ycocg(se), rgb_to_ycocg(sw))));
    aabb_max = max(aabb_max, max(max(rgb_to_ycocg(ne), rgb_to_ycocg(nw)), max(rgb_to_ycocg(se), rgb_to_ycocg(sw))));

    // ── depth reprojection to get previous-frame uv ───────────────────────
    // use textureLoad to avoid needing a comparison sampler for depth.
    // scale by depth_scale because depth_tex is at render resolution while staa
    // runs at display resolution (the two differ when render_scale < 1.0).
    let texel  = vec2<i32>(in.clip_pos.xy * params.depth_scale);
    let depth  = textureLoad(depth_tex, texel, 0).r;
    let is_sky = depth >= 0.9999;

    let ndc     = vec4<f32>(uv * 2.0 - 1.0, depth, 1.0);
    let world_h = params.inv_vp * ndc;
    let world   = world_h.xyz / world_h.w;

    let prev_clip = params.prev_vp * vec4<f32>(world, 1.0);
    let prev_uv   = (prev_clip.xy / prev_clip.w) * 0.5 + 0.5;

    let in_bounds = all(prev_uv >= vec2<f32>(0.0)) && all(prev_uv <= vec2<f32>(1.0));

    // velocity in screen-pixels for blend adaptation
    let curr_uv_unjittered = uv - params.jitter * 0.5;
    let velocity_uv        = curr_uv_unjittered - prev_uv;
    let speed_px           = length(velocity_uv) / max(rc.x, rc.y);

    // ── history sample + clip ─────────────────────────────────────────────
    let history_raw  = textureSample(history_tex, linear_smp, prev_uv).rgb;
    let history_ycocg = rgb_to_ycocg(history_raw);
    let clipped_ycocg = clip_aabb(history_ycocg, aabb_min, aabb_max);
    let history_clipped = ycocg_to_rgb(clipped_ycocg);

    // ── taa mask: only touch pixels that need it ──────────────────────────
    // shimmer detection: high luma variance between current and (clipped) history
    let curr_luma = luma(center);
    let hist_luma = luma(history_clipped);
    let temporal_diff = abs(curr_luma - hist_luma) / max(max(curr_luma, hist_luma), 0.01);
    // ramp: zero below 2% diff, full above 7% diff
    let shimmer_weight = saturate((temporal_diff - 0.02) * 20.0);

    // edge detection: luma range in the 3×3 neighborhood (sub-pixel edges shimmer with jitter)
    let luma_min = min(curr_luma, min(min(luma(n), luma(s)), min(luma(e), luma(w))));
    let luma_max = max(curr_luma, max(max(luma(n), luma(s)), max(luma(e), luma(w))));
    let luma_range = luma_max - luma_min;
    // lower threshold than fxaa to catch sub-pixel edges that benefit from jitter AA
    let is_edge = luma_range > max(0.03, luma_max * 0.06);
    // edges get full taa weight (1.0) so effective current-frame contribution is
    // taa_mask × blend_alpha = 1.0 × 0.1 = 10% — proper 8-sample Halton accumulation.
    // at 0.65 the current weight was 41.5% (0.35 + 0.65×0.1), causing visible ~7.5 Hz
    // oscillation at the edge from the cycling jitter offsets.
    let edge_weight = select(0.0, 1.0, is_edge);

    // combined mask: max of shimmer (up to 1.0) and edge (0.65)
    var taa_mask = max(shimmer_weight, edge_weight);

    // disable taa for sky, out-of-frame reprojections, or very fast movement
    // fast movement: the aabb clip handles it, but very high speed means the history
    // is probably completely wrong — disable the mask to pass current through unchanged
    if is_sky || !in_bounds || speed_px > 12.0 {
        taa_mask = 0.0;
    }

    // cold start: no valid history yet, produce output for future frames without blending
    if params.frame_index == 0u {
        taa_mask = 0.0;
    }

    // ── blend within the masked region ────────────────────────────────────
    // alpha controls history vs current WITHIN the taa zone.
    // faster motion → higher alpha (more current frame) → less ghost risk.
    let motion_alpha = min(0.5, speed_px * 0.05);
    let alpha        = max(params.blend_alpha, motion_alpha);

    // taa blend: weighted combination of clipped history and current
    let taa_blend = mix(history_clipped, center, alpha);

    // selective application: smooth surfaces get 0% taa → output = current frame
    let output = mix(center, taa_blend, taa_mask);

    var result: FragOut;
    result.present = vec4<f32>(output, 1.0);
    result.history = vec4<f32>(output, 1.0);
    return result;
}
