// lighting_model values — matches LightingModel enum on the CPU
const LIGHTING_PBR:     u32 = 0u;  // full PBR (GGX specular, SH ambient, shadows)
const LIGHTING_LAMBERT: u32 = 1u;  // diffuse-only, no microfacet specular
const LIGHTING_BAKED:   u32 = 2u;  // lightmap + ambient only, no runtime lights

// group 0: view-global — set once per pass
struct Globals {
    view_proj:      mat4x4<f32>,  // 64 bytes
    cam_pos:        vec3<f32>,    // 12 bytes (offset 64)
    elapsed_secs:   f32,          //  4 bytes (offset 76)
    delta_secs:     f32,          //  4 bytes (offset 80)
    lighting_model: u32,          //  4 bytes (offset 84)
    render_flags:   u32,          //  4 bytes (offset 88) — bit 0: soft_shadows, bit 1: contact_shadows, bit 2: affine_textures
    vertex_snap:    f32,          //  4 bytes (offset 92) — snap grid (0 = off)
    classic_light:  f32,          //  4 bytes (offset 96) — surface-path depth-cued light constant (0 = off)
    _pad0: f32, _pad1: f32, _pad2: f32, // total: 112 bytes
}
@group(0) @binding(0) var<uniform> globals: Globals;

// group 1: material storage — indexed by instance_id, set once per pass
struct MaterialUniforms {
    base_color:    vec4<f32>,  // 16 bytes, offset  0
    metallic:      f32,         //  4 bytes, offset 16
    roughness:     f32,         //  4 bytes, offset 20
    flags:         u32,         //  4 bytes, offset 24  (bit 0 = unlit)
    has_lightmap:  u32,         //  4 bytes, offset 28
    lm_uv_offset:  vec2<f32>,  //  8 bytes, offset 32  (atlas offset; identity = (0,0))
    lm_uv_scale:   vec2<f32>,  //  8 bytes, offset 40  (atlas scale;  identity = (1,1))
    // total: 48 bytes
}
@group(1) @binding(0) var<storage, read> materials: array<MaterialUniforms>;

// group 2: per-instance transforms — storage array, indexed by @builtin(instance_index).
// padded to 256 bytes to match the UNIFORM_STRIDE staging layout on the CPU.
struct MeshInstance {
    model:     mat4x4<f32>,              // 64 bytes — offset   0
    normal_c0: vec4<f32>,                // 16 bytes — offset  64
    normal_c1: vec4<f32>,                // 16 bytes — offset  80
    normal_c2: vec4<f32>,                // 16 bytes — offset  96
    // 9 L2 SH coefficients: .xyz = RGB irradiance, .w = 1.0 when probe data present / 0.0 = fallback
    sh_coeffs: array<vec4<f32>, 9>,      // 144 bytes — offset 112 (total: 256)
}
@group(2) @binding(0) var<storage, read> instances: array<MeshInstance>;

// group 3: lights + shadow map array
struct PointLightGpu {
    position:    vec3<f32>,  // offset  0
    intensity:   f32,         // offset 12
    color:       vec3<f32>,  // offset 16
    radius:      f32,         // offset 28
    shadow_index: u32,        // offset 32  (0xffffffff = unshadowed)
    _pad0:        u32,        // offset 36
    _pad1:        u32,        // offset 40
    _pad2:        u32,        // offset 44  — total: 48 bytes
}

