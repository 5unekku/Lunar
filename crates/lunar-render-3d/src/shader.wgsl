// group 0: view-global — set once per pass
struct Globals {
    view_proj:    mat4x4<f32>,  // 64 bytes
    cam_pos:      vec3<f32>,    // 12 bytes (offset 64)
    elapsed_secs: f32,          //  4 bytes (offset 76)
    delta_secs:   f32,          //  4 bytes (offset 80)
    _pad0:        f32,          //  4 bytes
    _pad1:        f32,          //  4 bytes
    _pad2:        f32,          //  4 bytes — total: 96 bytes
}
@group(0) @binding(0) var<uniform> globals: Globals;

// group 1: material — dynamic offset
struct MaterialUniforms {
    base_color: vec4<f32>,  // 16 bytes
    metallic:   f32,         //  4 bytes
    roughness:  f32,         //  4 bytes
    flags:      u32,         //  4 bytes  (bit 0 = unlit)
    _pad:       f32,         //  4 bytes — total: 32 bytes
}
@group(1) @binding(0) var<uniform> material: MaterialUniforms;

// group 2: per-mesh — dynamic offset
struct MeshUniforms {
    model:     mat4x4<f32>,  // 64 bytes
    normal_c0: vec4<f32>,    // 16 bytes — column 0 of normal matrix
    normal_c1: vec4<f32>,    // 16 bytes — column 1
    normal_c2: vec4<f32>,    // 16 bytes — column 2 — total: 112 bytes
}
@group(2) @binding(0) var<uniform> mesh: MeshUniforms;

// group 3: lights + shadow map array
struct PointLightGpu {
    position:  vec3<f32>,  // offset  0
    intensity: f32,         // offset 12
    color:     vec3<f32>,  // offset 16
    radius:    f32,         // offset 28 — total: 32 bytes
}

// 3 cascades, tight per-slice light-space matrices.
// layout (std140, all 16-byte aligned):
//   [0..16]   ambient_color (vec3) + ambient_intensity (f32)
//   [16..32]  dir_color (vec3) + dir_illuminance (f32)
//   [32..48]  dir_direction (vec3) + dir_enabled (u32)
//   [48..112] light_space_0 (mat4)
//   [112..176] light_space_1 (mat4)
//   [176..240] light_space_2 (mat4)
//   [240..256] cascade_splits (vec4): [split0, split1, split2, far_plane]
//   [256..272] point header (count + 3 pads)
//   [272..528] 8 × PointLightGpu (32 bytes each)
struct Lights {
    ambient_color:     vec3<f32>,
    ambient_intensity: f32,
    dir_color:         vec3<f32>,
    dir_illuminance:   f32,
    dir_direction:     vec3<f32>,
    dir_enabled:       u32,
    light_space_0:     mat4x4<f32>,
    light_space_1:     mat4x4<f32>,
    light_space_2:     mat4x4<f32>,
    cascade_splits:    vec4<f32>,   // x=split0, y=split1, z=split2, w=far
    point_count:       u32,
    _pad0:             u32,
    _pad1:             u32,
    _pad2:             u32,
    point_lights:      array<PointLightGpu, 8>,
}
@group(3) @binding(0) var<uniform>  lights:         Lights;
@group(3) @binding(1) var           shadow_map:     texture_depth_2d_array;
@group(3) @binding(2) var           shadow_sampler: sampler_comparison;

// ── vertex I/O ─────────────────────────────────────────────────────────────

struct VertIn {
    @location(0) position:    vec3<f32>,
    @location(1) normal:      vec3<f32>,
    @location(2) tangent:     vec4<f32>,
    @location(3) uv:          vec2<f32>,
    @location(4) uv_lightmap: vec2<f32>,
    @location(5) color:       vec4<f32>,
}

struct VertOut {
    @builtin(position) clip_pos:     vec4<f32>,
    @location(0)       world_pos:    vec3<f32>,
    @location(1)       world_normal: vec3<f32>,
    @location(2)       uv:           vec2<f32>,
    @location(3)       color:        vec4<f32>,
    @location(4)       view_depth:   f32,   // linear view-space depth for cascade selection
}

@vertex
fn vs_main(in: VertIn) -> VertOut {
    let world_pos4 = mesh.model * vec4<f32>(in.position, 1.0);
    let view_pos4  = globals.view_proj * world_pos4;
    let normal_mat = mat3x3<f32>(
        mesh.normal_c0.xyz,
        mesh.normal_c1.xyz,
        mesh.normal_c2.xyz,
    );
    var out: VertOut;
    out.clip_pos     = view_pos4;
    out.world_pos    = world_pos4.xyz;
    out.world_normal = normalize(normal_mat * in.normal);
    out.uv           = in.uv;
    out.color        = in.color;
    // view_depth is positive distance from the camera (clip_w ≈ view_z in RH perspective)
    out.view_depth   = view_pos4.w;
    return out;
}

// ── PBR helpers ────────────────────────────────────────────────────────────

const PI: f32 = 3.14159265358979;

// GGX/Trowbridge-Reitz normal distribution function
fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a2 = roughness * roughness * roughness * roughness;
    let ndh = max(dot(n, h), 0.0);
    let denom = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

