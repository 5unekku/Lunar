use bevy_ecs::prelude::Component;
use lunar_assets::{Asset, Handle};
use lunar_math::{Vec2, Vec3};

/// a vertex in a rigid (non-skinned) 3D mesh.
///
/// # layout
///
/// - `position`: local model space xyz
/// - `normal`: unit surface normal
/// - `tangent`: tangent space +T vector; `w` stores handedness (±1.0).
///   bitangent is reconstructed in the vertex shader as `cross(normal, tangent.xyz) * tangent.w`,
///   which is the glTF 2.0 standard convention.
/// - `uv`: primary texture coordinate (diffuse, normal map, specular)
/// - `uv_lightmap`: secondary UV for baked lightmaps. mirrors `uv` if no lightmap is used.
/// - `color`: per-vertex RGBA8 linear color. multiplied with diffuse in the shader.
///   use `[255,255,255,255]` (white) for no tinting.
///
/// # normal map convention
///
/// normal maps store only the XY tangent-space components (RG channels).
/// the shader reconstructs Z as `sqrt(1.0 - saturate(dot(xy, xy)))`.
/// this aligns with BC5 block compression, which only stores two channels anyway.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vertex3d {
    pub position: Vec3,
    pub normal: Vec3,
    /// tangent xyz + handedness w (±1.0). bitangent = cross(normal, tangent.xyz) * tangent.w.
    pub tangent: [f32; 4],
    pub uv: Vec2,
    /// secondary UV for lightmap sampling. mirrors `uv` if no lightmap.
    pub uv_lightmap: Vec2,
    /// per-vertex RGBA8 linear color. `[255,255,255,255]` = no tint.
    pub color: [u8; 4],
}

impl Vertex3d {
    /// create a vertex with sensible defaults (white, uv_lightmap mirrors uv).
    #[must_use]
    pub fn new(position: Vec3, normal: Vec3, tangent: [f32; 4], uv: Vec2) -> Self {
        Self {
            position,
            normal,
            tangent,
            uv,
            uv_lightmap: uv,
            color: [255, 255, 255, 255],
        }
    }
}

/// additional per-vertex data for skeletal (skinned) meshes.
///
/// stored separately from [`Vertex3d`] so static geometry does not pay for
/// bone data it never uses. the render system selects the vertex layout based
/// on whether a skeleton is present.
///
/// up to 4 joint influences per vertex (sufficient for characters; 2 covers
/// most rigid-body joints). weights must sum to 1.0; unused slots are zero.
/// joint indices address into the bone matrix array uploaded per draw call.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct SkinWeights {
    /// indices into the bone matrix array. max 255 joints per mesh.
    pub bone_indices: [u8; 4],
    /// blend weights. must sum to 1.0. unused influences = 0.0.
    pub bone_weights: [f32; 4],
}

impl SkinWeights {
    /// rigid binding to a single joint (weight 1.0 on bone 0, rest zero).
    #[must_use]
    pub const fn rigid(bone: u8) -> Self {
        Self {
            bone_indices: [bone, 0, 0, 0],
            bone_weights: [1.0, 0.0, 0.0, 0.0],
        }
    }
}

impl Default for SkinWeights {
    fn default() -> Self {
        Self::rigid(0)
    }
}

/// index format — 16-bit for meshes under 65536 verts, 32-bit for larger ones.
///
/// prefer u16 where possible: half the index buffer size, better GPU cache utilization.
#[derive(Debug, Clone)]
pub enum IndexBuffer {
    U16(Vec<u16>),
    U32(Vec<u32>),
}

impl IndexBuffer {
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::U16(v) => v.len(),
            Self::U32(v) => v.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// how often this mesh's vertex data changes on the GPU.
///
/// the render system uses this to pick the appropriate buffer strategy.
///
/// - `Static`: uploaded once, never modified. world geometry, props, architecture.
/// - `Cached`: re-uploaded when the source data changes (e.g. after a pose update).
///   skeletal pose results, destructibles after a state change.
/// - `Streaming`: rebuilt and re-uploaded every frame. particles, cloth, water, debug lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeshUsage {
    #[default]
    Static,
    Cached,
    Streaming,
}

