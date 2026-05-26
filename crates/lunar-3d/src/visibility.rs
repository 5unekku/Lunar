use std::collections::HashMap;

use bevy_ecs::prelude::*;
use lunar_core::Parent;
use lunar_math::{Mat4, Vec3, Vec4};

use crate::camera::{ActiveCamera3d, Camera3d};
use crate::transform::WorldTransform3d;

/// user-facing visibility state for a renderable entity.
///
/// hierarchy propagation computes [`ComputedVisibility`] from this and the parent chain.
/// if an entity has no parent, `Inherited` and `Visible` are equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Visibility {
    /// inherit from parent — hidden if any ancestor is `Hidden`.
    Inherited,
    /// always hidden regardless of parent or children.
    Hidden,
    /// always visible regardless of parent.
    Visible,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Inherited
    }
}

/// computed visibility — propagated from [`Visibility`] through the entity hierarchy.
///
/// set each frame by [`propagate_visibility`]. the render system reads this to skip
/// invisible entities without walking the hierarchy itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct ComputedVisibility(pub bool);

impl Default for ComputedVisibility {
    fn default() -> Self {
        Self(true)
    }
}

/// render layer membership bitmask.
///
/// cameras and entities both carry `RenderLayers`; an entity is rendered by a camera
/// only when their bitmasks share at least one bit. default is layer 0.
///
/// up to 64 independent layers. layer 0 is the standard scene layer.
/// use higher layers for things like debug overlays, skyboxes, or UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct RenderLayers(pub u64);

impl RenderLayers {
    /// default: layer 0 only.
    pub const DEFAULT: Self = Self(1);
    /// no layers — entity is invisible to all cameras.
    pub const NONE: Self = Self(0);

    /// single-layer constructor.
    #[must_use]
    pub const fn layer(n: u32) -> Self {
        Self(1 << n)
    }

    /// add a layer (fluent).
    #[must_use]
    pub const fn with(self, n: u32) -> Self {
        Self(self.0 | (1 << n))
    }

    /// remove a layer (fluent).
    #[must_use]
    pub const fn without(self, n: u32) -> Self {
        Self(self.0 & !(1 << n))
    }

    /// true if the two masks share at least one layer.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl Default for RenderLayers {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// axis-aligned bounding box in local model space.
///
/// add alongside [`Mesh3d`](crate::mesh::Mesh3d) so the render system can perform
/// CPU-side frustum culling before issuing draw calls. the render system transforms
/// the center to world space using the entity's [`WorldTransform3d`] before testing.
///
/// compute from [`MeshData::compute_aabb`] after loading a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct Aabb3d {
    /// center in local model space.
    pub center: Vec3,
    /// half-extents along each axis. always positive.
    pub half_extents: Vec3,
}

impl Aabb3d {
    /// compute a tight AABB from a slice of positions.
    #[must_use]
    pub fn from_positions(positions: &[Vec3]) -> Self {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for &pos in positions {
            min = min.min(pos);
            max = max.max(pos);
        }
        let center = (min + max) * 0.5;
        let half_extents = (max - min) * 0.5;
        Self { center, half_extents }
    }
}

/// camera frustum — 6 half-space planes bounding the view volume.
///
/// computed from the view-projection matrix each frame by [`update_frustum`].
/// stored as a resource so the render backend can use it for culling without
/// recomputing per-system.
///
/// planes are in world space and face inward — a point is inside if it satisfies
/// all 6 planes (dot(plane.xyz, point) + plane.w >= 0).
#[derive(Debug, Clone, Copy, Resource)]
pub struct Frustum {
    /// [left, right, bottom, top, near, far]. each Vec4 is (nx, ny, nz, d).
    pub planes: [Vec4; 6],
}

impl Frustum {
    /// extract frustum planes from a combined view-projection matrix.
    ///
    /// uses the Gribb/Hartmann method (column-major, right-handed clip space).
    /// planes are not normalized — use for overlap tests, not distance queries.
    #[must_use]
    pub fn from_view_proj(vp: Mat4) -> Self {
        let cols = vp.to_cols_array_2d();
        // cols[i][j] = column i, row j
        let row = |i: usize| Vec4::new(cols[0][i], cols[1][i], cols[2][i], cols[3][i]);
        let r0 = row(0);
        let r1 = row(1);
        let r2 = row(2);
        let r3 = row(3);
        Self {
            planes: [
                r3 + r0, // left
                r3 - r0, // right
                r3 + r1, // bottom
                r3 - r1, // top
                r3 + r2, // near
                r3 - r2, // far
            ],
        }
    }

