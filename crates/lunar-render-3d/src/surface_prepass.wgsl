// masked surface z-prepass — depth-only variant of surface.wgsl for surfaces
// with alpha_test stages (sprites, grates, fences). the regular prepass draws
// positions only, which would stamp the full quad into the depth buffer and
// punch holes into everything behind the transparent texels; this one mirrors
// the main pass discard exactly (every enabled alpha_test stage, lightmap uv,
// affine uv) so prepass depth matches the surface pass bit for bit. stages
// without alpha_test can't discard and are skipped entirely.

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
    flags:      u32,  // bit 0 = alpha test, bit 1 = nearest sampling
}
struct SurfaceParams {
    stages: array<StageData, 4>,
}
@group(2) @binding(0) var<uniform> surface_params: SurfaceParams;
@group(2) @binding(1) var          tex0:           texture_2d<f32>;
@group(2) @binding(2) var          tex1:           texture_2d<f32>;
@group(2) @binding(3) var          tex2:           texture_2d<f32>;
@group(2) @binding(4) var          tex3:           texture_2d<f32>;
@group(2) @binding(5) var          surf_sampler:   sampler;
@group(2) @binding(6) var          surf_nearest:   sampler;

struct VertIn {
    @location(0) position:    vec3<f32>,
    @location(3) uv:          vec2<f32>,
    @location(4) uv_lightmap: vec2<f32>,
}

struct VertOut {
    @builtin(position) clip_pos:    vec4<f32>,
    @location(0)       uv:          vec2<f32>,
    @location(1)       uv_lightmap: vec2<f32>,
    // screen-linear interpolant, selected when affine_textures is on — must
    // match the main pass so the discard pattern lines up under affine warping
    @location(2) @interpolate(linear) uv_affine: vec2<f32>,
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
    out.clip_pos    = snap_vertex(globals.view_proj * world_pos);
    out.uv          = in.uv;
    out.uv_lightmap = in.uv_lightmap;
    out.uv_affine   = in.uv;
    return out;
}

// matches surface.wgsl apply_uv
fn apply_uv(base_uv: vec2<f32>, lm_uv: vec2<f32>, stage: StageData) -> vec2<f32> {
    let raw = select(base_uv, lm_uv, stage.use_lm_uv != 0u);
    return (raw + stage.uv_offset) * stage.uv_scale;
}

// matches surface.wgsl sample_stage
fn sample_stage(stage_idx: u32, uv: vec2<f32>) -> vec4<f32> {
    if (surface_params.stages[stage_idx].flags & 2u) != 0u {
        if stage_idx == 0u { return textureSample(tex0, surf_nearest, uv); }
        if stage_idx == 1u { return textureSample(tex1, surf_nearest, uv); }
        if stage_idx == 2u { return textureSample(tex2, surf_nearest, uv); }
        return textureSample(tex3, surf_nearest, uv);
    }
    if stage_idx == 0u { return textureSample(tex0, surf_sampler, uv); }
    if stage_idx == 1u { return textureSample(tex1, surf_sampler, uv); }
    if stage_idx == 2u { return textureSample(tex2, surf_sampler, uv); }
    return textureSample(tex3, surf_sampler, uv);
}

@fragment
fn fs_surface_prepass(in: VertOut) {
    let base_uv = select(in.uv, in.uv_affine, (globals.render_flags & 4u) != 0u);
    for (var s = 0u; s < 4u; s++) {
        let stage = surface_params.stages[s];
        // only enabled alpha_test stages can discard; nothing else affects depth
        if stage.enabled == 0u || (stage.flags & 1u) == 0u {
            continue;
        }
        let uv = apply_uv(base_uv, in.uv_lightmap, stage);
        if sample_stage(s, uv).a * stage.alpha < 0.5 {
            discard;
        }
    }
}
