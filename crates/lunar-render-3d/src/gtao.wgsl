// ground truth ambient occlusion (GTAO-style)
// simplified horizon-based AO inspired by XeGTAO (Intel, 2021)
// reference: "XeGTAO: practical realtime AO using a horizon-based formulation"
//
// pass 1 (gtao): half-res AO from depth + normals
// pass 2 (blur_h): horizontal 5-tap bilateral filter
// pass 3 (blur_v): vertical 5-tap bilateral filter + blend into composite
//
// all fullscreen passes use the standard triangle vertex shader.

struct GtaoParams {
    inv_proj:     mat4x4<f32>,    // inverse projection to reconstruct view-space pos
    proj:         mat4x4<f32>,    // projection (for reprojecting samples)
    noise_seed:   f32,            // per-frame noise seed (elapsed_secs)
    radius:       f32,            // world-space AO radius (metres)
    falloff_end:  f32,            // distance beyond which contribution fades to zero
    slice_count:  f32,            // number of AO slices (directions); 3 for mid, 5 for high
    step_count:   f32,            // steps per slice; 4 for mid, 6 for high
    half_res_w:   f32,            // half-res width  (for texel size)
    half_res_h:   f32,            // half-res height
    _pad:         f32,
}
@group(0) @binding(0) var<uniform> params: GtaoParams;
@group(0) @binding(1) var depth_tex:  texture_2d<f32>;
@group(0) @binding(2) var ao_src:     texture_2d<f32>;   // AO input for blur passes
@group(0) @binding(3) var smp:        sampler;
@group(0) @binding(4) var smp_point:  sampler;

// ── fullscreen vertex ──────────────────────────────────────────────────────

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

// ── helpers ────────────────────────────────────────────────────────────────

fn reconstruct_view_pos(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv * 2.0 - 1.0, depth, 1.0);
    let view4 = params.inv_proj * ndc;
    return view4.xyz / view4.w;
}

fn sample_depth(uv: vec2<f32>) -> f32 {
    return textureSample(depth_tex, smp_point, uv).r;
}

// low-discrepancy hash for spatial noise (avoids structured banding)
fn interleaved_gradient_noise(pixel: vec2<f32>, seed: f32) -> f32 {
    let magic = vec3<f32>(0.06711056, 0.00583715, 52.9829189);
    return fract(magic.z * fract(dot(pixel + seed * 5.588238, magic.xy)));
}

// ── main GTAO pass ─────────────────────────────────────────────────────────

