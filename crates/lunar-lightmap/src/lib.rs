//! CPU lightmap baker for static 3D geometry.
//!
//! bakes directional + ambient occlusion into UV2 lightmap textures at load time.
//! static surfaces are pre-lit once; runtime cost of a static wall's shading is zero.
//! dynamic objects (characters, projectiles) continue using runtime PBR lighting.
//!
//! # workflow
//!
//! ```ignore
//! // 1. assign UV2 coords to static mesh vertices (uv_lightmap field on Vertex3d)
//! // 2. bake the lightmap for the mesh
//! let bake_result = LightmapBaker::new()
//!     .with_resolution(512)
//!     .with_samples(128)
//!     .with_directional(dir_light)
//!     .bake(&mesh_data);
//! // 3. load the resulting RGBA8 image into the asset server
//! // 4. attach Lightmap component to the entity
//! commands.entity(e).insert(Lightmap { texture: handle, intensity: 1.0 });
//! ```
//!
//! # renderer integration
//!
//! the renderer reads `Lightmap` components from entities in the gather pass.
//! when present, the lightmap texture is bound as group 1 binding 1 and the
//! shader samples it at `uv_lightmap` to replace the SH/flat ambient contribution.
//! entities without `Lightmap` use the existing SH ambient path unchanged.

pub mod baker;

pub use baker::{BakeDirectional, BakeResult, LightmapBaker};

/// common, game-facing lightmap components for `use lunar::prelude::*`.
/// the baker API (`LightmapBaker`, `BakeResult`, …) stays at the crate root.
pub mod prelude {
    pub use crate::{DirectionalLightmap, Lightmap};
}

use bevy_ecs::prelude::*;
use lunar_assets::Handle;

/// component: directional lightmap pair for a static `Mesh3d` entity.
///
/// extends `Lightmap` with a second texture storing the dominant light direction
/// per texel, packed as `RGB = dir * 0.5 + 0.5`. use `LightmapBaker::bake_directional`
/// to produce both textures. when present, the renderer modulates baked irradiance
/// by how well the surface normal aligns with the baked dominant direction, giving
/// the appearance of correctly-oriented shading under dynamic relighting.
#[derive(Debug, Clone, Component)]
pub struct DirectionalLightmap {
    /// RGBA8 irradiance texture (same format as `Lightmap.texture`).
    pub irradiance: Handle<lunar_assets::Texture>,
    /// RGBA8 direction texture; RGB = dominant_dir * 0.5 + 0.5.
    pub direction: Handle<lunar_assets::Texture>,
    /// intensity multiplier, same semantics as `Lightmap.intensity`.
    pub intensity: f32,
}

/// component: pre-baked lightmap texture for a static `Mesh3d` entity.
///
/// when present, the renderer samples this texture at `uv_lightmap` (the entity's
/// secondary UV channel) to determine the static ambient lighting contribution.
/// this replaces the runtime SH ambient evaluation for the entity, reducing
/// per-fragment work on surfaces that never change.
///
/// pair with static geometry (no `KinematicBody3d`, not expected to move).
/// dynamic entities (characters, doors) should not have a `Lightmap` component.
#[derive(Debug, Clone, Component)]
pub struct Lightmap {
    /// RGBA8 linear lightmap texture. u=right, v=up, matching the glTF UV convention.
    /// UV2 coordinates on the mesh (`uv_lightmap`) address into this texture.
    pub texture: Handle<lunar_assets::Texture>,
    /// multiplier applied to the lightmap sample before adding to the scene.
    /// 1.0 = physically-based direct intensity. use < 1.0 for artistic darkening.
    pub intensity: f32,
}
