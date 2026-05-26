// per-frame globals
struct Globals {
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> globals: Globals;

// per-draw: model matrix + base color packed into one 80-byte buffer
struct DrawUniforms {
    model:      mat4x4<f32>,  // offset 0,  64 bytes
    base_color: vec4<f32>,    // offset 64, 16 bytes
}
@group(1) @binding(0) var<uniform> draw: DrawUniforms;

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
    out.clip_pos = globals.view_proj * draw.model * vec4<f32>(in.position, 1.0);
    out.color    = draw.base_color * in.color;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return in.color;
}
