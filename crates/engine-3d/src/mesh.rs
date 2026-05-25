use bevy_ecs::prelude::Component;
use engine_assets::{Asset, Handle};
use engine_math::{Vec2, Vec3};

/// a single vertex in a 3D mesh.
///
/// layout matches the vertex buffer expected by the 3D render pipeline.
/// tangent is the +T basis vector for normal map tangent space; handedness
/// is stored in the W component (+1.0 or -1.0) to reconstruct the bitangent.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Vertex3d {
    /// position in local model space.
    pub position: Vec3,
    /// surface normal (unit length).
    pub normal: Vec3,
    /// tangent vector (xyz) and handedness (w = ±1.0).
    pub tangent: [f32; 4],
    /// primary texture coordinate.
    pub uv: Vec2,
}

/// index format for a mesh — 16-bit for small meshes, 32-bit for large ones.
#[derive(Debug, Clone)]
pub enum IndexBuffer {
    U16(Vec<u16>),
    U32(Vec<u32>),
}

impl IndexBuffer {
    /// number of indices in this buffer.
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

/// raw mesh data: vertex and index buffers in CPU memory.
///
/// the render system uploads this to the GPU on first use.
/// normals and tangents are expected to be pre-computed; see [`MeshData::compute_normals`].
pub struct MeshData {
    pub vertices: Vec<Vertex3d>,
    pub indices: IndexBuffer,
}

impl MeshData {
    /// create a mesh from pre-built vertex and index data.
    #[must_use]
    pub fn new(vertices: Vec<Vertex3d>, indices: IndexBuffer) -> Self {
        Self { vertices, indices }
    }

    /// flat-shade normals from the triangle faces. overwrites existing normals.
    ///
    /// suitable for low-poly / faceted look. for smooth meshes, average normals
    /// per shared vertex instead.
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
/// pair with a [`LocalTransform3d`](crate::transform::LocalTransform3d) and a
/// [`Material3d`](crate::material::Material3d) to get a fully renderable object.
#[derive(Debug, Clone, Copy, Component)]
pub struct Mesh3d(pub Handle<MeshData>);