// lights uniform buffer layout (total 816 bytes):
//   [0..16]    ambient_color (vec3) + ambient_intensity (f32)
//   [16..32]   dir_color (vec3) + dir_illuminance (f32)
//   [32..48]   dir_direction (vec3) + dir_enabled (u32)
//   [48..112]  light_space_0 (mat4)
//   [112..176] light_space_1 (mat4)
//   [176..240] light_space_2 (mat4)
//   [240..256] cascade_splits (vec4)
//   [256..272] point header (count + 3 pads)
//   [272..656] 8 × PointLightGpu (48 bytes each)
//   [656..816] SH ambient (header + 9 coefficients)
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
    // SH ambient: when sh_enabled=1 these 9 pre-scaled L2 coefficients replace flat ambient.
    // each coefficient is vec4(R, G, B, 0). order: L0, L1x, L1y, L1z, L2xy, L2yz, L2_0, L2xz, L2_x2y2
    sh_enabled:        u32,
    _sh_pad0:          u32,
    _sh_pad1:          u32,
    _sh_pad2:          u32,
    sh0:  vec4<f32>,   sh1:  vec4<f32>,   sh2:  vec4<f32>,
    sh3:  vec4<f32>,   sh4:  vec4<f32>,   sh5:  vec4<f32>,
    sh6:  vec4<f32>,   sh7:  vec4<f32>,   sh8:  vec4<f32>,
}
// point lights are in group 5 (separate storage buffer for up to 256 lights)
@group(3) @binding(0) var<uniform>  lights:            Lights;
@group(3) @binding(1) var           shadow_map:        texture_depth_2d_array;
@group(3) @binding(2) var           shadow_sampler:    sampler_comparison;
// 4 shadowed point lights × 6 faces = 24 layers (u32::MAX shadow_index = unshadowed)
@group(3) @binding(3) var           point_shadow_maps: texture_depth_2d_array;

// group 5: clustered point lighting
// 16×9×24 = 3456 clusters; each cluster holds up to 32 light indices.
const CLUSTER_X_F: u32 = 16u;
const CLUSTER_Y_F: u32 = 9u;
const CLUSTER_Z_F: u32 = 24u;
const MAX_LIGHTS_PER_CLUSTER_F: u32 = 32u;

struct ClusterParamsF {
    view_proj:   mat4x4<f32>,
    screen_w:    u32,
    screen_h:    u32,
    light_count: u32,
    _pad0:       u32,
    near:        f32,
    far:         f32,
    focal_x:     f32,
    _pad1:       f32,
}
@group(5) @binding(0) var<uniform>       cluster_params_f:       ClusterParamsF;
@group(5) @binding(1) var<storage, read> light_list_f:           array<PointLightGpu>;
@group(5) @binding(2) var<storage, read> cluster_counts_f:       array<u32>;
@group(5) @binding(3) var<storage, read> cluster_light_indices_f: array<u32>;

// group 4: lightmap — bound per draw group; fallback textures are 1×1
// binding 0: irradiance (rgba8 srgb, white fallback)
// binding 1: dominant direction packed as rgb * 0.5 + 0.5 (neutral fallback = (0.5, 0.5, 1.0) = world up)
// binding 2: shared sampler
@group(4) @binding(0) var lightmap_tex:     texture_2d<f32>;
@group(4) @binding(1) var lightmap_dir_tex: texture_2d<f32>;
@group(4) @binding(2) var lightmap_sampler: sampler;

// ── vertex I/O ─────────────────────────────────────────────────────────────

struct VertIn {
    @location(0) position:    vec3<f32>,
    @location(1) normal:      vec4<f32>,  // snorm8×4 — hardware normalises to [-1,1]; use .xyz
    @location(2) tangent:     vec4<f32>,  // snorm8×4 — .xyz = tangent, .w = handedness
    @location(3) uv:          vec2<f32>,  // unorm16×2
    @location(4) uv_lightmap: vec2<f32>,  // unorm16×2
    @location(5) color:       vec4<f32>,  // unorm8×4
}

struct VertOut {
    @builtin(position) clip_pos:     vec4<f32>,
    @location(0)       world_pos:    vec3<f32>,
    @location(1)       world_normal: vec3<f32>,
    @location(2)       uv:           vec2<f32>,
    @location(3)       color:        vec4<f32>,
    @location(4)       view_depth:   f32,   // linear view-space depth for cascade selection
    @location(5)       uv_lightmap:  vec2<f32>,
    @location(6) @interpolate(flat) instance_id:  u32,
}

