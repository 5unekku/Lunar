use bevy_ecs::prelude::Component;
use engine_assets::{Asset, Handle};
use engine_math::{Vec2, Vec3};

/// a single vertex in a 3D mesh.
///
/// # layout (matches the wgpu vertex buffer descriptor)
///
/// - `position`: local model space xyz
/// - `normal`: unit surface normal
/// - `tangent`: tangent space +T vector; `w` stores handedness (±1.0).
///   bitangent is reconstructed in the vertex shader as `cross(normal, tangent.xyz) * tangent.w`
///   to avoid storing a redundant vec3 per vertex.
/// - `uv`: primary texture coordinate (diffuse, normal map, specular)
/// - `uv_lightmap`: secondary UV for baked lightmaps. if no lightmap, set equal to `uv`.
/// - `color`: per-vertex RGBA8 color. used for vertex-baked ambient contribution or tinting.
/// - `bone_indices`: up to 4 skeletal joint indices (u8 — max 255 joints per mesh).
/// - `bone_weights`: blend weights summing to 1.0. for rigid meshes, set [1,0,0,0].
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vertex3d {
    pub position: Vec3,
    pub normal: Vec3,
    /// tangent xyz + handedness w. bitangent = cross(normal, tangent.xyz) * tangent.w.
    pub tangent: [f32; 4],
    pub uv: Vec2,
    /// secondary UV for lightmap sampling. mirrors `uv` if no lightmap.
    pub uv_lightmap: Vec2,
    /// vertex color (RGBA8 linear). multiplied with the diffuse sample in the shader.
    pub color: [u8; 4],
    /// skeletal joint indices (indices into the bone matrix array, max 255 joints).
    pub bone_indices: [u8; 4],
    /// blend weights for each bone. must sum to 1.0. use [1,0,0,0] for rigid meshes.
    pub bone_weights: [f32; 4],
}

impl Vertex3d {
    /// create a rigid (non-skinned) vertex with the most common fields.
    #[must_use]
    pub fn rigid(position: Vec3, normal: Vec3, tangent: [f32; 4], uv: Vec2) -> Self {
        Self {
            position,
            normal,
            tangent,
            uv,
            uv_lightmap: uv,
            color: [255, 255, 255, 255],
            bone_indices: [0; 4],
            bone_weights: [1.0, 0.0, 0.0, 0.0],
        }
    }
}

/// index format — 16-bit for meshes under 65536 verts, 32-bit for larger ones.
///
/// prefer u16 where possible: smaller memory footprint, better GPU cache utilization.
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

/// how often this mesh's vertex data changes.
///
/// the render system uses this to pick the right GPU buffer strategy.
/// matches the DM_STATIC / DM_CACHED / DM_CONTINUOUS distinction from id Tech 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshUsage {
    /// uploaded once, never changed. world geometry, props, static architecture.
    Static,
    /// updated infrequently — when the source data changes, not every frame.
    /// skeletal pose results, destructible objects after state change.
    Cached,
    /// rebuilt every frame. particles, cloth, water surfaces, debug lines.
    Streaming,
}

impl Default for MeshUsage {
    fn default() -> Self {
        Self::Static
    }
}

/// raw mesh data: vertex and index buffers in CPU memory.
///
/// the render system uploads this to the GPU. normals and tangents are expected
/// to be pre-computed before upload; see [`MeshData::compute_flat_normals`].
pub struct MeshData {
    pub vertices: Vec<Vertex3d>,
    pub indices: IndexBuffer,
    /// upload strategy hint. see [`MeshUsage`].
    pub usage: MeshUsage,
}

impl MeshData {
    #[must_use]
    pub fn new(vertices: Vec<Vertex3d>, indices: IndexBuffer) -> Self {
        Self {
            vertices,
            indices,
            usage: MeshUsage::Static,
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

/// component that marks an entity as having a 3D mesh.
///
/// pair with [`LocalTransform3d`](crate::transform::LocalTransform3d) and
/// [`Material3d`](crate::material::Material3d) for a fully renderable object.
#[derive(Debug, Clone, Copy, Component)]
pub struct Mesh3d(pub Handle<MeshData>);
