// depth-only shadow pass — renders scene geometry from the directional light's POV.
// only the vertex position is needed; no fragment output.

// group 0: light view-projection
struct ShadowGlobals {
    light_vp: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> shadow_globals: ShadowGlobals;

// group 1: per-instance transforms — storage array, indexed by @builtin(instance_index).
// layout matches group 2 in shader.wgsl (256 bytes per entry, UNIFORM_STRIDE padding).
struct MeshInstance {
    model:     mat4x4<f32>,
    normal_c0: vec4<f32>,
    normal_c1: vec4<f32>,
    normal_c2: vec4<f32>,
    _pad:      array<vec4<f32>, 9>,
}
@group(1) @binding(0) var<storage, read> instances: array<MeshInstance>;

@vertex
fn vs_shadow(@location(0) position: vec3<f32>, @builtin(instance_index) instance_id: u32) -> @builtin(position) vec4<f32> {
    return shadow_globals.light_vp * instances[instance_id].model * vec4<f32>(position, 1.0);
}
