//! pre-composed bundles for common 3D entity configurations.
//!
//! bundles group the components that always go together, so game code can
//! `commands.spawn(Mesh3dBundle { ... })` instead of manually listing every component.
//! all bundles include `WorldTransform3d` — it starts as identity and is overwritten
//! by the propagation system each frame.
//!
//! # example
//!
//! ```ignore
//! commands.spawn(Camera3dBundle {
//!     local: LocalTransform3d::from_xyz(0.0, 2.0, 10.0),
//!     camera: Camera3d { projection: Projection::Perspective { fov_y: 1.05, near: 0.1, far: 500.0 }, ..default() },
//!     ..default()
//! });
//!
//! commands.spawn(Mesh3dBundle {
//!     local: LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
//!     mesh: Mesh3d(mesh_handle),
//!     material: Material3d(mat_handle),
//!     ..default()
//! });
//! ```

use bevy_ecs::bundle::Bundle;

use crate::camera::Camera3d;
use crate::light::{DirectionalLight, PointLight, SpotLight};
use crate::material::Material3d;
use crate::mesh::Mesh3d;
use crate::transform::{LocalTransform3d, WorldTransform3d};
use crate::visibility::{
    Aabb3d, ComputedVisibility, RenderLayers, ShadowCaster, Visibility,
};

/// bundle for a 3D camera.
///
/// spawns with default projection (60° perspective, near 0.1, far 1000) facing -Z.
/// override `local` to position the camera.
#[derive(Bundle)]
pub struct Camera3dBundle {
    pub local: LocalTransform3d,
    pub world: WorldTransform3d,
    pub camera: Camera3d,
    pub render_layers: RenderLayers,
    pub visibility: Visibility,
    pub computed: ComputedVisibility,
}

impl Default for Camera3dBundle {
    fn default() -> Self {
        Self {
            local: LocalTransform3d::default(),
            world: WorldTransform3d::default(),
            camera: Camera3d::default(),
            render_layers: RenderLayers::DEFAULT,
            visibility: Visibility::Visible,
            computed: ComputedVisibility(true),
        }
    }
}

/// bundle for a static (non-animated) mesh entity.
///
/// `mesh` and `material` must be set — there is no sensible default handle.
/// add [`ShadowCaster`] manually if the mesh should cast shadows.
/// add [`Aabb3d`] manually to enable CPU-side frustum culling.
#[derive(Bundle)]
pub struct Mesh3dBundle {
    pub local: LocalTransform3d,
    pub world: WorldTransform3d,
    pub mesh: Mesh3d,
    pub material: Material3d,
    pub visibility: Visibility,
    pub computed: ComputedVisibility,
    pub render_layers: RenderLayers,
}

impl Default for Mesh3dBundle {
    fn default() -> Self {
        Self {
            local: LocalTransform3d::default(),
            world: WorldTransform3d::default(),
            mesh: Mesh3d(lunar_assets::Handle::new(0, 0)),
            material: Material3d(lunar_assets::Handle::new(0, 0)),
            visibility: Visibility::Inherited,
            computed: ComputedVisibility::default(),
            render_layers: RenderLayers::DEFAULT,
        }
    }
}

/// [`Mesh3dBundle`] plus shadow casting and AABB bounds — for any geometry that
/// should cast shadows and participate in frustum culling.
#[derive(Bundle)]
pub struct ShadowMesh3dBundle {
    pub base: Mesh3dBundle,
    pub aabb: Aabb3d,
    pub shadow_caster: ShadowCaster,
}

/// bundle for a directional light (sun/moon).
///
/// direction is taken from the entity's `WorldTransform3d` forward vector.
/// position is irrelevant for directional lights — only rotation matters.
#[derive(Bundle, Default)]
pub struct DirectionalLightBundle {
    pub local: LocalTransform3d,
    pub world: WorldTransform3d,
    pub light: DirectionalLight,
}

/// bundle for a point light.
#[derive(Bundle, Default)]
pub struct PointLightBundle {
    pub local: LocalTransform3d,
    pub world: WorldTransform3d,
    pub light: PointLight,
}

/// bundle for a spot light.
#[derive(Bundle, Default)]
pub struct SpotLightBundle {
    pub local: LocalTransform3d,
    pub world: WorldTransform3d,
    pub light: SpotLight,
}
