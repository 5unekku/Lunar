//! 3D components and systems for the Lunar engine.
//!
//! this crate provides the building blocks for 3D scenes: transforms, cameras,
//! meshes, materials, and lights. it is intentionally decoupled from any rendering
//! backend — no wgpu code lives here. the render system (a future `engine-render-3d`
//! crate) reads these components and issues GPU draw calls.
//!
//! # design
//!
//! the component model mirrors id Tech 4 (Doom 3) more than id Tech 3 (Quake 3):
//! per-pixel lighting with normal maps is the baseline, not lightmaps. this suits
//! the Halo CE visual target (dynamic lights, per-object specular, skeletal meshes).
//!
//! # quick start
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
//!     // camera
//!     commands.spawn((
//!         LocalTransform3d::from_xyz(0.0, 2.0, 10.0),
//!         WorldTransform3d::default(),
//!         Camera3d::default(),
//!     ));
//!
//!     // mesh entity
//!     commands.spawn((
//!         LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
//!         WorldTransform3d::default(),
//!         Mesh3d(mesh_handle),
//!         Material3d(material_handle),
//!     ));
//! }
//! ```

mod camera;
mod light;
mod material;
mod mesh;
mod plugin;
mod systems;
mod transform;

pub use camera::{ActiveCamera3d, AmbientLight, Camera3d, Projection, update_active_camera};
pub use light::{DirectionalLight, PointLight, SpotLight};
pub use material::{CullMode, Material3d, MaterialData, ShadingModel};
pub use mesh::{IndexBuffer, Mesh3d, MeshData, Vertex3d};
pub use plugin::Plugin3d;
pub use systems::propagate_transforms_3d;
pub use transform::{LocalTransform3d, WorldTransform3d};