@fragment
fn fs_gtao(in: VertOut) -> @location(0) vec4<f32> {
    let pixel = in.clip_pos.xy;
    let uv    = in.uv;

    let depth = sample_depth(uv);
    // skip sky / background (depth = 1 in no-reverse-z or 0 in reverse-z)
    if depth >= 0.9999 {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }

    let p_vs  = reconstruct_view_pos(uv, depth);
    let dist  = length(p_vs);

    // reconstruct view-space normal from depth derivatives (no G-buffer needed)
    let texel = vec2<f32>(1.0 / params.half_res_w, 1.0 / params.half_res_h);
    let p_r = reconstruct_view_pos(uv + vec2<f32>(texel.x, 0.0), sample_depth(uv + vec2<f32>(texel.x, 0.0)));
    let p_u = reconstruct_view_pos(uv + vec2<f32>(0.0, texel.y), sample_depth(uv + vec2<f32>(0.0, texel.y)));
    let n_vs = normalize(cross(p_r - p_vs, p_u - p_vs));

    let slice_count = max(i32(params.slice_count), 1);
    let step_count  = max(i32(params.step_count), 1);
    let radius_px   = params.radius * (params.half_res_w / (2.0 * dist));  // project radius to pixels
    let step_px     = max(radius_px / f32(step_count), 1.5);

    let noise = interleaved_gradient_noise(pixel, params.noise_seed);

    var ao_sum = 0.0;

    for (var si = 0; si < slice_count; si++) {
        let angle = (f32(si) + noise) * 3.14159265 / f32(slice_count);
        let dir = vec2<f32>(cos(angle), sin(angle));

        // find max horizon angle in +dir and -dir
        var max_horizon = -1.0;  // starts at -1 (cos(π))

        for (var step = 1; step <= step_count; step++) {
            let offset = dir * f32(step) * step_px * texel;

            // sample in both directions
            for (var sign = -1; sign <= 1; sign += 2) {
                let s_uv  = uv + offset * f32(sign);
                if s_uv.x < 0.0 || s_uv.x > 1.0 || s_uv.y < 0.0 || s_uv.y > 1.0 { continue; }
                let s_d   = sample_depth(s_uv);
                let s_vs  = reconstruct_view_pos(s_uv, s_d);
                let delta = s_vs - p_vs;
                let delta_len = length(delta);

                // falloff: samples far away contribute less
                let falloff = clamp(1.0 - delta_len / params.falloff_end, 0.0, 1.0);
                if falloff <= 0.0 { continue; }

                let horizon = dot(n_vs, normalize(delta)) * falloff;
                max_horizon = max(max_horizon, horizon);
            }
        }

        // AO contribution for this slice: 1 - occluded fraction
        // integrate: visibility = 1 - max(0, max_horizon)
        ao_sum += 1.0 - max(0.0, max_horizon);
    }

    let ao = clamp(ao_sum / f32(slice_count), 0.0, 1.0);
    return vec4<f32>(ao, depth, 0.0, 1.0);
}

// ── bilateral blur (horizontal) ────────────────────────────────────────────
// preserves depth edges by weighting by depth similarity.

fn bilateral_weight(center_depth: f32, sample_depth: f32) -> f32 {
    let diff = abs(center_depth - sample_depth);
    // tight sigma in depth: reject samples with large depth discontinuity
    return exp(-diff * 10.0);
}

@fragment
fn fs_blur_h(in: VertOut) -> @location(0) vec4<f32> {
    let texel_w = 1.0 / params.half_res_w;
    let center  = textureSample(ao_src, smp_point, in.uv);
    let cd = center.g;  // depth stored in G channel

    var ao_acc = center.r;
    var weight = 1.0;

    let offsets = array<f32, 4>(-2.0, -1.0, 1.0, 2.0);
    let gauss   = array<f32, 4>(0.25, 0.5, 0.5, 0.25);

    for (var i = 0; i < 4; i++) {
        let s_uv = in.uv + vec2<f32>(offsets[i] * texel_w, 0.0);
        let s    = textureSample(ao_src, smp_point, s_uv);
        let w    = gauss[i] * bilateral_weight(cd, s.g);
        ao_acc  += s.r * w;
        weight  += w;
    }

    return vec4<f32>(ao_acc / weight, cd, 0.0, 1.0);
}

// ── bilateral blur (vertical) ──────────────────────────────────────────────

@fragment
fn fs_blur_v(in: VertOut) -> @location(0) vec4<f32> {
    let texel_h = 1.0 / params.half_res_h;
    let center  = textureSample(ao_src, smp_point, in.uv);
    let cd = center.g;

    var ao_acc = center.r;
    var weight = 1.0;

    let offsets = array<f32, 4>(-2.0, -1.0, 1.0, 2.0);
    let gauss   = array<f32, 4>(0.25, 0.5, 0.5, 0.25);

    for (var i = 0; i < 4; i++) {
        let s_uv = in.uv + vec2<f32>(0.0, offsets[i] * texel_h);
        let s    = textureSample(ao_src, smp_point, s_uv);
        let w    = gauss[i] * bilateral_weight(cd, s.g);
        ao_acc  += s.r * w;
        weight  += w;
    }

    // final output: just the AO value in R (G/B unused after blur)
    return vec4<f32>(ao_acc / weight, 0.0, 0.0, 1.0);
}