/// raw mesh data: vertex and index buffers in CPU memory.
///
/// the render system uploads this to the GPU. normals and tangents are expected
/// to be pre-computed before upload; see [`MeshData::compute_flat_normals`].
///
/// skinned meshes additionally provide [`SkinWeights`] parallel to `vertices`.
pub struct MeshData {
    pub vertices: Vec<Vertex3d>,
    pub indices: IndexBuffer,
    /// per-vertex skin weights. `None` for rigid (non-animated) meshes.
    /// when `Some`, must have the same length as `vertices`.
    pub skin: Option<Vec<SkinWeights>>,
    pub usage: MeshUsage,
}

impl MeshData {
    /// create a rigid (non-skinned) static mesh.
    #[must_use]
    pub fn new(vertices: Vec<Vertex3d>, indices: IndexBuffer) -> Self {
        Self {
            vertices,
            indices,
            skin: None,
            usage: MeshUsage::Static,
        }
    }

    /// create a skinned mesh with bone weights.
    ///
    /// # Panics
    /// panics if `skin.len() != vertices.len()`.
    #[must_use]
    pub fn new_skinned(
        vertices: Vec<Vertex3d>,
        indices: IndexBuffer,
        skin: Vec<SkinWeights>,
    ) -> Self {
        assert_eq!(
            vertices.len(),
            skin.len(),
            "skin weights must match vertex count"
        );
        Self {
            vertices,
            indices,
            skin: Some(skin),
            usage: MeshUsage::Cached,
        }
    }

    #[must_use]
    pub fn with_usage(mut self, usage: MeshUsage) -> Self {
        self.usage = usage;
        self
    }

    /// compute flat (faceted) normals from triangle faces. overwrites existing normals.
    ///
    /// for smooth shading, average normals across shared vertices instead.
    pub fn compute_flat_normals(&mut self) {
        let indices: Vec<usize> = match &self.indices {
            IndexBuffer::U16(v) => v.iter().map(|&i| i as usize).collect(),
            IndexBuffer::U32(v) => v.iter().map(|&i| i as usize).collect(),
        };
        for vertex in &mut self.vertices {
            vertex.normal = Vec3::ZERO;
        }
        for tri in indices.chunks_exact(3) {
            let a = self.vertices[tri[0]].position;
            let b = self.vertices[tri[1]].position;
            let c = self.vertices[tri[2]].position;
            let n = (b - a).cross(c - a).normalize_or_zero();
            self.vertices[tri[0]].normal = n;
            self.vertices[tri[1]].normal = n;
            self.vertices[tri[2]].normal = n;
        }
    }
}

impl Asset for MeshData {}

/// impostor atlas: a pre-rendered texture of an object from multiple angles.
///
/// the atlas stores `angle_count` images arranged in a single row:
/// column i = the object rendered from azimuth `i * (360 / angle_count)` degrees.
/// generated offline (or by [`MeshImpostor::bake`]) and loaded as a regular texture.
#[derive(Debug, Clone)]
pub struct ImpostorAtlas {
    /// texture handle for the multi-angle atlas
    pub texture: Handle<lunar_assets::Texture>,
    /// number of angles baked into the atlas (equally spaced around 360°)
    pub angle_count: u32,
    /// width × height in texels of each individual angle frame
    pub frame_width: u32,
    pub frame_height: u32,
}

