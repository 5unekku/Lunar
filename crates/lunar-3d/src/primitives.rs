//! procedural mesh generators for common primitive shapes.
//!
//! each function returns a [`MeshData`] that can be uploaded to the GPU or
//! combined with other geometry. normals and tangents are pre-computed.
//!
//! # uv convention
//!
//! all primitives map UVs so that (0,0) is the top-left and (1,1) is the bottom-right
//! of each face. tangents follow the glTF 2.0 convention — bitangent is reconstructed
//! as `cross(normal, tangent.xyz) * tangent.w`.

use lunar_math::{Vec2, Vec3};

use crate::mesh::{IndexBuffer, MeshData, Vertex3d};

/// axis-aligned box (rectangular prism) with independently specified half-extents.
///
/// produces 24 vertices (4 per face × 6 faces) so each face has its own normals
/// and UVs without sharing vertices across edges. indices are u16.
///
/// # arguments
///
/// `half_extents` — half the width (X), height (Y), and depth (Z).
#[must_use]
pub fn box_mesh(half_extents: Vec3) -> MeshData {
    let ex = half_extents.x;
    let ey = half_extents.y;
    let ez = half_extents.z;

    // each face: (center_normal, right_axis, up_axis)
    // tangent = right_axis (with w = 1.0)
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),    // +X right
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),    // -X left
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),    // +Y top
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),    // -Y bottom
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y), // -Z front
        (Vec3::Z, Vec3::X, Vec3::Y),        // +Z back
    ];

    // half-extents along right and up axes per face
    let face_extents: [(f32, f32, f32); 6] = [
        (ez, ey, ex), // +X: right=Z up=Y offset=X
        (ez, ey, ex), // -X
        (ex, ez, ey), // +Y: right=X up=Z offset=Y
        (ex, ez, ey), // -Y
        (ex, ey, ez), // -Z: right=X up=Y offset=Z
        (ex, ey, ez), // +Z
    ];

    let mut vertices: Vec<Vertex3d> = Vec::with_capacity(24);
    let mut indices: Vec<u16> = Vec::with_capacity(36);

    for (face_index, ((normal, right, up), (hr, hu, hoff))) in
        faces.iter().zip(face_extents.iter()).enumerate()
    {
        let center = *normal * *hoff;
        let base = face_index as u16 * 4;

        let corners = [
            (Vec2::new(0.0, 0.0), -*right * *hr + *up * *hu),
            (Vec2::new(1.0, 0.0), *right * *hr + *up * *hu),
            (Vec2::new(1.0, 1.0), *right * *hr - *up * *hu),
            (Vec2::new(0.0, 1.0), -*right * *hr - *up * *hu),
        ];

        let tangent = [right.x, right.y, right.z, 1.0];

        for (uv, offset) in &corners {
            vertices.push(Vertex3d::new(center + *offset, *normal, tangent, *uv));
        }

        // two triangles per face
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    MeshData::new(vertices, IndexBuffer::U16(indices))
}

/// unit cube (box with half-extents = 0.5, so side length = 1).
#[must_use]
pub fn unit_cube() -> MeshData {
    box_mesh(Vec3::splat(0.5))
}

/// flat quad in the XZ plane, facing +Y.
///
/// center is at the origin. suitable for floors, terrain patches, and decals.
///
/// # arguments
///
/// - `half_x` — half-width along X
/// - `half_z` — half-depth along Z
#[must_use]
pub fn quad_mesh(half_x: f32, half_z: f32) -> MeshData {
    let normal = Vec3::Y;
    let tangent = [1.0_f32, 0.0, 0.0, 1.0];

    let vertices = vec![
        Vertex3d::new(Vec3::new(-half_x, 0.0, half_z), normal, tangent, Vec2::new(0.0, 0.0)),
        Vertex3d::new(Vec3::new(half_x, 0.0, half_z), normal, tangent, Vec2::new(1.0, 0.0)),
        Vertex3d::new(Vec3::new(half_x, 0.0, -half_z), normal, tangent, Vec2::new(1.0, 1.0)),
        Vertex3d::new(Vec3::new(-half_x, 0.0, -half_z), normal, tangent, Vec2::new(0.0, 1.0)),
    ];
    let indices = IndexBuffer::U16(vec![0, 1, 2, 0, 2, 3]);
    MeshData::new(vertices, indices)
}

