//! portal-based area culling for indoor 3D levels.
//!
//! designers place [`Portal`] entities at boundaries between areas (rooms, corridors).
//! each portal connects two [`Area`] ids and has a bounding rect in world space.
//! at runtime, the system does a BFS from the camera's area through portals whose
//! screen-projected rects overlap the current frustum. only entities in reachable
//! areas are visible; an entire wing of a level behind a closed portal is culled
//! at zero GPU cost.
//!
//! entities without an [`Area`] component are never portal-culled (they're always
//! visible if frustum-visible). pair with `BvhPlugin` for the frustum test itself.
//!
//! # portal design
//!
//! portals are two-sided — area_a and area_b are symmetric. opening or closing a portal
//! is done by removing/adding a [`PortalOpen`] component (missing = closed = culls both
//! sides). multiple portals can connect the same pair of areas (e.g. two windows).

use bevy_ecs::prelude::*;
use lunar_3d::{ActiveCamera3d, WorldTransform3d};
use lunar_math::Vec3;
use std::collections::{HashSet, VecDeque};

use lunar_core::{App, GamePlugin, UpdateStage};

/// tags an entity as belonging to a named area.
///
/// the portal culling system only prunes entities with this component.
/// entities without `Area` are always considered visible.
#[derive(Debug, Clone, Copy, Component)]
pub struct Area(pub u32);

/// marks a portal between two areas.
///
/// the portal is a convex bounding volume defined by a center point and half-extents.
/// when the camera's frustum overlaps the portal's AABB, the connected area becomes
/// potentially visible. when the portal is absent from an entity or has no
/// [`PortalOpen`] component, the portal is treated as closed (no visibility through it).
#[derive(Debug, Clone, Component)]
pub struct Portal {
    /// first area this portal connects
    pub area_a: u32,
    /// second area this portal connects
    pub area_b: u32,
    /// world-space center of the portal opening
    pub center: Vec3,
    /// world-space half-extents of the portal opening
    pub half_extents: Vec3,
}

/// marker component — this portal is currently open (visibility passes through).
///
/// remove this component to close the portal (e.g. a door closes; the room behind it
/// becomes invisible and its entities are culled from the draw list).
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct PortalOpen;

/// resource: area ids reachable from the camera this frame.
///
/// populated by `cull_portals`. game code and the renderer read this to filter
/// draw lists to only entities in reachable areas.
#[derive(Resource, Default)]
pub struct VisibleAreas {
    pub area_ids: HashSet<u32>,
    /// true if portal culling ran this frame (false = no camera / no areas)
    pub active: bool,
}

impl VisibleAreas {
    /// returns true if entities with this area should be drawn.
    ///
    /// entities with no Area component should always return true (caller's responsibility).
    #[must_use]
    pub fn contains(&self, area: u32) -> bool {
        !self.active || self.area_ids.contains(&area)
    }
}

/// resource inserted by [`PortalPlugin`] to configure portal culling.
#[derive(Resource, Clone)]
pub struct PortalCulling {
    /// maximum BFS depth (number of portal hops visible from camera).
    /// lower = more aggressive culling but may miss geometry in deep levels.
    /// default: 8 (enough for any sane indoor level).
    pub max_depth: u32,
}

impl Default for PortalCulling {
    fn default() -> Self {
        Self { max_depth: 8 }
    }
}

/// system: determine which areas are visible from the camera via portal traversal.
///
/// runs each render frame. camera must have `WorldTransform3d` and be in an area
/// (either from the `CameraArea` resource or from the nearest portal).
pub fn cull_portals(
    camera_q: Query<(&WorldTransform3d,), With<lunar_3d::Camera3d>>,
    active_cam: Res<ActiveCamera3d>,
    portals: Query<(&Portal, Option<&PortalOpen>)>,
    areas: Query<&Area>,
    config: Res<PortalCulling>,
    frustum: Res<lunar_3d::Frustum>,
    mut visible: ResMut<VisibleAreas>,
    camera_area: Option<Res<CameraArea>>,
) {
    visible.area_ids.clear();
    visible.active = false;

    let cam_entity = match active_cam.entity { Some(e) => e, None => return };
    let cam_pos = match camera_q.get(cam_entity).ok() {
        Some((wt,)) => wt.translation,
        None => return,
    };

    // determine camera area: from CameraArea resource or nearest portal center
    let cam_area = if let Some(res) = camera_area {
        res.area_id
    } else {
        // fallback: camera is in the area of the nearest portal
        portals.iter()
            .filter(|(_, open)| open.is_some())
            .min_by(|(a, _), (b, _)| {
                let da = (a.center - Vec3::from(cam_pos)).length_squared();
                let db = (b.center - Vec3::from(cam_pos)).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(p, _)| p.area_a)
            .unwrap_or(0)
    };

    // if there are no portals or no area-tagged entities, skip culling
    let has_areas = areas.iter().next().is_some();
    if !has_areas { return; }

    visible.active = true;
    visible.area_ids.insert(cam_area);

    // BFS through portals
    let mut queue: VecDeque<(u32, u32)> = VecDeque::new(); // (area_id, depth)
    let mut visited: HashSet<u32> = HashSet::new();
    queue.push_back((cam_area, 0));
    visited.insert(cam_area);

    while let Some((area, depth)) = queue.pop_front() {
        if depth >= config.max_depth { continue; }
        for (portal, open) in portals.iter() {
            if open.is_none() { continue; } // closed portal
            let (this_area, other_area) = if portal.area_a == area {
                (portal.area_a, portal.area_b)
            } else if portal.area_b == area {
                (portal.area_b, portal.area_a)
            } else {
                continue;
            };
            let _ = this_area;
            if visited.contains(&other_area) { continue; }
            // check: portal AABB intersects frustum
            use lunar_math::Vec3A;
            let center = Vec3A::from(portal.center);
            let half = Vec3A::from(portal.half_extents);
            if frustum.intersects_aabb(center, half) {
                visited.insert(other_area);
                visible.area_ids.insert(other_area);
                queue.push_back((other_area, depth + 1));
            }
        }
    }
}

/// resource: override for the camera's area id.
///
/// insert this resource to tell the portal system which area the camera is in.
/// if absent, the system falls back to the nearest portal's area (a heuristic
/// that works for most levels but may be wrong near area boundaries).
#[derive(Resource, Clone, Copy)]
pub struct CameraArea {
    pub area_id: u32,
}

/// plugin that adds portal culling to the render pipeline.
pub struct PortalPlugin;

impl GamePlugin for PortalPlugin {
    fn name(&self) -> &str { "PortalPlugin" }
    fn build(&mut self, app: &mut App) {
        app.insert_resource(VisibleAreas::default())
           .insert_resource(PortalCulling::default());
        app.add_system_to_stage(UpdateStage::Render, cull_portals);
    }
}
