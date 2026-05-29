// gpu frustum culling compute shader.
//
// tests each entity AABB against 6 view-frustum planes and writes a per-entity
// visibility flag. replaces the CPU CullSoa frustum loop on high-tier hardware.
//
// frustum planes are in world space, facing inward (point inside if dot >= 0).
// this matches the Gribb/Hartmann extraction used by lunar_3d::Frustum.
//
// workgroup: 64 threads per group. dispatch ceil(entity_count / 64) groups.

struct AabbEntry {
    center:      vec3<f32>,
    _pad0:       f32,
    half_extent: vec3<f32>,
    _pad1:       f32,
}

struct CullParams {
    planes:       array<vec4<f32>, 6>,
    entity_count: u32,
    _pad:         array<u32, 3>,
}

@group(0) @binding(0) var<storage, read>       aabbs:         array<AabbEntry>;
@group(0) @binding(1) var<uniform>             params:        CullParams;
@group(0) @binding(2) var<storage, read_write> visible_flags: array<u32>;

@compute @workgroup_size(64)
fn cs_cull(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.entity_count { return; }

    let center = aabbs[i].center;
    let he     = aabbs[i].half_extent;

    for (var p: u32 = 0u; p < 6u; p++) {
        let plane = params.planes[p];
        // projected half-extent onto the plane normal (conservative expansion)
        let signed_radius = abs(plane.x) * he.x
                          + abs(plane.y) * he.y
                          + abs(plane.z) * he.z;
        // dot(plane.xyz, center) + plane.w + signed_radius < 0 → outside
        if dot(plane.xyz, center) + plane.w + signed_radius < 0.0 {
            visible_flags[i] = 0u;
            return;
        }
    }
    visible_flags[i] = 1u;
}
