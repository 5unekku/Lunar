//! rerecast baking pipeline: raw triangle geometry → [`navmesh::NavMesh`].
//!
//! call [`bake`] once at map load. the result is stored as a [`super::NavMeshResource`].

use glam::Vec3;
use glam_rc::{UVec3 as RcUVec3, Vec3A as RcVec3A};
use navmesh::{NavMesh, NavResult, NavTriangle, NavVec3};
use rerecast::{
    AreaType, BuildContoursFlags, ConfigBuilder, DetailNavmesh, HeightfieldBuilder, TriMesh,
};

/// configuration for the rerecast baking pipeline.
///
/// defaults are tuned for a human-scale FPS agent (≈1.8 m tall, 0.3 m radius).
pub struct BakeConfig {
    pub agent_height: f32,
    pub agent_radius: f32,
    /// max step the agent can climb (metres)
    pub max_climb: f32,
    /// max walkable slope (degrees)
    pub max_slope_deg: f32,
    /// cell_size = agent_radius / cell_size_fraction
    pub cell_size_fraction: f32,
    /// cell_height = agent_radius / cell_height_fraction
    pub cell_height_fraction: f32,
}

impl Default for BakeConfig {
    fn default() -> Self {
        Self {
            agent_height: 1.8,
            agent_radius: 0.3,
            max_climb: 0.4,
            max_slope_deg: 45.0,
            cell_size_fraction: 3.0,
            cell_height_fraction: 4.0,
        }
    }
}

/// a walkable or obstacle triangle fed to the baking pipeline.
pub struct NavTriangleInput {
    pub vertices: [Vec3; 3],
    pub walkable: bool,
}

impl NavTriangleInput {
    pub fn walkable(a: Vec3, b: Vec3, c: Vec3) -> Self { Self { vertices: [a, b, c], walkable: true } }
    pub fn obstacle(a: Vec3, b: Vec3, c: Vec3) -> Self { Self { vertices: [a, b, c], walkable: false } }
}

#[derive(Debug)]
pub enum BakeError {
    EmptyGeometry,
    /// wraps any rerecast or navmesh error as a string to avoid exposing their types
    Pipeline(String),
}

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") }
}

fn pipe<E: std::fmt::Debug>(e: E) -> BakeError { BakeError::Pipeline(format!("{e:?}")) }

/// bake raw triangle geometry into a [`NavMesh`] ready for runtime path queries.
///
/// slow path — run once at map load, never per-frame.
pub fn bake(triangles: &[NavTriangleInput], config: &BakeConfig) -> Result<NavMesh, BakeError> {
    if triangles.is_empty() {
        return Err(BakeError::EmptyGeometry);
    }

    let trimesh = build_trimesh(triangles);
    let aabb = trimesh.compute_aabb().ok_or(BakeError::EmptyGeometry)?;

    let rc = ConfigBuilder {
        aabb,
        agent_height: config.agent_height,
        agent_radius: config.agent_radius,
        walkable_climb: config.max_climb,
        walkable_slope_angle: config.max_slope_deg.to_radians(),
        cell_size_fraction: config.cell_size_fraction,
        cell_height_fraction: config.cell_height_fraction,
        ..ConfigBuilder::default()
    }.build();

    let mut hf = HeightfieldBuilder {
        aabb: rc.aabb,
        cell_size: rc.cell_size,
        cell_height: rc.cell_height,
    }.build().map_err(pipe)?;

    hf.populate_from_trimesh(trimesh, rc.walkable_height, rc.walkable_climb)
        .map_err(pipe)?;

    let mut chf = hf.into_compact(rc.walkable_height, rc.walkable_climb)
        .map_err(pipe)?;

    chf.erode_walkable_area(rc.walkable_radius);
    chf.build_distance_field();
    chf.build_regions(rc.border_size, rc.min_region_area, rc.merge_region_area)
        .map_err(pipe)?;

    let contours = chf.build_contours(
        rc.max_simplification_error,
        rc.max_edge_len,
        BuildContoursFlags::default(),
    );

    let poly = contours.into_polygon_mesh(rc.max_vertices_per_polygon).map_err(pipe)?;

    let detail = DetailNavmesh::new(&poly, &chf, rc.detail_sample_dist, rc.detail_sample_max_error)
        .map_err(pipe)?;

    detail_to_navmesh(&detail).map_err(|e| BakeError::Pipeline(format!("{e:?}")))
}

/// build a rerecast TriMesh from engine geometry.
/// uses glam-rc (0.30) types so they unify with rerecast's own dependency.
fn build_trimesh(triangles: &[NavTriangleInput]) -> TriMesh {
    let mut verts: Vec<RcVec3A> = Vec::with_capacity(triangles.len() * 3);
    let mut indices: Vec<RcUVec3> = Vec::with_capacity(triangles.len());
    let mut area_types: Vec<AreaType> = Vec::with_capacity(triangles.len());

    for (i, tri) in triangles.iter().enumerate() {
        let base = (i * 3) as u32;
        for v in &tri.vertices {
            verts.push(RcVec3A::new(v.x, v.y, v.z));
        }
        indices.push(RcUVec3::new(base, base + 1, base + 2));
        area_types.push(if tri.walkable {
            AreaType::DEFAULT_WALKABLE
        } else {
            AreaType::NOT_WALKABLE
        });
    }

    TriMesh { vertices: verts, indices, area_types }
}

/// convert a rerecast DetailNavmesh into a navmesh::NavMesh for runtime queries.
fn detail_to_navmesh(detail: &DetailNavmesh) -> NavResult<NavMesh> {
    let verts: Vec<NavVec3> = detail.vertices
        .iter()
        .map(|v| NavVec3::new(v.x, v.y, v.z))
        .collect();

    let tris: Vec<NavTriangle> = detail.meshes
        .iter()
        .flat_map(|submesh| {
            let base_t = submesh.base_triangle_index as usize;
            let base_v = submesh.base_vertex_index;
            detail.triangles[base_t..base_t + submesh.triangle_count as usize]
                .iter()
                .map(move |t| NavTriangle::from([
                    base_v + t[0] as u32,
                    base_v + t[1] as u32,
                    base_v + t[2] as u32,
                ]))
        })
        .collect();

    NavMesh::new(verts, tris)
}
