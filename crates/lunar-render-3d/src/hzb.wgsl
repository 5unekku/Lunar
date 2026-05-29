// hierarchical Z-buffer (HZB) shaders — three entry points.
//
// cs_copy_depth: copies a Depth32Float texture to the R32Float HZB mip 0.
// cs_downsample: downsamples one HZB mip to the next (MIN of 4 texels).
//   - uses MIN so the HZB stores the nearest occluder depth in each tile.
//   - an entity is occluded if its nearest projected depth > the HZB sample
//     (even the entity's front face is farther than the closest thing in the tile).
// cs_cull_hzb:   tests entity AABBs against last-frame HZB and clears flags
//   for occluded entities. operates on the same visible_flags array as
//   cs_cull in cull.wgsl; cull_flags already zeroed for frustum-culled entities.
//
// all passes run on high-tier only (compute + storage texture required).

// ── depth copy ───────────────────────────────────────────────────────────────

@group(0) @binding(0) var depth_src: texture_depth_2d;
@group(0) @binding(1) var hzb_dst:   texture_storage_2d<r32float, write>;

@compute @workgroup_size(8, 8)
fn cs_copy_depth(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(hzb_dst);
    if gid.x >= size.x || gid.y >= size.y { return; }
    let depth = textureLoad(depth_src, vec2<i32>(gid.xy), 0);
    textureStore(hzb_dst, vec2<i32>(gid.xy), vec4<f32>(depth, 0.0, 0.0, 1.0));
}

// ── mip downsample ───────────────────────────────────────────────────────────

@group(0) @binding(0) var mip_src: texture_2d<f32>;
@group(0) @binding(1) var mip_dst: texture_storage_2d<r32float, write>;

@compute @workgroup_size(8, 8)
fn cs_downsample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(mip_dst);
    if gid.x >= size.x || gid.y >= size.y { return; }
    let src = vec2<i32>(gid.xy) * 2;
    let src_size = textureDimensions(mip_src, 0);
    let s0 = textureLoad(mip_src, clamp(src,                   vec2(0), vec2<i32>(src_size) - 1), 0).r;
    let s1 = textureLoad(mip_src, clamp(src + vec2(1, 0), vec2(0), vec2<i32>(src_size) - 1), 0).r;
    let s2 = textureLoad(mip_src, clamp(src + vec2(0, 1), vec2(0), vec2<i32>(src_size) - 1), 0).r;
    let s3 = textureLoad(mip_src, clamp(src + vec2(1, 1), vec2(0), vec2<i32>(src_size) - 1), 0).r;
    // min depth = nearest occluder in this tile (conservative)
    textureStore(mip_dst, vec2<i32>(gid.xy), vec4<f32>(min(min(s0, s1), min(s2, s3)), 0.0, 0.0, 1.0));
}

// ── hzb occlusion cull ───────────────────────────────────────────────────────

struct AabbEntry {
    center:      vec3<f32>,
    _pad0:       f32,
    half_extent: vec3<f32>,
    _pad1:       f32,
}

struct HzbParams {
    view_proj:    mat4x4<f32>,
    viewport:     vec2<f32>,
    mip_count:    u32,
    entity_count: u32,
}

@group(0) @binding(0) var<storage, read>       hzb_aabbs:   array<AabbEntry>;
@group(0) @binding(1) var<uniform>             hzb_params:  HzbParams;
@group(0) @binding(2) var<storage, read_write> hzb_flags:   array<u32>;
@group(0) @binding(3) var                      hzb_tex:     texture_2d<f32>;

@compute @workgroup_size(64)
fn cs_cull_hzb(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= hzb_params.entity_count { return; }
    // skip already-culled entities (frustum culled)
    if hzb_flags[i] == 0u { return; }

    let center = hzb_aabbs[i].center;
    let he     = hzb_aabbs[i].half_extent;

    // project all 8 AABB corners to clip space; find NDC bounding rect + min depth
    var min_x =  2.0; var max_x = -2.0;
    var min_y =  2.0; var max_y = -2.0;
    var min_z =  1.0;

    for (var k: u32 = 0u; k < 8u; k++) {
        let sx = select(-1.0, 1.0, (k & 1u) != 0u);
        let sy = select(-1.0, 1.0, (k & 2u) != 0u);
        let sz = select(-1.0, 1.0, (k & 4u) != 0u);
        let wp = vec4<f32>(center + vec3<f32>(sx * he.x, sy * he.y, sz * he.z), 1.0);
        let cl = hzb_params.view_proj * wp;
        if cl.w <= 0.0 { return; } // behind camera: keep entity (conservative)
        let ndc = cl.xyz / cl.w;
        min_x = min(min_x, ndc.x); max_x = max(max_x, ndc.x);
        min_y = min(min_y, ndc.y); max_y = max(max_y, ndc.y);
        min_z = min(min_z, ndc.z);
    }

    // entity entirely off-screen: frustum cull already handles this
    if max_x < -1.0 || min_x > 1.0 || max_y < -1.0 || min_y > 1.0 { return; }

    // NDC → UV [0,1]  (y flipped: NDC +1 = UV 0)
    let uv_x0 = (clamp(min_x, -1.0, 1.0) + 1.0) * 0.5;
    let uv_x1 = (clamp(max_x, -1.0, 1.0) + 1.0) * 0.5;
    let uv_y0 = (1.0 - clamp(max_y, -1.0, 1.0)) * 0.5;
    let uv_y1 = (1.0 - clamp(min_y, -1.0, 1.0)) * 0.5;

    // choose mip level covering the entity's screen footprint in ~2×2 texels
    let sw = (uv_x1 - uv_x0) * hzb_params.viewport.x;
    let sh = (uv_y1 - uv_y0) * hzb_params.viewport.y;
    let mip_f = log2(max(sw, sh) * 0.5);
    let mip = clamp(u32(ceil(max(mip_f, 0.0))), 0u, hzb_params.mip_count - 1u);

    // sample 4 texels at mip corners (conservative: use MAX = farthest nearest-depth)
    let ms = vec2<u32>(textureDimensions(hzb_tex, mip));
    let tx0 = clamp(u32(uv_x0 * f32(ms.x)), 0u, ms.x - 1u);
    let ty0 = clamp(u32(uv_y0 * f32(ms.y)), 0u, ms.y - 1u);
    let tx1 = clamp(u32(uv_x1 * f32(ms.x)), 0u, ms.x - 1u);
    let ty1 = clamp(u32(uv_y1 * f32(ms.y)), 0u, ms.y - 1u);
    let s0 = textureLoad(hzb_tex, vec2<u32>(tx0, ty0), mip).r;
    let s1 = textureLoad(hzb_tex, vec2<u32>(tx1, ty0), mip).r;
    let s2 = textureLoad(hzb_tex, vec2<u32>(tx0, ty1), mip).r;
    let s3 = textureLoad(hzb_tex, vec2<u32>(tx1, ty1), mip).r;
    // max of samples: only cull if entity's front face is behind ALL 4 tile nearest-depths
    let hzb_nearest = max(max(s0, s1), max(s2, s3));

    // entity's nearest projected depth > hzb nearest → occluded
    if min_z > hzb_nearest {
        hzb_flags[i] = 0u;
    }
}
