// detail sprite system — GPU-driven billboarded ground cover sprites.
//
// compute pass: reads density map + camera position, generates instance data
//   (world_pos: vec3, scale: f32, variant_u: f32, rotation: f32) for sprites
//   within max_dist. uses a deterministic position hash so sprites are stable
//   as the camera moves.
//
// render pass: one instanced draw per DetailDensity entity. vertex shader
//   positions a quad, billboard-aligns it to the camera, scales by instance.
//   fragment shader alpha-tests from the atlas texture.

// ── compute shader: instance generation ──────────────────────────────────

struct DetailComputeParams {
    cam_pos:        vec3<f32>,
    max_dist:       f32,
    world_origin:   vec2<f32>,  // XZ world origin of the density map area
    world_size:     vec2<f32>,  // XZ world size of the density map area
    grid_step:      f32,        // world units per grid cell
    density_scale:  f32,        // sprites per m² at max density
    size_min:       f32,
    size_max:       f32,
    variant_count:  u32,
    _pad:           u32,
}

struct SpriteInstance {
    position: vec3<f32>,
    scale:    f32,
    variant:  f32,
    _pad0:    f32,
    _pad1:    f32,
    _pad2:    f32,
}

// atomic counter must be wrapped in a struct for WebGPU storage binding
struct InstanceCount { value: atomic<u32> }

@group(0) @binding(0) var<uniform>             detail_params:     DetailComputeParams;
@group(0) @binding(1) var                      density_map:       texture_2d<f32>;
@group(0) @binding(2) var                      density_smp:       sampler;
@group(0) @binding(3) var<storage, read_write> instances:         array<SpriteInstance>;
@group(0) @binding(4) var<storage, read_write> instance_count_ws: InstanceCount;

fn hash3(p: vec3<f32>) -> f32 {
    var h = dot(p, vec3<f32>(127.1, 311.7, 74.7));
    return fract(sin(h) * 43758.5453);
}

@compute @workgroup_size(8, 8)
fn cs_generate_instances(@builtin(global_invocation_id) gid: vec3<u32>) {
    let grid_x = f32(gid.x) * detail_params.grid_step + detail_params.world_origin.x;
    let grid_z = f32(gid.y) * detail_params.grid_step + detail_params.world_origin.y;

    let dist = length(vec2<f32>(grid_x, grid_z) - detail_params.cam_pos.xz);
    if dist > detail_params.max_dist { return; }

    // density fade at max_dist * 0.7
    let fade_start = detail_params.max_dist * 0.7;
    let density_t  = 1.0 - clamp((dist - fade_start) / (detail_params.max_dist - fade_start), 0.0, 1.0);

    // sample density map (textureSampleLevel required in compute — explicit mip 0)
    let uv = (vec2<f32>(grid_x, grid_z) - detail_params.world_origin) / detail_params.world_size;
    let density = textureSampleLevel(density_map, density_smp, clamp(uv, vec2(0.001), vec2(0.999)), 0.0).r;
    let effective_density = density * density_t * detail_params.density_scale;

    // use hash to decide if a sprite spawns at this cell
    let rng = hash3(vec3<f32>(grid_x, 0.0, grid_z));
    if rng > effective_density { return; }

    // generate instance
    let rng2 = hash3(vec3<f32>(grid_x + 1.0, 0.0, grid_z));
    let rng3 = hash3(vec3<f32>(grid_x, 0.0, grid_z + 1.0));
    let rng4 = hash3(vec3<f32>(grid_x + 2.0, 0.0, grid_z));

    let jitter_x = (rng2 - 0.5) * detail_params.grid_step * 0.8;
    let jitter_z = (rng3 - 0.5) * detail_params.grid_step * 0.8;
    let scale    = mix(detail_params.size_min, detail_params.size_max, rng4);
    let variant  = floor(rng * f32(detail_params.variant_count)) / f32(detail_params.variant_count);

    let idx = atomicAdd(&instance_count_ws.value, 1u);
    if idx >= arrayLength(&instances) { return; }

    instances[idx].position = vec3<f32>(grid_x + jitter_x, 0.0, grid_z + jitter_z);
    instances[idx].scale    = scale;
    instances[idx].variant  = variant;
}

// ── render shader: billboard sprite ──────────────────────────────────────

struct SpriteGlobals {
    view_proj: mat4x4<f32>,
    cam_right: vec3<f32>, _p0: f32,
    cam_up:    vec3<f32>, _p1: f32,
    cam_pos:   vec3<f32>, _p2: f32,
}

@group(1) @binding(0) var<uniform> sprite_globals: SpriteGlobals;
@group(1) @binding(1) var          sprite_atlas:   texture_2d<f32>;
@group(1) @binding(2) var          sprite_smp:     sampler;
@group(1) @binding(3) var<storage, read> sprite_instances: array<SpriteInstance>;

struct SpriteVertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
    @location(1)       alpha_t:  f32,
}

// 4 vertices of a quad (CCW)
const QUAD_OFFSETS: array<vec2<f32>, 4> = array<vec2<f32>, 4>(
    vec2<f32>(-0.5,  0.0),
    vec2<f32>( 0.5,  0.0),
    vec2<f32>(-0.5,  1.0),
    vec2<f32>( 0.5,  1.0),
);

@vertex
fn vs_sprite(
    @builtin(vertex_index)   vi: u32,
    @builtin(instance_index) ii: u32,
) -> SpriteVertOut {
    let inst     = sprite_instances[ii];
    let offset   = QUAD_OFFSETS[vi];
    let scale    = inst.scale;
    let world_pos = inst.position
        + sprite_globals.cam_right * offset.x * scale
        + sprite_globals.cam_up    * offset.y * scale;

    var out: SpriteVertOut;
    out.clip_pos = sprite_globals.view_proj * vec4<f32>(world_pos, 1.0);
    let variant_w = 1.0 / 4.0;  // assume 4 variants max; overridden by uniform
    out.uv = vec2<f32>(inst.variant + offset.x * variant_w + 0.5 * variant_w, 1.0 - offset.y);
    out.alpha_t = 1.0;
    return out;
}

@fragment
fn fs_sprite(in: SpriteVertOut) -> @location(0) vec4<f32> {
    let color = textureSample(sprite_atlas, sprite_smp, in.uv);
    if color.a < 0.5 { discard; }
    return vec4<f32>(color.rgb, 1.0);
}
