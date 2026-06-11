// cylindrical panorama sky, drawn inside the main color pass in place of the
// sky dome (depth Always, write off): geometry then draws over it, so msaa
// edges resolve against real sky texels instead of the clear color, and no
// post-resolve depth classification is needed. the texture tiles horizontally
// `repeats` times per full turn and maps linearly in tan(pitch) vertically,
// matching a software renderer's screen-linear sky columns at any fov.
// colors pass through untouched (the pipeline is gamma space).

struct Globals {
    view_proj:      mat4x4<f32>,
    cam_pos:        vec3<f32>,
    elapsed_secs:   f32,
    delta_secs:     f32,
    lighting_model: u32,
    render_flags:   u32,
    vertex_snap:    f32,
    classic_light:  f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

struct PanoramaParams {
    repeats:   f32,  // horizontal texture repeats per 360°
    tan_scale: f32,  // v advance per unit tan(pitch)
    v_offset:  f32,  // v at the horizon
    _pad0:     f32,
}

@group(0) @binding(0) var<uniform> globals:     Globals;
@group(1) @binding(0) var<uniform> params:      PanoramaParams;
@group(1) @binding(1) var          sky_tex:     texture_2d<f32>;
@group(1) @binding(2) var          sky_sampler: sampler;

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

const TAU: f32 = 6.28318530717958;

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // reconstruct the world-space view ray (same basis trick as atmos.wgsl)
    let ndc = in.uv * 2.0 - 1.0;
    let right = vec3<f32>(globals.view_proj[0][0], globals.view_proj[1][0], globals.view_proj[2][0]);
    let up    = vec3<f32>(globals.view_proj[0][1], globals.view_proj[1][1], globals.view_proj[2][1]);
    let fwd   = vec3<f32>(-globals.view_proj[0][2], -globals.view_proj[1][2], -globals.view_proj[2][2]);
    let tan_fov_x = 1.0 / globals.view_proj[0][0];
    let tan_fov_y = 1.0 / globals.view_proj[1][1];
    let ray = normalize(fwd + right * ndc.x * tan_fov_x + up * (-ndc.y) * tan_fov_y);

    // cylinder mapping: yaw → u, tan(pitch) → v. v is clamped, not wrapped,
    // so steep look-up/down smears the texture edge instead of re-tiling it
    let yaw = atan2(-ray.z, ray.x);
    let horizontal = max(length(ray.xz), 1e-4);
    let tan_pitch = ray.y / horizontal;
    let u = yaw / TAU * params.repeats;
    let v = clamp(params.v_offset - tan_pitch * params.tan_scale, 0.001, 0.999);

    let color = textureSampleLevel(sky_tex, sky_sampler, vec2<f32>(u, v), 0.0);
    return vec4<f32>(color.rgb, 1.0);
}