// vertex snapping: quantize clip-space xy onto a low-resolution grid.
// returns the input unchanged when off (vertex_snap <= 0).
// w is preserved so view-space depth and perspective division stay correct.
fn snap_vertex(clip: vec4<f32>) -> vec4<f32> {
    if globals.vertex_snap <= 0.0 {
        return clip;
    }
    let g = globals.vertex_snap;
    let snapped = round(clip.xy / clip.w * g) / g * clip.w;
    return vec4<f32>(snapped, clip.z, clip.w);
}

@vertex
fn vs_main(in: VertIn, @builtin(instance_index) instance_id: u32) -> VertOut {
    let inst       = instances[instance_id];
    let world_pos4 = inst.model * vec4<f32>(in.position, 1.0);
    let view_pos4  = globals.view_proj * world_pos4;
    let normal_mat = mat3x3<f32>(
        inst.normal_c0.xyz,
        inst.normal_c1.xyz,
        inst.normal_c2.xyz,
    );
    var out: VertOut;
    out.clip_pos     = snap_vertex(view_pos4);
    out.world_pos    = world_pos4.xyz;
    out.world_normal = normalize(normal_mat * in.normal.xyz);
    out.uv           = in.uv;
    out.color        = in.color;
    // view_depth is positive distance from the camera (clip_w ≈ view_z in RH perspective)
    out.view_depth   = view_pos4.w;
    out.uv_lightmap  = in.uv_lightmap;
    out.instance_id  = instance_id;
    return out;
}

// depth-only pass — reads position only, no varyings. used by z-prepass pipelines.
@vertex
fn vs_depth(
    @location(0) position: vec3<f32>,
    @builtin(instance_index) instance_id: u32,
) -> @builtin(position) vec4<f32> {
    let model = instances[instance_id].model;
    // snap identically to vs_main so a depth-prepass matches the colour pass exactly.
    return snap_vertex(globals.view_proj * model * vec4<f32>(position, 1.0));
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

// per-light contribution dispatched by lighting_model. LIGHTING_PBR is the full
// Cook-Torrance path; LIGHTING_LAMBERT drops microfacet specular. LIGHTING_BAKED
// never reaches here (its light loops are skipped entirely).
fn lit(
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, roughness: f32,
    irradiance: vec3<f32>, ndotl: f32,
) -> vec3<f32> {
    if globals.lighting_model == LIGHTING_LAMBERT {
        return albedo / PI * irradiance * ndotl;
    }
    return pbr_light(n, v, l, albedo, metallic, roughness, irradiance, ndotl);
}

// ── Poisson disk sample sets ───────────────────────────────────────────────

const POISSON_8: array<vec2<f32>, 8> = array<vec2<f32>, 8>(
    vec2<f32>(-0.9450, -0.3255), vec2<f32>(-0.4503,  0.8905),
    vec2<f32>( 0.7490, -0.6596), vec2<f32>( 0.4285,  0.5616),
    vec2<f32>(-0.7090, -0.7010), vec2<f32>( 0.9490,  0.1565),
    vec2<f32>(-0.1470,  0.3650), vec2<f32>( 0.1540, -0.8060),
);

const POISSON_16: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
    vec2<f32>(-0.9440, -0.3275), vec2<f32>(-0.4515,  0.8890),
    vec2<f32>( 0.7480, -0.6600), vec2<f32>( 0.4295,  0.5610),
    vec2<f32>(-0.7095, -0.7005), vec2<f32>( 0.9495,  0.1570),
    vec2<f32>(-0.1475,  0.3645), vec2<f32>( 0.1545, -0.8055),
    vec2<f32>(-0.6275, -0.1695), vec2<f32>( 0.3075,  0.9360),
    vec2<f32>(-0.1830, -0.4975), vec2<f32>( 0.7635,  0.4245),
    vec2<f32>( 0.4250, -0.3295), vec2<f32>(-0.9325,  0.3615),
    vec2<f32>( 0.0615,  0.1800), vec2<f32>(-0.3070, -0.9270),
);

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

