struct LodParams { cam_pos: vec3<f32>, entity_count: u32, thresholds: vec4<f32> }
@group(0) @binding(0) var<uniform>             params:      LodParams;
@group(0) @binding(1) var<storage, read>       aabbs:       array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> lod_indices: array<u32>;
@compute @workgroup_size(64)
fn cs_lod_select(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.entity_count { return; }
    let centre = aabbs[i * 2u].xyz;
    let d = centre - params.cam_pos;
    let dist_sq = dot(d, d);
    let t = params.thresholds;
    var lod: u32 = 4u;
    if      dist_sq <= t.x { lod = 0u; }
    else if dist_sq <= t.y { lod = 1u; }
    else if dist_sq <= t.z { lod = 2u; }
    else if dist_sq <= t.w { lod = 3u; }
    lod_indices[i] = lod;
}