    /// conservative AABB visibility test.
    ///
    /// returns false only when the AABB is provably outside the frustum.
    /// may return true for AABBs that are near-edge or slightly outside (false positives
    /// are safe — they just result in unnecessary draw calls).
    ///
    /// `world_center` is the AABB center transformed to world space. scale is ignored
    /// (the half_extents already account for object scale if set from world-space bounds).
    #[must_use]
    pub fn intersects_aabb(self, center: Vec3, half_extents: Vec3) -> bool {
        for plane in &self.planes {
            let normal = Vec3::new(plane.x, plane.y, plane.z);
            let signed_radius =
                half_extents.x * normal.x.abs()
                + half_extents.y * normal.y.abs()
                + half_extents.z * normal.z.abs();
            let dist = normal.dot(center) + plane.w;
            if dist + signed_radius < 0.0 {
                return false;
            }
        }
        true
    }
}

impl Default for Frustum {
    /// identity frustum — passes everything. replaced on the first frame by `update_frustum`.
    fn default() -> Self {
        let inf = Vec4::new(0.0, 0.0, 0.0, f32::MAX);
        Self {
            planes: [inf; 6],
        }
    }
}

/// marker component — this entity casts shadows when rendered.
///
/// the render backend skips shadow map draw calls for entities without this component.
/// add to mesh entities that should cast shadows; omit for small props, particles,
/// or anything that won't noticeably contribute to shadow quality.
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct ShadowCaster;

/// marker component — this surface receives projected shadows.
///
/// only entities with this component are included in the shadow-receive pass.
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct ShadowReceiver;

/// propagate [`Visibility`] through the entity hierarchy to produce [`ComputedVisibility`].
///
/// an entity is computed-visible if:
/// - it has `Visibility::Visible`, or
/// - it has `Visibility::Inherited` and its parent is computed-visible.
/// `Visibility::Hidden` always produces `false` regardless of parent.
pub fn propagate_visibility(world: &mut World) {
    let snapshot: Vec<(Entity, Visibility, Option<Entity>)> = world
        .query::<(Entity, &Visibility, Option<&Parent>)>()
        .iter(world)
        .map(|(entity, vis, parent)| (entity, *vis, parent.map(|p| p.0)))
        .collect();

    if snapshot.is_empty() {
        return;
    }

    let parent_of: HashMap<Entity, Entity> = snapshot
        .iter()
        .filter_map(|(entity, _, parent)| parent.map(|p| (*entity, p)))
        .collect();

    let mut depths: HashMap<Entity, u32> = HashMap::new();
    for &(entity, _, _) in &snapshot {
        depth_of(entity, &parent_of, &mut depths);
    }

    let mut sorted = snapshot;
    sorted.sort_by_key(|(entity, _, _)| depths.get(entity).copied().unwrap_or(0));

    let mut computed: HashMap<Entity, bool> = HashMap::with_capacity(sorted.len());

    for (entity, visibility, parent_entity) in sorted {
        let parent_visible = parent_entity
            .and_then(|parent| computed.get(&parent).copied())
            .unwrap_or(true);
        let visible = match visibility {
            Visibility::Visible => true,
            Visibility::Hidden => false,
            Visibility::Inherited => parent_visible,
        };
        computed.insert(entity, visible);

        let cv = ComputedVisibility(visible);
        if let Some(mut existing) = world.get_mut::<ComputedVisibility>(entity) {
            *existing = cv;
        } else if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(cv);
        }
    }
}

fn depth_of(
    entity: Entity,
    parent_of: &HashMap<Entity, Entity>,
    cache: &mut HashMap<Entity, u32>,
) -> u32 {
    if let Some(&depth) = cache.get(&entity) {
        return depth;
    }
    let depth = parent_of
        .get(&entity)
        .map(|&parent| depth_of(parent, parent_of, cache) + 1)
        .unwrap_or(0);
    cache.insert(entity, depth);
    depth
}

/// recompute the [`Frustum`] resource from the active camera each frame.
///
/// runs after `update_active_camera` so the active entity is current.
/// the frustum uses world-space planes — render systems can use it directly
/// without additional transformation.
///
/// uses 16:9 as the aspect ratio when the viewport size is unknown. the render
/// backend should call `Frustum::from_view_proj` again with the true aspect ratio
/// before its first culling pass.
pub fn update_frustum(
    active: Res<ActiveCamera3d>,
    cameras: Query<(&Camera3d, &WorldTransform3d)>,
    mut frustum: ResMut<Frustum>,
) {
    let Some(camera_entity) = active.entity else {
        return;
    };
    let Ok((camera, transform)) = cameras.get(camera_entity) else {
        return;
    };
    let aspect = 16.0 / 9.0;
    let vp = camera.view_proj(*transform, aspect);
    *frustum = Frustum::from_view_proj(vp);
}
