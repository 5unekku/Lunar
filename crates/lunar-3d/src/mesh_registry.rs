use std::collections::HashMap;

use bevy_ecs::prelude::*;
use lunar_assets::Handle;

use crate::material::MaterialData;
use crate::mesh::MeshData;

/// stores procedurally created meshes and materials by handle id.
///
/// game code adds mesh and material data here and receives cheap
/// [`Handle`]s back. the 3d renderer reads from this registry to
/// upload data to the GPU on first use.
///
/// insert via [`Plugin3d`](crate::plugin::Plugin3d) — no manual setup needed.
///
/// # example
///
/// ```ignore
/// fn setup(mut commands: Commands, mut registry: ResMut<MeshRegistry>) {
///     let floor = registry.add_mesh(quad_mesh(2.0, 2.0));
///     let mat = registry.add_material(MaterialData {
///         base_color: Color::GREEN,
///         ..default()
///     });
///     commands.spawn(Mesh3dBundle { mesh: Mesh3d(floor), material: Material3d(mat), ..default() });
/// }
/// ```
#[derive(Resource, Default)]
pub struct MeshRegistry {
    meshes: HashMap<u32, MeshData>,
    materials: HashMap<u32, MaterialData>,
    next_mesh_id: u32,
    next_mat_id: u32,
}

impl MeshRegistry {
    /// store a [`MeshData`] and return a handle to it.
    pub fn add_mesh(&mut self, mesh: MeshData) -> Handle<MeshData> {
        let id = self.next_mesh_id;
        self.next_mesh_id += 1;
        self.meshes.insert(id, mesh);
        Handle::new(id, 0)
    }

    /// retrieve mesh data by handle.
    pub fn get_mesh(&self, handle: Handle<MeshData>) -> Option<&MeshData> {
        self.meshes.get(&handle.id())
    }

    /// free cpu vertex and index data for a gpu_only mesh after GPU upload.
    pub fn evict_cpu_data(&mut self, handle: Handle<MeshData>) {
        if let Some(mesh) = self.meshes.get_mut(&handle.id())
            && mesh.gpu_only {
                mesh.vertices = Vec::new();
                mesh.indices = crate::mesh::IndexBuffer::U32(Vec::new());
                mesh.skin = None;
            }
    }

    /// store a [`MaterialData`] and return a handle to it.
    pub fn add_material(&mut self, material: MaterialData) -> Handle<MaterialData> {
        let id = self.next_mat_id;
        self.next_mat_id += 1;
        self.materials.insert(id, material);
        Handle::new(id, 0)
    }

    /// retrieve material data by handle.
    pub fn get_material(&self, handle: Handle<MaterialData>) -> Option<&MaterialData> {
        self.materials.get(&handle.id())
    }
}
