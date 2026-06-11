// masked surface z-prepass — depth-only variant of surface.wgsl for stages
// with alpha_test enabled (sprites, grates, fences). the regular prepass draws
// positions only, which would stamp the full quad into the depth buffer and
// punch holes into everything behind the transparent texels; this one samples
// stage 0 and discards below the threshold so prepass depth matches the
// surface pass exactly.

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
@group(0) @binding(0) var<uniform> globals: Globals;

struct MeshInstance {
    model:     mat4x4<f32>,
    normal_c0: vec4<f32>,
    normal_c1: vec4<f32>,
    normal_c2: vec4<f32>,
    _pad:      array<vec4<f32>, 9>,
}
@group(1) @binding(0) var<storage, read> instances: array<MeshInstance>;

struct StageData {
    uv_offset:  vec2<f32>,
    uv_scale:   f32,
    blend:      u32,
    alpha:      f32,
    use_lm_uv:  u32,
    enabled:    u32,
    flags:      u32,
}
struct SurfaceParams {
    stages: array<StageData, 4>,
}
@group(2) @binding(0) var<uniform> surface_params: SurfaceParams;
@group(2) @binding(1) var          tex0:           texture_2d<f32>;
@group(2) @binding(5) var          surf_sampler:   sampler;
@group(2) @binding(6) var          surf_nearest:   sampler;

struct VertIn {
    @location(0) position: vec3<f32>,
    @location(3) uv:       vec2<f32>,
}

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
}

// matches surface.wgsl so prepass depth lines up with the main pass
fn snap_vertex(clip: vec4<f32>) -> vec4<f32> {
    if globals.vertex_snap <= 0.0 {
        return clip;
    }
    let g = globals.vertex_snap;
    let snapped = round(clip.xy / clip.w * g) / g * clip.w;
    return vec4<f32>(snapped, clip.z, clip.w);
}

@vertex
fn vs_surface_prepass(in: VertIn, @builtin(instance_index) instance_id: u32) -> VertOut {
    let inst = instances[instance_id];
    let world_pos = inst.model * vec4<f32>(in.position, 1.0);
    var out: VertOut;
    out.clip_pos = snap_vertex(globals.view_proj * world_pos);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_surface_prepass(in: VertOut) {
    let stage = surface_params.stages[0];
    let uv = (in.uv + stage.uv_offset) * stage.uv_scale;
    var sampled: vec4<f32>;
    if (stage.flags & 2u) != 0u {
        sampled = textureSample(tex0, surf_nearest, uv);
    } else {
        sampled = textureSample(tex0, surf_sampler, uv);
    }
    if (stage.flags & 1u) != 0u && sampled.a * stage.alpha < 0.5 {
        discard;
    }
}
