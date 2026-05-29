// Gerstner wave water shader — mid-tier forward water rendering.
//
// 4 wave components in the vertex shader: each displaces the mesh surface
// using the Gerstner (trochoidal) wave model. reference: Jerry Tessendorf
// "Simulating Ocean Water" (SIGGRAPH 2004 course notes), simplified to the
// closed-form Gerstner version without FFT.
//
// rendered as a semi-transparent plane with depth write disabled.
// SSR contribution comes from the composite pass (SSR texture).
// simple refraction: sample the HDR buffer at a perturbed UV offset.

struct Globals {
    view_proj:    mat4x4<f32>,
    cam_pos:      vec3<f32>,
    elapsed_secs: f32,
    delta_secs:   f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

struct WaterParams {
    // wave[4]: each vec4 = (direction.x, direction.z, wavelength, amplitude)
    wave0: vec4<f32>,
    wave1: vec4<f32>,
    wave2: vec4<f32>,
    wave3: vec4<f32>,
    // world matrix of the water plane
    model:        mat4x4<f32>,
    water_color:  vec4<f32>,  // shallow colour
    deep_color:   vec4<f32>,  // deep colour (blended by depth)
    refract_strength: f32,
    wave_speed:       f32,
    fresnel_power:    f32,
    screen_width:     f32,
    screen_height:    f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

@group(0) @binding(0) var<uniform> globals:  Globals;
@group(0) @binding(1) var          hdr_tex:  texture_2d<f32>; // HDR buffer for refraction
@group(0) @binding(2) var          lin_smp:  sampler;
@group(1) @binding(0) var<uniform> water:    WaterParams;

struct VertOut {
    @builtin(position) clip_pos:   vec4<f32>,
    @location(0)       world_pos:  vec3<f32>,
    @location(1)       normal:     vec3<f32>,
    @location(2)       view_dir:   vec3<f32>,
}

// Gerstner wave displacement: returns (displacement.xyz, normal.xz)
fn gerstner_wave(pos: vec2<f32>, wave: vec4<f32>, time: f32) -> vec4<f32> {
    let dir = normalize(wave.xy);
    let wavelength = max(wave.z, 0.01);
    let amplitude  = wave.w;
    let k          = 2.0 * 3.14159265 / wavelength;
    let speed      = water.wave_speed;
    let phase      = k * dot(dir, pos) - speed * time;
    let c = cos(phase);
    let s = sin(phase);
    // Gerstner horizontal and vertical displacement
    let dx = amplitude * dir.x * c;
    let dz = amplitude * dir.y * c;
    let dy = amplitude * s;
    // partial normal contribution (negated Gerstner tangent derivative)
    let nx = -k * amplitude * dir.x * s;
    let nz = -k * amplitude * dir.y * s;
    return vec4<f32>(dx, dy, dz, 0.0) + vec4<f32>(nx, 0.0, nz, 0.0) * 0.0; // returns displacement only
    // (normal is reconstructed below)
}

fn gerstner_normal(pos: vec2<f32>, wave: vec4<f32>, time: f32) -> vec2<f32> {
    let dir = normalize(wave.xy);
    let wavelength = max(wave.z, 0.01);
    let amplitude  = wave.w;
    let k          = 2.0 * 3.14159265 / wavelength;
    let speed      = water.wave_speed;
    let phase      = k * dot(dir, pos) - speed * time;
    let s = sin(phase);
    return vec2<f32>(-k * amplitude * dir.x * s, -k * amplitude * dir.y * s);
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) _normal:  vec3<f32>,
    @location(2) _color:   vec4<f32>,
    @location(3) _uv0:     vec2<f32>,
    @location(4) _uv1:     vec2<f32>,
    @location(5) _tint:    vec4<f32>,
) -> VertOut {
    var world_pos = (water.model * vec4<f32>(position, 1.0)).xyz;
    let xz = world_pos.xz;
    let t  = globals.elapsed_secs;

    // accumulate Gerstner wave displacement
    let d0 = gerstner_wave(xz, water.wave0, t);
    let d1 = gerstner_wave(xz, water.wave1, t);
    let d2 = gerstner_wave(xz, water.wave2, t);
    let d3 = gerstner_wave(xz, water.wave3, t);
    world_pos += d0.xyz + d1.xyz + d2.xyz + d3.xyz;

    // accumulate normal perturbations
    let n0 = gerstner_normal(xz, water.wave0, t);
    let n1 = gerstner_normal(xz, water.wave1, t);
    let n2 = gerstner_normal(xz, water.wave2, t);
    let n3 = gerstner_normal(xz, water.wave3, t);
    let nx = n0.x + n1.x + n2.x + n3.x;
    let nz = n0.y + n1.y + n2.y + n3.y;
    let world_normal = normalize(vec3<f32>(nx, 1.0, nz));

    var out: VertOut;
    out.clip_pos  = globals.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_pos = world_pos;
    out.normal    = world_normal;
    out.view_dir  = normalize(globals.cam_pos - world_pos);
    return out;
}

// schlick fresnel approximation
fn fresnel(normal: vec3<f32>, view: vec3<f32>, power: f32) -> f32 {
    let f0 = 0.02; // water IOR ≈ 1.33 → ((1-1.33)/(1+1.33))² ≈ 0.02
    let cos_theta = max(dot(normal, view), 0.0);
    return f0 + (1.0 - f0) * pow(1.0 - cos_theta, power);
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(in.view_dir);

    // screen UV for refraction sampling
    let screen_uv = in.clip_pos.xy / vec2<f32>(water.screen_width, water.screen_height);
    // perturb UVs by surface normal for cheap refraction
    let refract_uv = screen_uv + n.xz * water.refract_strength * 0.02;
    let refract_color = textureSample(hdr_tex, lin_smp, clamp(refract_uv, vec2<f32>(0.001), vec2<f32>(0.999))).rgb;

    // depth-based water colour blend (shallow near surface, deep further down)
    let depth_t = clamp(abs(in.world_pos.y) * 0.1, 0.0, 1.0);
    let water_tint = mix(water.water_color.rgb, water.deep_color.rgb, depth_t);

    // fresnel blend: refraction when looking straight down, reflection tint at glancing angles
    let f = fresnel(n, v, water.fresnel_power);
    let surface_color = mix(refract_color * water_tint, water_tint, f);

    // alpha: translucent at nadir, opaque at horizon
    let alpha = mix(0.5, 0.95, f) * water.water_color.a;

    return vec4<f32>(surface_color, alpha);
}
