// surface shader — q3-style multi-stage fixed-function blender.
// renders up to 4 texture stages, each with a blend mode and UV transform.
// only used with ShadingModel::Unlit entities.

// group 0: globals (view + time)
struct Globals {
    view_proj:      mat4x4<f32>,
    cam_pos:        vec3<f32>,
    elapsed_secs:   f32,
    delta_secs:     f32,
    lighting_model: u32,
    render_flags:   u32,  // bit 2 = affine_textures (disable perspective-correct UV)
    vertex_snap:    f32,  // snap grid resolution (0 = off)
}
@group(0) @binding(0) var<uniform> globals: Globals;

// group 1: per-instance transforms
struct MeshInstance {
    model:     mat4x4<f32>,
    normal_c0: vec4<f32>,
    normal_c1: vec4<f32>,
    normal_c2: vec4<f32>,
    _pad:      array<vec4<f32>, 9>,
}
@group(1) @binding(0) var<storage, read> instances: array<MeshInstance>;

// group 2: surface stage params + up to 4 textures
struct StageData {
    uv_offset:  vec2<f32>,  // current scroll + rotation offset, pre-evaluated on CPU
    uv_scale:   f32,         // uniform scale
    blend:      u32,         // 0=opaque, 1=add, 2=multiply, 3=alphablend
    alpha:      f32,         // constant alpha (AlphaGen::Const) or 1.0 for Identity
    use_lm_uv:  u32,         // 1 = sample using uv_lightmap channel
    enabled:    u32,         // 1 if this stage should be rendered
    _pad:       u32,
}

struct SurfaceParams {
    stages: array<StageData, 4>,  // 4 × 32 bytes = 128 bytes
}
@group(2) @binding(0) var<uniform>  surface_params: SurfaceParams;
@group(2) @binding(1) var           tex0:           texture_2d<f32>;
@group(2) @binding(2) var           tex1:           texture_2d<f32>;
@group(2) @binding(3) var           tex2:           texture_2d<f32>;
@group(2) @binding(4) var           tex3:           texture_2d<f32>;
@group(2) @binding(5) var           surf_sampler:   sampler;

// vertex I/O

struct VertIn {
    @location(0) position:    vec3<f32>,
    @location(1) normal:      vec3<f32>,
    @location(2) tangent:     vec4<f32>,
    @location(3) uv:          vec2<f32>,
    @location(4) uv_lightmap: vec2<f32>,
    @location(5) color:       vec4<f32>,
}

struct VertOut {
    @builtin(position) clip_pos:    vec4<f32>,
    @location(0)       uv:          vec2<f32>,
    @location(1)       uv_lightmap: vec2<f32>,
    @location(2)       color:       vec4<f32>,
    @location(3) @interpolate(flat) instance_id: u32,
    // same base uv but interpolated screen-linearly (no perspective correction).
    // selected in the fragment shader when affine_textures is on → the classic UV "swim".
    @location(4) @interpolate(linear) uv_affine: vec2<f32>,
}

// vertex snapping (matches shader.wgsl); returns input unchanged when off.
fn snap_vertex(clip: vec4<f32>) -> vec4<f32> {
    if globals.vertex_snap <= 0.0 {
        return clip;
    }
    let g = globals.vertex_snap;
    let snapped = round(clip.xy / clip.w * g) / g * clip.w;
    return vec4<f32>(snapped, clip.z, clip.w);
}

@vertex
fn vs_surface(in: VertIn, @builtin(instance_index) instance_id: u32) -> VertOut {
    let inst = instances[instance_id];
    let world_pos4 = inst.model * vec4<f32>(in.position, 1.0);
    var out: VertOut;
    out.clip_pos    = snap_vertex(globals.view_proj * world_pos4);
    out.uv          = in.uv;
    out.uv_lightmap = in.uv_lightmap;
    out.color       = in.color;
    out.instance_id = instance_id;
    out.uv_affine   = in.uv;
    return out;
}

// helper: apply UV transform for a stage
fn apply_uv(base_uv: vec2<f32>, lm_uv: vec2<f32>, stage: StageData) -> vec2<f32> {
    let raw = select(base_uv, lm_uv, stage.use_lm_uv != 0u);
    return (raw + stage.uv_offset) * stage.uv_scale;
}

// sample one texture by stage index
fn sample_stage(stage_idx: u32, uv: vec2<f32>) -> vec4<f32> {
    if stage_idx == 0u { return textureSample(tex0, surf_sampler, uv); }
    if stage_idx == 1u { return textureSample(tex1, surf_sampler, uv); }
    if stage_idx == 2u { return textureSample(tex2, surf_sampler, uv); }
    return textureSample(tex3, surf_sampler, uv);
}

// blend one stage onto the accumulator
fn blend_stage(acc: vec4<f32>, sample: vec4<f32>, stage: StageData) -> vec4<f32> {
    let src_a = sample.a * stage.alpha;
    let src_rgb = sample.rgb;
    switch stage.blend {
        case 0u: { return vec4<f32>(src_rgb, src_a); }                           // opaque
        case 1u: { return vec4<f32>(acc.rgb + src_rgb * src_a, acc.a); }         // add
        case 2u: { return vec4<f32>(acc.rgb * src_rgb, acc.a * src_a); }         // multiply
        default: {                                                                  // alpha blend
            return vec4<f32>(src_rgb * src_a + acc.rgb * (1.0 - src_a), acc.a);
        }
    }
}

@fragment
fn fs_surface(in: VertOut) -> @location(0) vec4<f32> {
    // affine_textures (render_flags bit 2): swap perspective-correct uv for the
    // screen-linear interpolant to recreate early-3D texture warping.
    let base_uv = select(in.uv, in.uv_affine, (globals.render_flags & 4u) != 0u);
    var acc = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    for (var s = 0u; s < 4u; s++) {
        let stage = surface_params.stages[s];
        if stage.enabled == 0u { continue; }
        let uv = apply_uv(base_uv, in.uv_lightmap, stage);
        let sampled = sample_stage(s, uv);
        acc = blend_stage(acc, sampled * in.color, stage);
    }
    return acc;
}
