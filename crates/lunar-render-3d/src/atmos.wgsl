// atmospheric scattering sky — Nishita-style single-scattering Rayleigh + Mie.
//
// replaces the flat-color dome on mid+ tier when AtmosphericScattering resource
// is present. rendered as an additive fullscreen pass over the cleared framebuffer
// (before opaque geometry, after depth prepass) with depth write disabled.
//
// reference: Nishita et al. 1993 "Display of the Earth taking into account
// atmospheric scattering"; simplified for real-time per Preetham 1999 and
// Hillaire 2020 "A Scalable and Production Ready Sky and Atmosphere Rendering
// Technique" (simplified single-scattering path only — no multi-scatter LUTs).

struct AtmosParams {
    sun_direction:    vec3<f32>,  // normalised direction towards sun
    sun_intensity:    f32,
    rayleigh_scatter: vec3<f32>,  // Rayleigh scattering coefficients per RGB (m^-1)
    mie_scatter:      f32,        // Mie scattering coefficient (m^-1)
    rayleigh_scale:   f32,        // Rayleigh scale height (m)
    mie_scale:        f32,        // Mie scale height (m)
    mie_anisotropy:   f32,        // Henyey-Greenstein g for Mie (0.76 typical)
    planet_radius:    f32,        // planet radius (m), 6371e3 for Earth
    atmos_radius:     f32,        // atmosphere radius (m), 6471e3 for Earth
    exposure:         f32,
    _pad0: f32,
    _pad1: f32,
}

struct Globals {
    view_proj:    mat4x4<f32>,
    cam_pos:      vec3<f32>,
    elapsed_secs: f32,
    delta_secs:   f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

@group(0) @binding(0) var<uniform> globals:    Globals;
@group(0) @binding(1) var           depth_tex: texture_2d<f32>;
@group(1) @binding(0) var<uniform> atmos:      AtmosParams;

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

const PI: f32 = 3.14159265358979;

// phase functions
fn rayleigh_phase(cos_theta: f32) -> f32 {
    return (3.0 / (16.0 * PI)) * (1.0 + cos_theta * cos_theta);
}
fn mie_phase(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (3.0 / (8.0 * PI)) * ((1.0 - g2) * (1.0 + cos_theta * cos_theta))
         / ((2.0 + g2) * pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5));
}

// ray-sphere intersection — returns (t_near, t_far), negative if no hit
fn ray_sphere(origin: vec3<f32>, dir: vec3<f32>, radius: f32) -> vec2<f32> {
    let a = dot(dir, dir);
    let b = 2.0 * dot(origin, dir);
    let c = dot(origin, origin) - radius * radius;
    let d = b * b - 4.0 * a * c;
    if d < 0.0 { return vec2<f32>(-1.0, -1.0); }
    let sq = sqrt(d);
    return vec2<f32>((-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a));
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // skip pixels already covered by geometry (depth < 1.0 = something was rendered there)
    let px = vec2<i32>(i32(in.clip_pos.x), i32(in.clip_pos.y));
    let depth = textureLoad(depth_tex, px, 0).r;
    if depth < 1.0 { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }

    // reconstruct the world-space view ray from view_proj rows. row 3 is the
    // unit forward axis (projection row 3 is (0,0,-1,0), which negates the view
    // matrix's z row); rows 0/1 are right*sx and up*sy, so dividing by their
    // squared length normalizes AND applies tan(half-fov) = 1/s in one step.
    // never read fov from a single element like vp[0][0] — that is sx*right.x,
    // which varies (and flips sign) with camera yaw
    let ndc = in.uv * 2.0 - 1.0;
    let row_x = vec3<f32>(globals.view_proj[0][0], globals.view_proj[1][0], globals.view_proj[2][0]);
    let row_y = vec3<f32>(globals.view_proj[0][1], globals.view_proj[1][1], globals.view_proj[2][1]);
    let fwd   = vec3<f32>(globals.view_proj[0][3], globals.view_proj[1][3], globals.view_proj[2][3]);
    let ray_view = normalize(fwd
        + row_x * (ndc.x / dot(row_x, row_x))
        + row_y * (-ndc.y / dot(row_y, row_y)));

    // camera position — offset to planet surface (assume cam is near planet centre)
    let planet_r = atmos.planet_radius;
    let atmos_r  = atmos.atmos_radius;
    // place camera above planet surface
    let cam_world = vec3<f32>(0.0, planet_r + max(globals.cam_pos.y, 0.0) * 0.001 + 1.0, 0.0);

    let ray_dir = ray_view;

    // intersect ray with atmosphere
    let t_atmos = ray_sphere(cam_world, ray_dir, atmos_r);
    if t_atmos.y < 0.0 { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }

    let t_start = max(t_atmos.x, 0.0);
    let t_end   = t_atmos.y;

    // integrate along the view ray (16 steps)
    let num_steps = 16u;
    let step_size = (t_end - t_start) / f32(num_steps);

    var rayleigh_sum = vec3<f32>(0.0);
    var mie_sum      = vec3<f32>(0.0);
    var optical_depth_r = 0.0;
    var optical_depth_m = 0.0;

    let sun_dir = normalize(atmos.sun_direction);
    let cos_theta = dot(ray_dir, sun_dir);
    let phase_r = rayleigh_phase(cos_theta);
    let phase_m = mie_phase(cos_theta, atmos.mie_anisotropy);

    for (var i = 0u; i < num_steps; i++) {
        let t = t_start + (f32(i) + 0.5) * step_size;
        let sample_pos = cam_world + ray_dir * t;

        let height = length(sample_pos) - planet_r;
        let hr = exp(-height / atmos.rayleigh_scale) * step_size;
        let hm = exp(-height / atmos.mie_scale)      * step_size;
        optical_depth_r += hr;
        optical_depth_m += hm;

        // transmittance from sun to this sample point (approximate: assume clear path)
        let t_sun = ray_sphere(sample_pos, sun_dir, atmos_r);
        let sun_steps = 8u;
        var sun_od_r = 0.0;
        var sun_od_m = 0.0;
        if t_sun.y > 0.0 {
            let sun_step = t_sun.y / f32(sun_steps);
            for (var j = 0u; j < sun_steps; j++) {
                let sp = sample_pos + sun_dir * ((f32(j) + 0.5) * sun_step);
                let sh = length(sp) - planet_r;
                sun_od_r += exp(-sh / atmos.rayleigh_scale) * sun_step;
                sun_od_m += exp(-sh / atmos.mie_scale)      * sun_step;
            }
        }

        let tau = atmos.rayleigh_scatter * (optical_depth_r + sun_od_r)
                + vec3<f32>(atmos.mie_scatter * 1.1) * (optical_depth_m + sun_od_m);
        let transmittance = exp(-tau);

        rayleigh_sum += transmittance * hr;
        mie_sum      += transmittance * hm;
    }

    let color = atmos.sun_intensity * (
        rayleigh_sum * atmos.rayleigh_scatter * phase_r +
        mie_sum      * atmos.mie_scatter      * phase_m
    );

    // simple Reinhard tone map — outputs into HDR target (composite will ACES the whole frame)
    let mapped = 1.0 - exp(-color * atmos.exposure);
    return vec4<f32>(mapped, 1.0);
}
