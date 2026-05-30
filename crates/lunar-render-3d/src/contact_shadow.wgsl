// screen-space contact shadow pass.
//
// quarter-resolution fullscreen pass. for each pixel, marches a short ray
// from the fragment toward the directional light in view space and compares
// the marched depth against the depth buffer. if the ray is occluded,
// outputs a shadow factor in [0, 1].
//
// applied in the composite pass as a darkening on top of the main HDR color.
// fills the "floating object" gap where the shadow map texel size is too coarse
// to capture contact shadows at the surface.

struct ContactShadowParams {
    inv_proj:       mat4x4<f32>,  // 64 bytes
    light_dir_vs:   vec3<f32>,   // view-space light direction (toward light, normalized)
    step_count:     u32,          // number of ray march steps (default 8)
    step_size:      f32,          // world-space step size per step (default 0.08)
    screen_width:   f32,
    screen_height:  f32,
    _pad:           f32,
}

@group(0) @binding(0) var<uniform> params:     ContactShadowParams;
@group(0) @binding(1) var          depth_tex:  texture_depth_2d;
@group(0) @binding(2) var          depth_smp:  sampler;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    let pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0), vec2<f32>(-1.0, 1.0), vec2<f32>(3.0, 1.0),
    );
    let uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0), vec2<f32>(0.0, 0.0), vec2<f32>(2.0, 0.0),
    );
    var out: VertOut;
    out.clip_pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uvs[vi];
    return out;
}

// reconstruct view-space position from depth and screen UV
fn view_pos_from_depth(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, depth, 1.0);
    let view4 = params.inv_proj * ndc;
    return view4.xyz / view4.w;
}

// project a view-space position back to screen UV
fn project_to_uv(view_pos: vec3<f32>) -> vec2<f32> {
    // approximate: assume orthographic-ish forward projection for screen UV
    // more correct: need proj matrix here, but inv_proj inverse is expensive.
    // use the ratio trick: view_pos.xy / (-view_pos.z) gives clip-space estimate.
    let clip_xy = view_pos.xy / max(-view_pos.z, 0.001);
    return clip_xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
}

@fragment
fn fs_main(in: VertOut) -> @location(0) f32 {
    let depth = textureSample(depth_tex, depth_smp, in.uv);
    // skip sky (depth == 1.0 in reversed-Z or 0.0 in regular Z — use near-to-far convention)
    if depth >= 0.9999 { return 0.0; }

    let view_pos   = view_pos_from_depth(in.uv, depth);
    let step_size  = params.step_size;
    let step_count = params.step_count;

    var occlusion = 0.0;
    var ray_pos   = view_pos + params.light_dir_vs * step_size;  // start one step away

    for (var i = 0u; i < step_count; i++) {
        let sample_uv = project_to_uv(ray_pos);
        if any(sample_uv < vec2<f32>(0.0)) || any(sample_uv > vec2<f32>(1.0)) { break; }

        let sample_depth = textureSample(depth_tex, depth_smp, sample_uv);
        let sample_vpos  = view_pos_from_depth(sample_uv, sample_depth);

        // the ray is occluded if the surface it hits is behind the ray position
        // (closer to camera) by a small tolerance to avoid self-shadowing
        let ray_depth    = -ray_pos.z;
        let surface_depth = -sample_vpos.z;
        if surface_depth < ray_depth - 0.02 && surface_depth > ray_depth - 0.5 {
            occlusion = 1.0 - f32(i) / f32(step_count) * 0.4;  // fade with step index
            break;
        }
        ray_pos += params.light_dir_vs * step_size;
    }

    return clamp(occlusion, 0.0, 1.0);
}
