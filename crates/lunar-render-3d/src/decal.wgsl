// box-projected decal renderer.
//
// each decal is a unit cube transformed into world space. the fragment shader
// samples the scene depth buffer to reconstruct the world-space hit position,
// then projects it into decal-local UVs. fragments outside [0,1]³ in decal
// space are discarded. blends over the HDR buffer with alpha.
//
// reference: "Deferred Decals" (Wili, ShaderX7) and John MacDonald's
// "Practical Decals" (GDC 2012, Killzone 3 approach).

struct Globals {
    view_proj:    mat4x4<f32>,
    cam_pos:      vec3<f32>,
    elapsed_secs: f32,
    delta_secs:   f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

struct DecalParams {
    // world→decal-local transform (inverse of the decal's world matrix)
    decal_inv_world: mat4x4<f32>,
    // inverse of the camera view-projection (for depth → world reconstruct)
    inv_view_proj:   mat4x4<f32>,
    color:           vec4<f32>,
    // decal world transform for drawing the box
    decal_world:     mat4x4<f32>,
    screen_width:    f32,
    screen_height:   f32,
    _pad0:           f32,
    _pad1:           f32,
}

@group(0) @binding(0) var<uniform> globals:   Globals;
@group(0) @binding(1) var          depth_tex: texture_2d<f32>;
@group(1) @binding(0) var<uniform> decal:     DecalParams;

struct VertOut {
    @builtin(position) clip_pos:  vec4<f32>,
    @location(0)       world_pos: vec3<f32>,
}

// unit cube vertex data (8 corners × 3 triangles each face = 36 vertices)
fn cube_pos(vi: u32) -> vec3<f32> {
    let positions = array<vec3<f32>, 8>(
        vec3<f32>(-0.5, -0.5, -0.5), vec3<f32>( 0.5, -0.5, -0.5),
        vec3<f32>( 0.5,  0.5, -0.5), vec3<f32>(-0.5,  0.5, -0.5),
        vec3<f32>(-0.5, -0.5,  0.5), vec3<f32>( 0.5, -0.5,  0.5),
        vec3<f32>( 0.5,  0.5,  0.5), vec3<f32>(-0.5,  0.5,  0.5),
    );
    let indices = array<u32, 36>(
        0u,1u,2u, 2u,3u,0u,   // -z
        4u,5u,6u, 6u,7u,4u,   // +z
        0u,4u,7u, 7u,3u,0u,   // -x
        1u,5u,6u, 6u,2u,1u,   // +x
        0u,1u,5u, 5u,4u,0u,   // -y
        3u,2u,6u, 6u,7u,3u,   // +y
    );
    return positions[indices[vi % 36u]];
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    let local_pos = cube_pos(vi);
    let world_pos = (decal.decal_world * vec4<f32>(local_pos, 1.0)).xyz;
    var out: VertOut;
    out.clip_pos  = globals.view_proj * vec4<f32>(world_pos, 1.0);
    out.world_pos = world_pos;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // sample depth at this screen pixel
    let px    = vec2<i32>(i32(in.clip_pos.x), i32(in.clip_pos.y));
    let depth = textureLoad(depth_tex, px, 0).r;
    if depth >= 1.0 { discard; } // no geometry here

    // reconstruct world position from depth
    let uv      = (in.clip_pos.xy / vec2<f32>(decal.screen_width, decal.screen_height));
    let ndc     = vec4<f32>(uv * 2.0 - 1.0, depth, 1.0);
    let world_h = decal.inv_view_proj * ndc;
    let world   = world_h.xyz / world_h.w;

    // project world position into decal-local space
    let local_h = decal.decal_inv_world * vec4<f32>(world, 1.0);
    let local   = local_h.xyz / local_h.w;

    // cull fragments outside the unit cube
    if any(abs(local) > vec3<f32>(0.5)) { discard; }

    // UV from local XZ (decal projected onto Y-down surface)
    let uv_decal = local.xz + vec2<f32>(0.5);
    _ = uv_decal; // future: sample a decal texture atlas here

    // tint with decal color, fade at edges
    let edge_fade = 1.0 - smoothstep(0.4, 0.5, max(abs(local.x), abs(local.z)));
    return vec4<f32>(decal.color.rgb, decal.color.a * edge_fade);
}