impl ImpostorAtlas {
    /// UV rect for the closest pre-baked angle to `view_angle_rad` (azimuth around Y).
    ///
    /// returns `(u_min, u_max, v_min, v_max)` in atlas UV space [0,1].
    #[must_use]
    pub fn uv_rect(&self, view_angle_rad: f32) -> (f32, f32, f32, f32) {
        use std::f32::consts::TAU;
        let angle_step = TAU / self.angle_count as f32;
        let norm = ((view_angle_rad % TAU) + TAU) % TAU;
        let frame = ((norm / angle_step).round() as u32) % self.angle_count;
        let u_step = 1.0 / self.angle_count as f32;
        let u_min = frame as f32 * u_step;
        (u_min, u_min + u_step, 0.0, 1.0)
    }
}

/// impostor billboard for far-distance rendering.
///
/// when present on a `Mesh3d` entity and the camera is beyond `min_dist_sq`, the
/// renderer substitutes a camera-facing quad rendered with the impostor atlas texture
/// instead of the full mesh. zero vertex throughput: just one quad (2 triangles).
///
/// pairs naturally with [`MeshLod`] — add the coarsest LOD level at the distance
/// where the mesh still looks correct, then add `MeshImpostor` to kick in beyond that.
///
/// # workflow
///
/// 1. render the object from `atlas.angle_count` angles offline
/// 2. pack into a single-row texture atlas (left to right, 0° → 360°)
/// 3. load the atlas texture and store its handle in `ImpostorAtlas`
/// 4. spawn the entity with `Mesh3d`, optional `MeshLod`, and `MeshImpostor`
#[derive(Debug, Clone, Component)]
pub struct MeshImpostor {
    /// squared camera distance beyond which the impostor is used instead of the mesh
    pub min_dist_sq: f32,
    /// the multi-angle pre-rendered atlas
    pub atlas: ImpostorAtlas,
    /// world-space half-extents of the impostor billboard quad (matches the object's visual size)
    pub half_width: f32,
    pub half_height: f32,
}

/// component that marks an entity as having a 3D mesh.
///
/// pair with [`LocalTransform3d`](crate::transform::LocalTransform3d) and
/// [`Material3d`](crate::material::Material3d) for a fully renderable object.
#[derive(Debug, Clone, Copy, Component)]
pub struct Mesh3d(pub Handle<MeshData>);

/// discrete LOD levels for a `Mesh3d` entity.
///
/// levels must be sorted ascending by `max_dist_sq`. the render system selects
/// the first level whose threshold is not exceeded by the entity's squared
/// distance from the camera, or falls back to the last (coarsest) level.
///
/// the `Mesh3d` component serves as LOD 0 (base mesh). `MeshLod` overrides
/// the mesh handle in the gather pass when the distance thresholds are met.
///
/// # example
///
/// ```ignore
/// let lod = MeshLod::new(vec![
///     (100.0 * 100.0, lod1_handle), // within 100 units: use lod1
///     (300.0 * 300.0, lod2_handle), // within 300 units: use lod2
///     // beyond 300 units: still use lod2 (last level)
/// ]);
/// ```
#[derive(Debug, Clone, Component)]
pub struct MeshLod {
    /// `(max_dist_sq, mesh_handle)` sorted ascending by `max_dist_sq`.
    pub levels: Vec<(f32, Handle<MeshData>)>,
}

impl MeshLod {
    /// construct and sort levels by `max_dist_sq`.
    #[must_use]
    pub fn new(mut levels: Vec<(f32, Handle<MeshData>)>) -> Self {
        levels.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        Self { levels }
    }

    /// select the mesh handle for `dist_sq` (squared camera distance).
    ///
    /// returns the first level whose `max_dist_sq >= dist_sq`, or the last
    /// (coarsest) level if all thresholds are exceeded.
    #[must_use]
    pub fn select(&self, dist_sq: f32) -> Option<Handle<MeshData>> {
        self.levels.iter()
            .find(|(max_d_sq, _)| dist_sq <= *max_d_sq)
            .or_else(|| self.levels.last())
            .map(|(_, handle)| *handle)
    }
}
