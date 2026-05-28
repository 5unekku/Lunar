// depth-only shadow pass — renders scene geometry from the directional light's POV.
// only the vertex position is needed; no fragment output.

// group 0: light view-projection
struct ShadowGlobals {
    light_vp: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> shadow_globals: ShadowGlobals;

// group 1: per-mesh transform (same layout as main pass; only model is read here)
struct MeshUniforms {
    model:     mat4x4<f32>,
    normal_c0: vec4<f32>,
    normal_c1: vec4<f32>,
    normal_c2: vec4<f32>,
}
@group(1) @binding(0) var<uniform> mesh: MeshUniforms;

@vertex
fn vs_shadow(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return shadow_globals.light_vp * mesh.model * vec4<f32>(position, 1.0);
}