// PCSS: percentage closer soft shadows for directional light.
// step 1: blocker search (8 taps, textureLoad) → average blocker depth
// step 2: penumbra width = (receiver - avg_blocker) / avg_blocker * scale
// step 3: 16-tap PCF with variable kernel radius
fn shadow_factor_pcss(world_pos: vec3<f32>, n: vec3<f32>, view_depth: f32) -> f32 {
    let bias = max(0.005 * (1.0 - dot(n, -lights.dir_direction)), 0.001);
    let idx  = cascade_index(view_depth);
    let lsp  = cascade_light_space(idx) * vec4<f32>(world_pos, 1.0);
    var proj = lsp.xyz / lsp.w;
    proj.x =  proj.x * 0.5 + 0.5;
    proj.y = -proj.y * 0.5 + 0.5;
    if proj.z > 1.0 || proj.z < 0.0 || proj.x < 0.0 || proj.x > 1.0 || proj.y < 0.0 || proj.y > 1.0 {
        return 1.0;
    }
    let shadow_res = 1024.0;
    let search_r   = 0.04;
    let idx32      = i32(idx);
    var blocker_sum   = 0.0;
    var blocker_count = 0u;
    for (var i = 0u; i < 8u; i++) {
        let tc_uv = proj.xy + POISSON_8[i] * search_r;
        let tc = vec2<i32>(
            clamp(i32(tc_uv.x * shadow_res), 0, i32(shadow_res) - 1),
            clamp(i32(tc_uv.y * shadow_res), 0, i32(shadow_res) - 1),
        );
        let depth = textureLoad(shadow_map, tc, idx32, 0);
        if depth < proj.z - bias {
            blocker_sum   += depth;
            blocker_count++;
        }
    }
    if blocker_count == 0u { return 1.0; }
    let avg_blocker  = blocker_sum / f32(blocker_count);
    let penumbra_uv  = max((proj.z - avg_blocker) / avg_blocker * 0.15, 1.0 / shadow_res);
    var shadow = 0.0;
    for (var i = 0u; i < 16u; i++) {
        let off = POISSON_16[i] * penumbra_uv;
        shadow += textureSampleCompare(shadow_map, shadow_sampler, proj.xy + off, idx, proj.z - bias);
    }
    return shadow / 16.0;
}

// dispatch to PCSS or fixed 5×5 PCF based on render_flags bit 0
fn shadow_factor(world_pos: vec3<f32>, n: vec3<f32>, view_depth: f32) -> f32 {
    if (globals.render_flags & 1u) != 0u {
        return shadow_factor_pcss(world_pos, n, view_depth);
    }
    return shadow_factor_5x5(world_pos, n, view_depth);
}

// ── point shadow helpers ───────────────────────────────────────────────────

// given a direction vector from light to surface, returns the 2D texture UV and
// layer index into point_shadow_maps (shadow_index * 6 + face).
// face numbering follows the OpenGL cube map convention:
// 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z
fn point_shadow_layer_uv(dir: vec3<f32>, shadow_index: u32) -> vec3<f32> {
    let ax = abs(dir.x); let ay = abs(dir.y); let az = abs(dir.z);
    var face: u32; var u: f32; var v: f32;
    if ax >= ay && ax >= az {
        if dir.x > 0.0 {
            face = 0u;
            u = (-dir.z / ax + 1.0) * 0.5;
            v = (1.0 + dir.y / ax) * 0.5;
        } else {
            face = 1u;
            u = (dir.z / ax + 1.0) * 0.5;
            v = (1.0 + dir.y / ax) * 0.5;
        }
    } else if ay >= ax && ay >= az {
        if dir.y > 0.0 {
            face = 2u;
            u = (dir.x / ay + 1.0) * 0.5;
            v = (1.0 - dir.z / ay) * 0.5;
        } else {
            face = 3u;
            u = (dir.x / ay + 1.0) * 0.5;
            v = (1.0 + dir.z / ay) * 0.5;
        }
    } else {
        if dir.z > 0.0 {
            face = 4u;
            u = (dir.x / az + 1.0) * 0.5;
            v = (1.0 + dir.y / az) * 0.5;
        } else {
            face = 5u;
            u = (-dir.x / az + 1.0) * 0.5;
            v = (1.0 + dir.y / az) * 0.5;
        }
    }
    let layer = shadow_index * 6u + face;
    return vec3<f32>(u, v, f32(layer));
}

