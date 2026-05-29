// cluster light assignment — one thread per cluster cell, tests all lights.
// produces cluster_counts and cluster_light_indices for the fragment shader.
//
// cluster grid: CLUSTER_X × CLUSTER_Y × CLUSTER_Z (16×9×24 = 3456 cells).
// each cell holds up to MAX_LIGHTS_PER_CLUSTER (32) light indices.

const CLUSTER_X: u32 = 16u;
const CLUSTER_Y: u32 = 9u;
const CLUSTER_Z: u32 = 24u;
const MAX_LIGHTS_PER_CLUSTER: u32 = 32u;

struct ClusterParams {
    view_proj:   mat4x4<f32>,   // 64 bytes
    screen_w:    u32,            //  4 bytes (offset 64)
    screen_h:    u32,            //  4 bytes
    light_count: u32,            //  4 bytes
    _pad0:       u32,            //  4 bytes
    near:        f32,            //  4 bytes (offset 80)
    far:         f32,            //  4 bytes
    focal_x:     f32,            //  4 bytes  (proj[0][0])
    _pad1:       f32,            //  4 bytes  — total: 96 bytes
}

struct PointLightEntry {
    position:    vec3<f32>,
    intensity:   f32,
    color:       vec3<f32>,
    radius:      f32,
    shadow_index: u32,
    _pad:        vec3<u32>,
}

@group(0) @binding(0) var<uniform>             cluster_params:       ClusterParams;
@group(0) @binding(1) var<storage, read>       light_list:           array<PointLightEntry>;
@group(0) @binding(2) var<storage, read_write> cluster_counts:       array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> cluster_light_indices: array<u32>;

@compute @workgroup_size(1, 1, 1)
fn cs_cluster_assign(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cx = gid.x; let cy = gid.y; let cz = gid.z;
    if cx >= CLUSTER_X || cy >= CLUSTER_Y || cz >= CLUSTER_Z { return; }

    let cluster_idx = cz * CLUSTER_X * CLUSTER_Y + cy * CLUSTER_X + cx;

    // clear this cluster's count
    atomicStore(&cluster_counts[cluster_idx], 0u);

    let near = cluster_params.near;
    let far = cluster_params.far;

    // tile bounds in screen pixels
    let tile_w = f32(cluster_params.screen_w) / f32(CLUSTER_X);
    let tile_h = f32(cluster_params.screen_h) / f32(CLUSTER_Y);
    let tile_x0 = f32(cx) * tile_w;
    let tile_x1 = f32(cx + 1u) * tile_w;
    let tile_y0 = f32(cy) * tile_h;
    let tile_y1 = f32(cy + 1u) * tile_h;

    // exponential depth slice: z_near * (z_far/z_near)^(cz/nz)
    let z_near_slice = near * pow(far / near, f32(cz) / f32(CLUSTER_Z));
    let z_far_slice  = near * pow(far / near, f32(cz + 1u) / f32(CLUSTER_Z));

    let half_screen_w = f32(cluster_params.screen_w) * 0.5;
    let half_screen_h = f32(cluster_params.screen_h) * 0.5;

    for (var i = 0u; i < cluster_params.light_count; i++) {
        let light = light_list[i];
        let clip = cluster_params.view_proj * vec4<f32>(light.position, 1.0);
        let depth = clip.w;
        if depth <= 0.0 { continue; }

        let ndc = clip.xy / depth;
        // screen-space pixel center of light (NDC y flipped for screen y-down)
        let sx = (ndc.x * 0.5 + 0.5) * f32(cluster_params.screen_w);
        let sy = (0.5 - ndc.y * 0.5) * f32(cluster_params.screen_h);
        // projected screen-space radius (focal_x scales x NDC to pixels)
        let proj_r = cluster_params.focal_x * light.radius / depth * half_screen_w;

        if sx + proj_r < tile_x0 || sx - proj_r >= tile_x1 { continue; }
        if sy + proj_r < tile_y0 || sy - proj_r >= tile_y1 { continue; }
        if depth + light.radius < z_near_slice || depth - light.radius > z_far_slice { continue; }

        let slot = atomicAdd(&cluster_counts[cluster_idx], 1u);
        if slot < MAX_LIGHTS_PER_CLUSTER {
            cluster_light_indices[cluster_idx * MAX_LIGHTS_PER_CLUSTER + slot] = i;
        }
    }
}
