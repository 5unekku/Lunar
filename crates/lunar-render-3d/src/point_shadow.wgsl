// point light shadow pass — renders scene from each cube face.
// writes linear depth (dist / light_radius) so the main shader can compare
// `dist / radius - bias` against stored values without a non-linear transform.

struct PointShadowGlobals {
    light_vp:     mat4x4<f32>,  // 64 bytes
    light_pos:    vec3<f32>,    // 12 bytes (offset 64)
    light_radius: f32,           //  4 bytes (offset 76) — total: 80 bytes
}
@group(0) @binding(0) var<uniform> shadow_globals: PointShadowGlobals;

struct MeshInstance {
    model:     mat4x4<f32>,
    normal_c0: vec4<f32>,
    normal_c1: vec4<f32>,
    normal_c2: vec4<f32>,
    _pad:      array<vec4<f32>, 9>,
}
@group(1) @binding(0) var<storage, read> instances: array<MeshInstance>;

struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       world_pos: vec3<f32>,
}

@vertex
fn vs_point_shadow(
    @location(0) position: vec3<f32>,
    @builtin(instance_index) instance_id: u32,
) -> VOut {
    let world_pos4 = instances[instance_id].model * vec4<f32>(position, 1.0);
    var out: VOut;
    out.clip_pos  = shadow_globals.light_vp * world_pos4;
    out.world_pos = world_pos4.xyz;
    return out;
}

@fragment
fn fs_point_shadow(in: VOut) -> @builtin(frag_depth) f32 {
    let dist = length(in.world_pos - shadow_globals.light_pos);
    return clamp(dist / shadow_globals.light_radius, 0.0, 1.0);
}
