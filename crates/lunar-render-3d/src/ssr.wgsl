// screen-space reflections — half-resolution ray march over the HDR buffer.
//
// mid+ tier only. reads scene depth via textureLoad (no sampler needed for depth),
// then marches the reflected ray in clip space, sampling the HDR color on hit.
// output: rgba16float (color×alpha, alpha) blended into composite.

struct Globals {
    view_proj:    mat4x4<f32>,
    cam_pos:      vec3<f32>,
    elapsed_secs: f32,
    delta_secs:   f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

struct SsrParams {
    inv_view_proj: mat4x4<f32>,  // 64 bytes
    proj:          mat4x4<f32>,  // 64 bytes
    view:          mat4x4<f32>,  // 64 bytes
    screen_size:   vec2<f32>,    // full-res pixel dimensions (w, h)
    max_steps:     u32,
    thickness:     f32,
    stride:        f32,
    fade_start:    f32,
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> globals:    Globals;
@group(0) @binding(1) var hdr_tex:   texture_2d<f32>;
@group(0) @binding(2) var depth_tex: texture_2d<f32>;
@group(0) @binding(3) var lin_smp:   sampler;
@group(1) @binding(0) var<uniform>  ssr:       SsrParams;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
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

fn load_depth(uv: vec2<f32>) -> f32 {
    let px = vec2<i32>(i32(uv.x * ssr.screen_size.x), i32(uv.y * ssr.screen_size.y));
    return textureLoad(depth_tex, px, 0).r;
}

fn reconstruct_world(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv * 2.0 - 1.0, depth, 1.0);
    let world_h = ssr.inv_view_proj * ndc;
    return world_h.xyz / world_h.w;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let uv    = in.uv;
    let depth = load_depth(uv);
    if depth >= 1.0 { return vec4<f32>(0.0); }

    let world_pos = reconstruct_world(uv, depth);
    let view_dir  = normalize(world_pos - globals.cam_pos);

    let dpdx = dpdx(world_pos);
    let dpdy = dpdy(world_pos);
    let n    = normalize(cross(dpdx, dpdy));
    let ndotv = dot(n, -view_dir);
    if ndotv < 0.1 { return vec4<f32>(0.0); }

    let refl_dir = reflect(view_dir, n);
    var ray_pos  = world_pos + refl_dir * 0.05;

    for (var i = 0u; i < ssr.max_steps; i++) {
        ray_pos += refl_dir * (ssr.stride * f32(i + 1u) * 0.01);

        let clip   = globals.view_proj * vec4<f32>(ray_pos, 1.0);
        let ray_uv = clip.xy / clip.w * 0.5 + 0.5;
        if any(ray_uv < vec2<f32>(0.0)) || any(ray_uv > vec2<f32>(1.0)) { break; }

        let scene_depth = load_depth(ray_uv);
        let scene_world = reconstruct_world(ray_uv, scene_depth);
        let diff = length(ray_pos - globals.cam_pos) - length(scene_world - globals.cam_pos);

        if diff > 0.0 && diff < ssr.thickness {
            let color      = textureSample(hdr_tex, lin_smp, ray_uv).rgb;
            let edge_fade  = min(min(ray_uv.x, 1.0 - ray_uv.x), min(ray_uv.y, 1.0 - ray_uv.y)) / ssr.fade_start;
            let len_fade   = clamp(1.0 - length(ray_pos - world_pos) * 0.05, 0.0, 1.0);
            let angle_fade = clamp(ndotv * 2.0, 0.0, 1.0);
            let alpha      = clamp(edge_fade, 0.0, 1.0) * len_fade * angle_fade;
            return vec4<f32>(color * alpha, alpha);
        }
    }
    return vec4<f32>(0.0);
}
