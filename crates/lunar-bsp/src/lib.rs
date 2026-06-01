//! BVH spatial acceleration and portal culling for Lunar 3D scenes.
//!
//! two independent systems:
//!
//! **BVH (bounding volume hierarchy)**: replaces the O(n) CullSoa frustum test with
//! a dynamic AABB tree. entities with [`Aabb3d`] are inserted each frame; the tree
//! is queried against the frustum to produce a compact visible set without scanning
//! every entity. complexity: O(log n) per ray/frustum query.
//!
//! **portal culling**: for indoor levels, designers place [`Portal`] entities marking
//! boundaries between [`Area`]s (rooms, corridors). the portal system runs a BFS from
//! the camera's area through portals whose bounding rects are within the camera frustum.
//! only entities in reachable areas are submitted for rendering. a single closed door
//! eliminates an entire wing of the level at zero GPU cost.
//!
//! # quick start
//!
//! ```ignore
//! // tag static entities with an area id and add portals between rooms
//! commands.spawn((Mesh3dBundle { .. }, Area(0)));
//! commands.spawn(PortalBundle {
//!     portal: Portal { area_a: 0, area_b: 1, .. },
//!     ..
//! });
//!
//! // add the plugin — registers BVH build + portal cull systems
//! app.add_plugin(BspPlugin);
//! ```
//!
//! entities without an [`Area`] component are always considered visible (the portal
//! system only prunes entities that are tagged, leaving untagged entities unaffected).

pub mod bvh;
pub mod level;
pub mod portal;
pub mod plugin;

pub use bvh::{Bvh, BvhNode, BvhPlugin, BvhVisible};
pub use level::{BspBlob, BspLevel, BspNode, PortalData};
pub use portal::{Area, Portal, PortalCulling, PortalPlugin, VisibleAreas};
pub use plugin::BspPlugin;

/// common, game-facing BSP/BVH types for `use lunar::prelude::*`.
/// the full surface (BVH nodes, portal/level internals, …) stays at the crate root.
pub mod prelude {
    pub use crate::{Area, BspPlugin, BvhPlugin, PortalPlugin};
}
