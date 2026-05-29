// volumetric fog — half-resolution ray-marched sun scattering.
//
// per Bart Wronski SIGGRAPH 2014 "Volumetric Fog: Unified, Compute Shader Based
// Solution to Atmospheric Scattering": march N steps along the view ray from
// camera to scene depth, accumulate directional + ambient in-scattering.
// output: rgba16float (in-scatter rgb, 1-transmittance) blended in composite.
//
// depth read via textureLoad — no sampler required for the depth texture.

struct Globals {
    view_proj:    mat4x4<f32>,
    cam_pos:      vec3<f32>,
    elapsed_secs: f32,
    delta_secs:   f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

struct FogParams {
    inv_view_proj: mat4x4<f32>,  // 64 bytes
    dir_direction: vec3<f32>,    // normalised direction towards the sun
    step_count:    u32,
    dir_color:     vec3<f32>,
    density:       f32,
    fog_color:     vec3<f32>,
    max_distance:  f32,
    sun_intensity: f32,
    anisotropy:    f32,
    screen_width:  f32,
    screen_height: f32,
}

@group(0) @binding(0) var<uniform> globals:   Globals;
@group(0) @binding(1) var depth_tex: texture_2d<f32>;
@group(1) @binding(0) var<uniform>  fog:      FogParams;

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
    let px = vec2<i32>(i32(uv.x * fog.screen_width), i32(uv.y * fog.screen_height));
    return textureLoad(depth_tex, px, 0).r;
}

fn reconstruct_world(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv * 2.0 - 1.0, depth, 1.0);
    let world_h = fog.inv_view_proj * ndc;
    return world_h.xyz / world_h.w;
}

fn hg_phase(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * 3.14159265 * pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5));
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let uv        = in.uv;
    let depth     = load_depth(uv);
    let scene_pos = reconstruct_world(uv, select(1.0, depth, depth < 1.0));
    let cam_pos   = globals.cam_pos;
    let ray_dir   = normalize(scene_pos - cam_pos);
    let raw_len   = length(scene_pos - cam_pos);
    let ray_length = min(select(fog.max_distance, raw_len, depth < 1.0), fog.max_distance);

    let step_size = ray_length / f32(fog.step_count);
    let sun_dir   = normalize(fog.dir_direction);
    let cos_theta = dot(ray_dir, sun_dir);
    let phase     = hg_phase(cos_theta, fog.anisotropy);

    var transmittance = 1.0;
    var in_scatter    = vec3<f32>(0.0);

    for (var i = 0u; i < fog.step_count; i++) {
        let sigma      = fog.density;
        let extinction = sigma * step_size;
        let sun_s      = fog.dir_color * fog.sun_intensity * phase * sigma * step_size;
        let amb_s      = fog.fog_color * sigma * step_size;
        in_scatter    += (sun_s + amb_s) * transmittance;
        transmittance *= exp(-extinction);
    }

    return vec4<f32>(in_scatter, 1.0 - transmittance);
}