// Smith-GGX geometry shadowing (Schlick approximation)
fn geometry_schlick(ndv: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = r * r / 8.0;
    return ndv / (ndv * (1.0 - k) + k);
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    return geometry_schlick(max(dot(n, v), 0.0), roughness)
         * geometry_schlick(max(dot(n, l), 0.0), roughness);
}

// Fresnel-Schlick approximation
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// Cook-Torrance specular + Lambertian diffuse for one light sample
fn pbr_light(
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, roughness: f32,
    irradiance: vec3<f32>, ndotl: f32,
) -> vec3<f32> {
    let h = normalize(v + l);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let d = distribution_ggx(n, h, roughness);
    let g = geometry_smith(n, v, l, roughness);
    let f = fresnel_schlick(max(dot(h, v), 0.0), f0);
    let specular = (d * g * f) / max(4.0 * max(dot(n, v), 0.0) * ndotl, 0.001);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    return (kd * albedo / PI + specular) * irradiance * ndotl;
}

// ── cascade shadow helpers ─────────────────────────────────────────────────

// select cascade index from view-space depth
fn cascade_index(view_depth: f32) -> i32 {
    if view_depth < lights.cascade_splits.x { return 0; }
    if view_depth < lights.cascade_splits.y { return 1; }
    return 2;
}

fn cascade_light_space(idx: i32) -> mat4x4<f32> {
    if idx == 0 { return lights.light_space_0; }
    if idx == 1 { return lights.light_space_1; }
    return lights.light_space_2;
}

// 5×5 PCF over the selected cascade array layer (mid/high quality)
fn shadow_factor_5x5(world_pos: vec3<f32>, n: vec3<f32>, view_depth: f32) -> f32 {
    let bias = max(0.005 * (1.0 - dot(n, -lights.dir_direction)), 0.001);
    let idx = cascade_index(view_depth);
    let lsp = cascade_light_space(idx) * vec4<f32>(world_pos, 1.0);
    var proj = lsp.xyz / lsp.w;
    proj.x =  proj.x * 0.5 + 0.5;
    proj.y = -proj.y * 0.5 + 0.5;
    if proj.z > 1.0 || proj.z < 0.0 || proj.x < 0.0 || proj.x > 1.0 || proj.y < 0.0 || proj.y > 1.0 {
        return 1.0;
    }
    let texel = 1.0 / 1024.0;
    var shadow = 0.0;
    for (var xi = -2; xi <= 2; xi++) {
        for (var yi = -2; yi <= 2; yi++) {
            let off = vec2<f32>(f32(xi) * texel, f32(yi) * texel);
            shadow += textureSampleCompare(shadow_map, shadow_sampler, proj.xy + off, idx, proj.z - bias);
        }
    }
    return shadow / 25.0;
}

// ── ACES filmic tonemap ────────────────────────────────────────────────────

// ACES filmic curve approximation (Narkowicz 2015)
fn aces_tonemap(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return saturate((x * (a * x + b)) / (x * (c * x + d) + e));
}

// ── fragment shader ────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let albedo   = material.base_color.rgb * in.color.rgb;
    let alpha    = material.base_color.a * in.color.a;
    let metallic = material.metallic;
    let roughness = clamp(material.roughness, 0.04, 1.0);

    // unlit path (sky dome, sun disc, debug geo)
    if (material.flags & 1u) != 0u {
        return vec4<f32>(albedo, alpha);
    }

    let n = normalize(in.world_normal);
    let v = normalize(globals.cam_pos - in.world_pos);

    var lo = vec3<f32>(0.0);

    // directional light with cascaded shadow
    if lights.dir_enabled != 0u {
        let l = normalize(-lights.dir_direction);
        let ndotl = max(dot(n, l), 0.0);
        if ndotl > 0.0 {
            let irradiance = lights.dir_color * (lights.dir_illuminance / 80000.0);
            let shadow = shadow_factor_5x5(in.world_pos, n, in.view_depth);
            lo += pbr_light(n, v, l, albedo, metallic, roughness, irradiance, ndotl) * shadow;
        }
    }

    // point lights (up to 8)
    for (var i = 0u; i < min(lights.point_count, 8u); i++) {
        let light   = lights.point_lights[i];
        let to_light = light.position - in.world_pos;
        let dist    = length(to_light);
        if dist >= light.radius { continue; }
        let l     = to_light / dist;
        let ndotl = max(dot(n, l), 0.0);
        if ndotl <= 0.0 { continue; }
        // Frostbite smooth-cutoff inverse-square attenuation
        let r     = dist / light.radius;
        let window = clamp(1.0 - r * r * r * r, 0.0, 1.0);
        let att   = window * window / (dist * dist + 1.0);
        let irradiance = light.color * light.intensity * att;
        lo += pbr_light(n, v, l, albedo, metallic, roughness, irradiance, ndotl);
    }

    // ambient (Lambert-weighted to avoid flat look)
    let ambient = lights.ambient_color * lights.ambient_intensity * albedo * (1.0 - metallic * 0.9);

    // ACES filmic tonemap before output
    let hdr = ambient + lo;
    let ldr = aces_tonemap(hdr);

    return vec4<f32>(ldr, alpha);
}
