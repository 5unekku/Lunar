// geometry clipmap terrain — Losasso/Hoppe 2004 "Geometry clipmaps: terrain rendering
// using nested regular grids".
//
// one draw call per clipmap ring. each ring is a regular NxN quad mesh in
// normalised [0,1]² space. the vertex shader reads the heightmap to displace Y.
// normals are estimated from a 3-tap central-difference stencil in the heightmap.
// cracks between rings are hidden by a one-row skirt (outermost vertices snap down).
//
// lighting: simple directional diffuse + ambient. no shadow support in this pass.

struct Globals {
    view_proj:    mat4x4<f32>,
    cam_pos:      vec3<f32>,
    elapsed_secs: f32,
    delta_secs:   f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

struct TerrainParams {
    // ring's origin in world space (camera-snapped to lod_scale cell boundary)
    ring_origin:    vec4<f32>,   // .xyz = origin, .w = unused
    // terrain entity world offset (entity LocalTransform origin)
    terrain_origin: vec4<f32>,   // .xyz = offset, .w = unused
    // scale of each cell in this ring (lod=0: finest, lod=k: 2^k * base)
    lod_cell_size:  f32,
    // world-space size of the heightmap
    world_size:     f32,
    // maps [0,1] heightmap sample to world Y
    height_scale:   f32,
    // number of quads per side in this ring mesh
    ring_resolution: f32,
    // tint colour rgba
    tint:           vec4<f32>,
    // sun direction (normalised, points toward sun)
    sun_dir:        vec4<f32>,   // .xyz = dir, .w = intensity
    // ambient
    ambient:        f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> terrain: TerrainParams;
@group(1) @binding(1) var          heightmap: texture_2d<f32>;
@group(1) @binding(2) var          hmap_smp:  sampler;

struct VertOut {
    @builtin(position) clip_pos:  vec4<f32>,
    @location(0)       world_pos: vec3<f32>,
    @location(1)       normal:    vec3<f32>,
    @location(2)       height_t:  f32,       // 0 = valley, 1 = peak (for tint blending)
}

fn sample_height(uv: vec2<f32>) -> f32 {
    let clamped = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(heightmap, hmap_smp, clamped, 0.0).r * terrain.height_scale;
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) _normal:  vec3<f32>,
    @location(2) _color:   vec4<f32>,
    @location(3) _uv0:     vec2<f32>,
    @location(4) _uv1:     vec2<f32>,
    @location(5) _tint:    u32,
) -> VertOut {
    // position.xz are in [0, ring_resolution] grid coordinates.
    // map to world space: ring_origin + position * lod_cell_size.
    let world_xz = terrain.ring_origin.xz + position.xz * terrain.lod_cell_size;

    // heightmap UV from world XZ
    let hmap_uv = (world_xz - terrain.terrain_origin.xz) / terrain.world_size;

    // sample height and neighbours for normal estimation
    let texel = 1.0 / terrain.world_size * terrain.lod_cell_size;
    let h  = sample_height(hmap_uv);
    let hx = sample_height(hmap_uv + vec2<f32>(texel, 0.0));
    let hz = sample_height(hmap_uv + vec2<f32>(0.0, texel));

    // central-difference normal (approximate but fast)
    let dx = vec3<f32>(terrain.lod_cell_size, hx - h, 0.0);
    let dz = vec3<f32>(0.0, hz - h, terrain.lod_cell_size);
    let world_normal = normalize(cross(dz, dx));

    let world_y = terrain.terrain_origin.y + h;
    let world_pos = vec3<f32>(world_xz.x, world_y, world_xz.y);

    var out: VertOut;
    out.clip_pos  = globals.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_pos = world_pos;
    out.normal    = world_normal;
    out.height_t  = clamp(h / terrain.height_scale, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let sun = normalize(terrain.sun_dir.xyz);
    let diff = max(dot(n, sun), 0.0) * terrain.sun_dir.w;
    let light = terrain.ambient + diff;

    // simple height-based colour: blend low-tint (valley) to tint (peak)
    let low_tint = vec4<f32>(terrain.tint.rgb * 0.6, terrain.tint.a);
    let surface = mix(low_tint, terrain.tint, in.height_t);

    return vec4<f32>(surface.rgb * light, surface.a);
}
