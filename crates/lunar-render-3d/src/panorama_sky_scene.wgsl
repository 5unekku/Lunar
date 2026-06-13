// cylindrical panorama sky, drawn inside the main color pass in place of the
// sky dome. the triangle sits at far depth (LessEqual, write off) and draws
// after the opaque section, so early-z skips every geometry-covered pixel and
// only visible sky shades; being in the main pass still lets msaa edges
// resolve against real sky texels instead of the clear color. the texture
// tiles horizontally `repeats` times per full turn and maps linearly in
// tan(pitch) vertically, matching a software renderer's screen-linear sky
// columns at any fov. colors pass through untouched (gamma-space pipeline).

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
    // z = w → far-plane depth 1.0: passes LessEqual only where no geometry wrote
    out.clip_pos = vec4<f32>(pos[vi], 1.0, 1.0);
    out.uv = uvs[vi];
    return out;
}

const TAU: f32 = 6.28318530717958;

// average of one full texture repeat along an edge row — the fill color for
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
    // camera basis from view_proj rows: row 3 is the unit forward axis
    // (projection row 3 is (0,0,-1,0), which negates the view matrix's z row)
    // and the lengths of rows 0/1 are the projection scales, so tan(half-fov)
    // is their reciprocal. never read fov from a single element like vp[0][0]
    // — that is sx*right.x, which varies (and flips sign) with camera yaw
    let ndc = in.uv * 2.0 - 1.0;
    let row_x = vec3<f32>(globals.view_proj[0][0], globals.view_proj[1][0], globals.view_proj[2][0]);
    let row_y = vec3<f32>(globals.view_proj[0][1], globals.view_proj[1][1], globals.view_proj[2][1]);
    let fwd   = vec3<f32>(globals.view_proj[0][3], globals.view_proj[1][3], globals.view_proj[2][3]);
    let tan_half_fov_x = 1.0 / length(row_x);
    let tan_half_fov_y = 1.0 / length(row_y);

    // software-renderer mapping, not a true cylinder: doom pans the sky per
    // screen COLUMN (u from view yaw + column angle) and shears it per screen
    // ROW (v linear in screen y, offset by tan of the look pitch), so cloud
    // bands stay straight and sky columns stay vertical at any pitch. mapping
    // through the actual view ray instead bows the bands into arcs once the
    // camera pitches. ndc.y is negated: screen top = -1 = looking up
    let cam_yaw = atan2(-fwd.z, fwd.x);
    let yaw = cam_yaw - atan(ndc.x * tan_half_fov_x);
    let tan_pitch = fwd.y / max(length(fwd.xz), 1e-4) - ndc.y * tan_half_fov_y;

    let u = yaw / TAU * params.repeats;
    let v_raw = params.v_offset - tan_pitch * params.tan_scale;
    let v = clamp(v_raw, 0.001, 0.999);
    var color = textureSampleLevel(sky_tex, sky_sampler, vec2<f32>(u, v), 0.0).rgb;

    // doom skies only cover a limited pitch band; past it fade to the average
    // edge-row color so the zenith reads as solid haze, not smeared texels
    let overshoot = max(-v_raw, v_raw - 1.0);
    if overshoot > 0.0 {
        color = mix(color, edge_average(v), clamp(overshoot * 4.0, 0.0, 1.0));
    }
    return vec4<f32>(color, 1.0);
}
