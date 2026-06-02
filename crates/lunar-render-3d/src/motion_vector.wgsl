// motion vector pass — per-pixel screen-space reprojection vectors.
//
// for each pixel: reads depth, reconstructs world position via inv_view_proj,
// reprojects to previous frame via prev_view_proj, outputs the 2D NDC delta
// as (cur_ndc - prev_ndc) into an Rg16Float texture.
//
// used as input to custom AA and future temporal effects. covers static geometry
// correctly; dynamic objects with PrevWorldTransform3d contribute implicitly
// through the per-entity staging buffer (handled by the per-entity motion path).

struct MotionVecParams {
    inv_view_proj:  mat4x4<f32>,   // 64 bytes
    prev_view_proj: mat4x4<f32>,   // 64 bytes (previous frame's combined VP)
    screen_width:   f32,
    screen_height:  f32,
    _pad0:          f32,
    _pad1:          f32,
}

@group(0) @binding(0) var<uniform> params:    MotionVecParams;
@group(0) @binding(1) var          depth_tex: texture_depth_2d;
@group(0) @binding(2) var          depth_smp: sampler;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertOut {
    let pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0), vec2<f32>(-1.0, 1.0), vec2<f32>(3.0, 1.0),
    );
    let uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0), vec2<f32>(0.0, 0.0), vec2<f32>(2.0, 0.0),
    );
    var out: VertOut;
    out.clip_pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uvs[vi];
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec2<f32> {
    let depth = textureSample(depth_tex, depth_smp, in.uv);
    if depth >= 0.9999 { return vec2<f32>(0.0); }  // sky — no motion

    // clip space is y-down (vulkan, no auto-flip): uv.y and ndc.y go the same direction.
    let cur_ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, in.uv.y * 2.0 - 1.0);

    // reconstruct world position
    let clip = vec4<f32>(cur_ndc, depth, 1.0);
    let world4 = params.inv_view_proj * clip;
    let world  = world4.xyz / world4.w;

    // reproject to previous frame
    let prev_clip = params.prev_view_proj * vec4<f32>(world, 1.0);
    let prev_ndc  = prev_clip.xy / prev_clip.w;

    return cur_ndc - prev_ndc;
}
