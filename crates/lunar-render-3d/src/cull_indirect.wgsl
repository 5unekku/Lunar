// gpu frustum culling + indirect draw output.
//
// extends cs_cull (cull.wgsl) with draw command output for multi_draw_indexed_indirect_count.
// active on high tier when INDIRECT_FIRST_INSTANCE is available.
//
// every entity owns slot i of indirect_out; a culled one gets instance_count 0. the
// draw count is one past the last visible slot (atomicMax, order-independent). an
// atomicAdd append made draw order depend on workgroup scheduling, so fragments tied
// at exactly equal depth (building bases on the ground) resolved differently from
// frame to frame and run to run under the prepass's Equal test: shimmer, and
// capture-unstable golden frames. the CPU issues one
// multi_draw_indexed_indirect_count call per frame.

struct AabbEntry {
    center:      vec3<f32>,
    _pad0:       f32,
    half_extent: vec3<f32>,
    _pad1:       f32,
}

struct CullParams {
    planes:       array<vec4<f32>, 6>,
    entity_count: u32,
    _pad0:        u32,
    _pad1:        u32,
    _pad2:        u32,
}

// per-entity draw params uploaded by CPU each frame (mega-buffer offsets + entity slot)
struct EntityDrawParams {
    index_count:    u32,
    first_index:    u32,
    base_vertex:    u32,   // i32 bits stored as u32; always >= 0
    first_instance: u32,   // ENTITY_SLOT_START + cull_soa_index
}

// matches wgpu DrawIndexedIndirect hardware layout exactly (20 bytes)
struct DrawIndirectArgs {
    index_count:    u32,
    instance_count: u32,
    first_index:    u32,
    base_vertex:    i32,
    first_instance: u32,
}

@group(0) @binding(0) var<storage, read>       aabbs:         array<AabbEntry>;
@group(0) @binding(1) var<uniform>             params:        CullParams;
@group(0) @binding(2) var<storage, read_write> visible_flags: array<u32>;
@group(0) @binding(3) var<storage, read>       draw_params:   array<EntityDrawParams>;
@group(0) @binding(4) var<storage, read_write> indirect_out:  array<DrawIndirectArgs>;
@group(0) @binding(5) var<storage, read_write> indirect_count: atomic<u32>;

@compute @workgroup_size(64)
fn cs_cull(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.entity_count { return; }

    let center = aabbs[i].center;
    let he     = aabbs[i].half_extent;

    var visible = 1u;
    for (var p: u32 = 0u; p < 6u; p++) {
        let plane = params.planes[p];
        let signed_radius = abs(plane.x) * he.x + abs(plane.y) * he.y + abs(plane.z) * he.z;
        if dot(plane.xyz, center) + plane.w + signed_radius < 0.0 {
            visible = 0u;
            break;
        }
    }
    visible_flags[i] = visible;
    let dp = draw_params[i];
    indirect_out[i] = DrawIndirectArgs(dp.index_count, visible, dp.first_index, bitcast<i32>(dp.base_vertex), dp.first_instance);
    if visible == 1u {
        atomicMax(&indirect_count, i + 1u);
    }
}
