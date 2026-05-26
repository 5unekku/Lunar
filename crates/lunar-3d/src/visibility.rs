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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub struct RenderLayers(pub u64);

impl RenderLayers {
    pub const DEFAULT: Self = Self(1);
    pub const NONE: Self = Self(0);

    #[must_use]
    pub const fn layer(n: u32) -> Self {
        Self(1 << n)
    }

    #[must_use]
    pub const fn with(self, n: u32) -> Self {
        Self(self.0 | (1 << n))
    }

    #[must_use]
    pub const fn without(self, n: u32) -> Self {
        Self(self.0 & !(1 << n))
    }

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
/// CPU-side frustum culling before issuing draw calls. compute from
/// [`Aabb3d::from_positions`] after loading a mesh.
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
/// stored as a resource so the render backend can use it without recomputing.
///
/// planes are in world space, facing inward — a point is inside if all 6 plane
/// tests pass: `dot(plane.xyz, point) + plane.w >= 0`.
#[derive(Debug, Clone, Copy, Resource)]
pub struct Frustum {
    /// [left, right, bottom, top, near, far]. each Vec4 is (nx, ny, nz, d).
    pub planes: [Vec4; 6],
}

impl Frustum {
    /// extract frustum planes from a combined view-projection matrix.
    ///
    /// uses the Gribb/Hartmann method (column-major, right-handed clip space).
    /// planes are not normalized — use for overlap tests only.
    #[must_use]
    pub fn from_view_proj(vp: Mat4) -> Self {
        let cols = vp.to_cols_array_2d();
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
    /// returns false only when the AABB is provably outside the frustum. false positives
    /// are safe — they result in a redundant draw call, not a visual artifact.
    #[must_use]
    pub fn intersects_aabb(self, center: Vec3, half_extents: Vec3) -> bool {
        for plane in &self.planes {
            let normal = Vec3::new(plane.x, plane.y, plane.z);
            let signed_radius = half_extents.x * normal.x.abs()
                + half_extents.y * normal.y.abs()
                + half_extents.z * normal.z.abs();
            if normal.dot(center) + plane.w + signed_radius < 0.0 {
                return false;
            }
        }
        true
    }
}

impl Default for Frustum {
    /// pass-everything default — replaced on the first frame by `update_frustum`.
    fn default() -> Self {
        Self {
            planes: [Vec4::new(0.0, 0.0, 0.0, f32::MAX); 6],
        }
    }
}

/// aspect ratio (width / height) of the primary render viewport.
///
/// set this resource from the windowing/render system whenever the window is created
/// or resized. `update_frustum` reads it when computing the projection matrix.
///
/// defaults to 16:9 so scenes render correctly before the first window event.
#[derive(Debug, Clone, Copy, Resource)]
pub struct ViewportAspect(pub f32);

impl ViewportAspect {
    #[must_use]
    pub fn from_size(width: f32, height: f32) -> Self {
        Self(width / height.max(f32::EPSILON))
    }
}

impl Default for ViewportAspect {
    fn default() -> Self {
        Self(16.0 / 9.0)
    }
}

/// marker component — this entity casts shadows.
///
/// the render backend skips shadow map draw calls for entities without this.
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct ShadowCaster;

/// marker component — this surface receives projected shadows.
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct ShadowReceiver;

/// scratch storage for visibility propagation — allocated once, reused every frame.
#[derive(Resource, Default)]
pub struct VisibilityScratch {
    snapshot: Vec<(Entity, Visibility, Option<Entity>)>,
    parent_of: HashMap<Entity, Entity>,
    depths: HashMap<Entity, u32>,
    computed: HashMap<Entity, bool>,
}

/// propagate [`Visibility`] through the entity hierarchy to produce [`ComputedVisibility`].
///
/// an entity is computed-visible when:
/// - it has `Visibility::Visible`, or
/// - it has `Visibility::Inherited` and its parent is computed-visible.
///
/// uses a persistent scratch resource to avoid per-frame heap allocations.
pub fn propagate_visibility(world: &mut World) {
    let mut scratch = world
        .remove_resource::<VisibilityScratch>()
        .unwrap_or_default();

    scratch.snapshot.clear();
    scratch.parent_of.clear();
    scratch.depths.clear();
    scratch.computed.clear();

    world
        .query::<(Entity, &Visibility, Option<&Parent>)>()
        .iter(world)
        .for_each(|(entity, vis, parent)| {
            scratch.snapshot.push((entity, *vis, parent.map(|p| p.0)));
        });

    if scratch.snapshot.is_empty() {
        world.insert_resource(scratch);
        return;
    }

    for &(entity, _, parent) in &scratch.snapshot {
        if let Some(parent_entity) = parent {
            scratch.parent_of.insert(entity, parent_entity);
        }
    }

    for i in 0..scratch.snapshot.len() {
        let entity = scratch.snapshot[i].0;
        depth_of(entity, &scratch.parent_of, &mut scratch.depths);
    }

    scratch
        .snapshot
        .sort_by_key(|(entity, _, _)| scratch.depths.get(entity).copied().unwrap_or(0));

    for (entity, visibility, parent_entity) in scratch.snapshot.iter().copied() {
        let parent_visible = parent_entity
            .and_then(|parent| scratch.computed.get(&parent).copied())
            .unwrap_or(true);
        let visible = match visibility {
            Visibility::Visible => true,
            Visibility::Hidden => false,
            Visibility::Inherited => parent_visible,
        };
        scratch.computed.insert(entity, visible);

        let cv = ComputedVisibility(visible);
        if let Some(mut existing) = world.get_mut::<ComputedVisibility>(entity) {
            *existing = cv;
        } else if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(cv);
        }
    }

    world.insert_resource(scratch);
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
/// reads [`ViewportAspect`] for the projection matrix — set this resource from the
/// render/windowing layer whenever the window is resized.
pub fn update_frustum(
    active: Res<ActiveCamera3d>,
    cameras: Query<(&Camera3d, &WorldTransform3d)>,
    aspect: Res<ViewportAspect>,
    mut frustum: ResMut<Frustum>,
) {
    let Some(camera_entity) = active.entity else {
        return;
    };
    let Ok((camera, transform)) = cameras.get(camera_entity) else {
        return;
    };
    *frustum = Frustum::from_view_proj(camera.view_proj(*transform, aspect.0));
}
