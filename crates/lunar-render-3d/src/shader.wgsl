// group 0: view-global — set once per pass
struct Globals {
    view_proj:    mat4x4<f32>,  // 64 bytes
    elapsed_secs: f32,          //  4 bytes
    delta_secs:   f32,          //  4 bytes
    _pad:         vec2<f32>,    //  8 bytes — total: 80 bytes
}
@group(0) @binding(0) var<uniform> globals: Globals;

// group 1: material — set once per material batch
struct MaterialUniforms {
    base_color: vec4<f32>,
}
@group(1) @binding(0) var<uniform> material: MaterialUniforms;

// group 2: per-mesh — dynamic offset selects entity slot
struct MeshUniforms {
    model: mat4x4<f32>,
}
@group(2) @binding(0) var<uniform> mesh: MeshUniforms;

struct VertIn {
    @location(0) position:    vec3<f32>,
    @location(1) normal:      vec3<f32>,
    @location(2) tangent:     vec4<f32>,
    @location(3) uv:          vec2<f32>,
    @location(4) uv_lightmap: vec2<f32>,
    @location(5) color:       vec4<f32>,  // unorm8x4 → [0,1] automatically
}

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       color:    vec4<f32>,
}

@vertex
fn vs_main(in: VertIn) -> VertOut {
    var out: VertOut;
    out.clip_pos = globals.view_proj * mesh.model * vec4<f32>(in.position, 1.0);
    out.color    = material.base_color * in.color;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return in.color;
}