/// UV sphere centered at the origin.
///
/// `sectors` controls the number of longitude slices (minimum 4).
/// `stacks` controls the number of latitude rings (minimum 2).
/// higher values produce rounder geometry at the cost of more triangles.
///
/// # arguments
///
/// - `radius` — sphere radius
/// - `sectors` — longitude slices. 16 is low-poly, 32 is smooth.
/// - `stacks` — latitude rings. 8 is low-poly, 16 is smooth.
#[must_use]
pub fn sphere_mesh(radius: f32, sectors: u32, stacks: u32) -> MeshData {
    let sectors = sectors.max(4);
    let stacks = stacks.max(2);

    let mut vertices: Vec<Vertex3d> = Vec::with_capacity(((stacks + 1) * (sectors + 1)) as usize);

    let sector_step = 2.0 * std::f32::consts::PI / sectors as f32;
    let stack_step = std::f32::consts::PI / stacks as f32;

    for stack in 0..=stacks {
        let stack_angle = std::f32::consts::FRAC_PI_2 - stack as f32 * stack_step;
        let xy = radius * stack_angle.cos();
        let z = radius * stack_angle.sin();

        for sector in 0..=sectors {
            let sector_angle = sector as f32 * sector_step;
            let x = xy * sector_angle.cos();
            let y = xy * sector_angle.sin();

            let position = Vec3::new(x, z, y); // Y-up: latitude → Y, longitude → XZ
            let normal = position / radius;
            let uv = Vec2::new(sector as f32 / sectors as f32, stack as f32 / stacks as f32);

            // tangent: derivative of position along longitude (d/d_sector_angle)
            let tangent_dir = Vec3::new(-sector_angle.sin(), 0.0, sector_angle.cos()).normalize_or_zero();
            let tangent = [tangent_dir.x, tangent_dir.y, tangent_dir.z, 1.0];

            vertices.push(Vertex3d::new(position, normal, tangent, uv));
        }
    }

    let ring_width = sectors + 1;
    let mut indices: Vec<u16> = Vec::with_capacity((stacks * sectors * 6) as usize);

    for stack in 0..stacks {
        for sector in 0..sectors {
            let top_left = (stack * ring_width + sector) as u16;
            let top_right = top_left + 1;
            let bottom_left = ((stack + 1) * ring_width + sector) as u16;
            let bottom_right = bottom_left + 1;

            // skip degenerate triangles at poles
            if stack != 0 {
                indices.extend_from_slice(&[top_left, bottom_left, top_right]);
            }
            if stack != stacks - 1 {
                indices.extend_from_slice(&[top_right, bottom_left, bottom_right]);
            }
        }
    }

    let index_buf = if vertices.len() <= u16::MAX as usize {
        IndexBuffer::U16(indices)
    } else {
        IndexBuffer::U32(indices.into_iter().map(|i| i as u32).collect())
    };

    MeshData::new(vertices, index_buf)
}

/// cylinder aligned with the Y axis, with optional end caps.
///
/// # arguments
///
/// - `radius` — radius of the circular cross-section
/// - `height` — total height (center is at origin, extents ±height/2)
/// - `sectors` — number of radial subdivisions (minimum 4)
/// - `caps` — whether to generate top and bottom disc caps
#[must_use]
pub fn cylinder_mesh(radius: f32, height: f32, sectors: u32, caps: bool) -> MeshData {
    let sectors = sectors.max(4);
    let half_h = height * 0.5;

    let mut vertices: Vec<Vertex3d> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let sector_step = 2.0 * std::f32::consts::PI / sectors as f32;

    // side vertices — two rings: bottom and top
    for stack in 0..=1u32 {
        let y = if stack == 0 { -half_h } else { half_h };
        for sector in 0..=sectors {
            let angle = sector as f32 * sector_step;
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            let x = radius * cos_a;
            let z = radius * sin_a;
            let normal = Vec3::new(cos_a, 0.0, sin_a);
            let uv = Vec2::new(sector as f32 / sectors as f32, stack as f32);
            let tangent = [-sin_a, 0.0, cos_a, 1.0];
            vertices.push(Vertex3d::new(Vec3::new(x, y, z), normal, tangent, uv));
        }
    }

    let ring_width = sectors + 1;
    for sector in 0..sectors {
        let bottom = sector;
        let top = sector + ring_width;
        indices.extend_from_slice(&[
            bottom, top, bottom + 1,
            bottom + 1, top, top + 1,
        ]);
    }

    if caps {
        // bottom cap
        let cap_center_uv = Vec2::new(0.5, 0.5);
        let bottom_y = -half_h;
        let top_y = half_h;

        for (y, normal_y, _v_offset) in [
            (bottom_y, -1.0_f32, 0),
            (top_y, 1.0_f32, sectors as usize + 1),
        ] {
            let center_idx = vertices.len() as u32;
            let normal = Vec3::new(0.0, normal_y, 0.0);
            let tangent = [1.0_f32, 0.0, 0.0, if normal_y > 0.0 { 1.0 } else { -1.0 }];
            vertices.push(Vertex3d::new(Vec3::new(0.0, y, 0.0), normal, tangent, cap_center_uv));

            let ring_start = vertices.len() as u32;
            for sector in 0..=sectors {
                let angle = sector as f32 * sector_step;
                let cos_a = angle.cos();
                let sin_a = angle.sin();
                let uv = Vec2::new(cos_a * 0.5 + 0.5, sin_a * 0.5 + 0.5);
                vertices.push(Vertex3d::new(
                    Vec3::new(radius * cos_a, y, radius * sin_a),
                    normal,
                    tangent,
                    uv,
                ));
            }

            for sector in 0..sectors {
                let a = ring_start + sector;
                let b = ring_start + sector + 1;
                if normal_y > 0.0 {
                    indices.extend_from_slice(&[center_idx, a, b]);
                } else {
                    indices.extend_from_slice(&[center_idx, b, a]);
                }
            }

        }
    }

    MeshData::new(vertices, IndexBuffer::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_has_24_vertices_36_indices() {
        let mesh = box_mesh(Vec3::splat(1.0));
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
    }

    #[test]
    fn quad_has_4_vertices_6_indices() {
        let mesh = quad_mesh(1.0, 1.0);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
    }

    #[test]
    fn sphere_vertex_count() {
        let mesh = sphere_mesh(1.0, 16, 8);
        assert_eq!(mesh.vertices.len(), (17 * 9) as usize); // (sectors+1) * (stacks+1)
    }

    #[test]
    fn sphere_vertices_on_unit_sphere() {
        let mesh = sphere_mesh(1.0, 16, 8);
        for vertex in &mesh.vertices {
            let len = vertex.position.length();
            assert!((len - 1.0).abs() < 0.001, "vertex not on sphere: {len}");
        }
    }

    #[test]
    fn cylinder_has_side_and_caps() {
        let mesh = cylinder_mesh(1.0, 2.0, 8, true);
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());
    }
}