// ── fragment shader ────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let material = materials[in.instance_id];
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

    // LIGHTING_BAKED skips all runtime dynamic lights (ambient + lightmap only).
    // Pbr/Lambert run the loops below; lit() picks the per-light shading model.
    let dynamic_lighting = globals.lighting_model != LIGHTING_BAKED;

    // directional light with cascaded shadow — separate from point lights for lightmap path
    var dir_lo = vec3<f32>(0.0);
    if dynamic_lighting && lights.dir_enabled != 0u {
        let l = normalize(-lights.dir_direction);
        let ndotl = max(dot(n, l), 0.0);
        if ndotl > 0.0 {
            let irradiance = lights.dir_color * (lights.dir_illuminance / 80000.0);
            let shadow = shadow_factor(in.world_pos, n, in.view_depth);
            dir_lo += lit(n, v, l, albedo, metallic, roughness, irradiance, ndotl) * shadow;
        }
    }

    // point lights — clustered lookup from group 5 storage buffers
    var point_lo = vec3<f32>(0.0);
    if dynamic_lighting && cluster_params_f.light_count > 0u {
        // determine cluster cell for this fragment
        let screen_x = in.clip_pos.x;
        let screen_y = in.clip_pos.y;
        let tile_w = f32(cluster_params_f.screen_w) / f32(CLUSTER_X_F);
        let tile_h = f32(cluster_params_f.screen_h) / f32(CLUSTER_Y_F);
        let cx = u32(screen_x / tile_w);
        let cy = u32(screen_y / tile_h);
        let depth = in.view_depth;
        let near = cluster_params_f.near;
        let far = cluster_params_f.far;
        let cz = u32(log(depth / near) / log(far / near) * f32(CLUSTER_Z_F));
        let cluster_idx = min(cz, CLUSTER_Z_F - 1u) * CLUSTER_X_F * CLUSTER_Y_F
                        + min(cy, CLUSTER_Y_F - 1u) * CLUSTER_X_F
                        + min(cx, CLUSTER_X_F - 1u);
        let count = min(cluster_counts_f[cluster_idx], MAX_LIGHTS_PER_CLUSTER_F);
        let base = cluster_idx * MAX_LIGHTS_PER_CLUSTER_F;
        for (var j = 0u; j < count; j++) {
            let light_idx = cluster_light_indices_f[base + j];
            let light = light_list_f[light_idx];
            let to_light = light.position - in.world_pos;
            let dist    = length(to_light);
            if dist >= light.radius { continue; }
            let l     = to_light / dist;
            let ndotl = max(dot(n, l), 0.0);
            if ndotl <= 0.0 { continue; }
            let r     = dist / light.radius;
            let window = clamp(1.0 - r * r * r * r, 0.0, 1.0);
            let att   = window * window / (dist * dist + 1.0);
            let irradiance = light.color * light.intensity * att;
            var shadow_fac = 1.0;
            if light.shadow_index != 0xffffffffu {
                let shadow_dir = in.world_pos - light.position;
                let ref_depth  = dist / light.radius - 0.01;
                if (globals.render_flags & 1u) != 0u {
                    // 5-tap jittered PCF for soft point shadows
                    let jitter_scale = 0.04 * dist;
                    let offsets = array<vec3<f32>, 5>(
                        vec3<f32>( 0.00,  0.00,  0.00),
                        vec3<f32>( jitter_scale,  0.00,  0.00),
                        vec3<f32>(-jitter_scale,  0.00,  0.00),
                        vec3<f32>( 0.00,  jitter_scale,  0.00),
                        vec3<f32>( 0.00, -jitter_scale,  0.00),
                    );
                    var point_shadow_sum = 0.0;
                    for (var si = 0u; si < 5u; si++) {
                        let jdir = shadow_dir + offsets[si];
                        let jluv = point_shadow_layer_uv(jdir, light.shadow_index);
                        point_shadow_sum += textureSampleCompare(point_shadow_maps, shadow_sampler, jluv.xy, i32(jluv.z), ref_depth);
                    }
                    shadow_fac = point_shadow_sum / 5.0;
                } else {
                    let luv = point_shadow_layer_uv(shadow_dir, light.shadow_index);
                    shadow_fac = textureSampleCompare(point_shadow_maps, shadow_sampler, luv.xy, i32(luv.z), ref_depth);
                }
            }
            point_lo += shadow_fac * lit(n, v, l, albedo, metallic, roughness, irradiance, ndotl);
        }
    }

    // ambient — per-entity SH from instance buffer when sh_coeffs[0].w > 0, else global lights.sh
    var ambient: vec3<f32>;
    let inst_sh = instances[in.instance_id].sh_coeffs;
    let nx = n.x; let ny = n.y; let nz = n.z;
    if inst_sh[0].w > 0.0 {
        // per-entity SH (from AmbientProbeGrid or global IrradianceSH written per-entity)
        var sh_irr = inst_sh[0].xyz;
        sh_irr += inst_sh[1].xyz * nx;
        sh_irr += inst_sh[2].xyz * ny;
        sh_irr += inst_sh[3].xyz * nz;
        sh_irr += inst_sh[4].xyz * (nx * ny);
        sh_irr += inst_sh[5].xyz * (ny * nz);
        sh_irr += inst_sh[6].xyz * (3.0 * nz * nz - 1.0);
        sh_irr += inst_sh[7].xyz * (nx * nz);
        sh_irr += inst_sh[8].xyz * (nx * nx - ny * ny);
        ambient = max(sh_irr, vec3<f32>(0.0)) * albedo * (1.0 - metallic * 0.9);
    } else if lights.sh_enabled != 0u {
        // fallback: global IrradianceSH resource
        var sh_irr = lights.sh0.xyz;
        sh_irr += lights.sh1.xyz * nx;
        sh_irr += lights.sh2.xyz * ny;
        sh_irr += lights.sh3.xyz * nz;
        sh_irr += lights.sh4.xyz * (nx * ny);
        sh_irr += lights.sh5.xyz * (ny * nz);
        sh_irr += lights.sh6.xyz * (3.0 * nz * nz - 1.0);
        sh_irr += lights.sh7.xyz * (nx * nz);
        sh_irr += lights.sh8.xyz * (nx * nx - ny * ny);
        ambient = max(sh_irr, vec3<f32>(0.0)) * albedo * (1.0 - metallic * 0.9);
    } else {
        ambient = lights.ambient_color * lights.ambient_intensity * albedo * (1.0 - metallic * 0.9);
    }

    // output raw HDR — composite pass applies ACES tonemap + post effects.
    // lightmap replaces directional + ambient for static baked geometry.
    var hdr: vec3<f32>;
    if (material.has_lightmap != 0u) {
        let atlas_uv = material.lm_uv_offset + in.uv_lightmap * material.lm_uv_scale;
        let lm = textureSample(lightmap_tex, lightmap_sampler, atlas_uv).rgb;
        var lm_contrib = lm;
        // bit 1 = has directional lightmap; modulate irradiance by normal/dominant-dir alignment
        if ((material.flags & 2u) != 0u) {
            // direction texture uses raw uv_lightmap (not atlased)
            let lm_dir_raw = textureSample(lightmap_dir_tex, lightmap_sampler, in.uv_lightmap).rgb;
            let lm_dir = normalize(lm_dir_raw * 2.0 - vec3<f32>(1.0));
            // weight in [0, 2]; average over hemisphere = 1.0, preserves energy
            let dir_weight = max(dot(n, lm_dir), 0.0) * 2.0;
            lm_contrib = lm * dir_weight;
        }
        hdr = lm_contrib * albedo + point_lo;
    } else {
        hdr = ambient + dir_lo + point_lo;
    }
    return vec4<f32>(hdr, alpha);
}
