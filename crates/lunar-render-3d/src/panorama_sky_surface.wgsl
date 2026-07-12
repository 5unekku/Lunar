// sky surface: real world geometry (sky ceilings, sky-to-sky upper walls) shaded
// with the SAME screen-space panorama mapping as the fullscreen sky, but drawn as
// ordinary opaque geometry that writes real depth. that depth write is the whole
// point: the surface occludes anything behind it, reproducing a software
// renderer's sky planes that hide the geometry beyond them instead of leaving a
// see-through hole. the vertical mapping is screen-linear (matching the fullscreen
// pass), so a sky ceiling meets open sky with no seam. shares the panorama params
// and texture bind group. see panorama_sky_scene.wgsl for the mapping rationale.

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
    _vp_x: f32, _vp_y: f32, _vp_w: f32, _vp_h: f32,  // unused here; shared layout
}

@group(0) @binding(0) var<uniform> globals:     Globals;
@group(1) @binding(0) var<uniform> params:      PanoramaParams;
@group(1) @binding(1) var          sky_tex:     texture_2d<f32>;
@group(1) @binding(2) var          sky_sampler: sampler;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    // the clip position passed through as a varying: dividing by w in the
    // fragment recovers this pixel's true ndc, independent of the framebuffer's
    // y orientation (wgpu flips y on vulkan), so surface sky and the fullscreen
    // sky stay pixel-identical
    @location(0) vclip: vec4<f32>,
}

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VertOut {
    var out: VertOut;
    // geometry is authored in world space (identity model), so view_proj alone
    // places it and rasterizes its true depth
    let clip = globals.view_proj * vec4<f32>(position, 1.0);
    out.clip_pos = clip;
    out.vclip = clip;
    return out;
}

const TAU: f32 = 6.28318530717958;

// average of one full texture repeat along an edge row: the fill color for
// pixels past the texture's vertical coverage (steep look-up/down)
fn edge_average(v_edge: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < 16u; i++) {
        sum += textureSampleLevel(sky_tex, sky_sampler, vec2<f32>(f32(i) / 16.0, v_edge), 0.0).rgb;
    }
    return sum / 16.0;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // recover this fragment's clip-space ndc, then match the fullscreen sky's
    // convention where its `ndc` is (ndc.x, -ndc.y). purely in clip space, so the
    // viewport's y-flip (vulkan) never enters and the two skies agree per pixel.
    let cndc = in.vclip.xy / in.vclip.w;
    let ndc = vec2<f32>(cndc.x, -cndc.y);
    let row_x = vec3<f32>(globals.view_proj[0][0], globals.view_proj[1][0], globals.view_proj[2][0]);
    let row_y = vec3<f32>(globals.view_proj[0][1], globals.view_proj[1][1], globals.view_proj[2][1]);
    let fwd   = vec3<f32>(globals.view_proj[0][3], globals.view_proj[1][3], globals.view_proj[2][3]);
    let tan_half_fov_x = 1.0 / length(row_x);
    let tan_half_fov_y = 1.0 / length(row_y);

    let cam_yaw = atan2(-fwd.z, fwd.x);
    let yaw = cam_yaw - atan(ndc.x * tan_half_fov_x);
    let tan_pitch = fwd.y / max(length(fwd.xz), 1e-4) - ndc.y * tan_half_fov_y;

    let u = yaw / TAU * params.repeats;
    let v_raw = params.v_offset - tan_pitch * params.tan_scale;
    let v = clamp(v_raw, 0.001, 0.999);
    var color = textureSampleLevel(sky_tex, sky_sampler, vec2<f32>(u, v), 0.0).rgb;

    let overshoot = max(-v_raw, v_raw - 1.0);
    if overshoot > 0.0 {
        color = mix(color, edge_average(v), clamp(overshoot * 4.0, 0.0, 1.0));
    }
    return vec4<f32>(color, 1.0);
}
