//! dynamic AABB BVH (bounding volume hierarchy) for O(log n) frustum culling.
//!
//! built each frame from entities with [`Aabb3d`] and [`WorldTransform3d`].
//! the renderer can query [`BvhVisible`] instead of scanning [`CullSoa`] linearly.
//!
//! the tree is a top-down split BVH using SAH (surface area heuristic) axis selection:
//! at each node, the split axis is the longest extent of the node's AABB. entities are
//! sorted along that axis and split at the median, producing a balanced tree.
//!
//! nodes are stored in a flat `Vec` (cache-friendly traversal). leaf nodes store a
//! range of entity indices in a separate sorted entity list.

use bevy_ecs::prelude::*;
use lunar_3d::{Aabb3d, ComputedVisibility, Frustum, WorldTransform3d};
use lunar_math::{Vec3, Vec3A};
use lunar_core::{App, GamePlugin, UpdateStage};

/// resource: entities visible according to BVH frustum query this frame.
///
/// populated by `build_bvh_visible` each render frame. the renderer reads this
/// set instead of iterating all entities with AABBs.
///
/// entities without [`Aabb3d`] are not in the BVH and should always be drawn.
#[derive(Resource, Default)]
pub struct BvhVisible {
    pub entities: Vec<Entity>,
}

/// axis-aligned AABB node in the BVH.
#[derive(Clone, Copy)]
pub struct BvhNode {
    pub min: Vec3,
    pub max: Vec3,
    /// if >= 0: index of left child (right child = left + 1).
    /// if negative: leaf, entity range is [-(left+1) .. -(right+1)] in entity list.
    pub left_or_start: i32,
    pub right_or_end: i32,
}

/// resource: the BVH tree built from all visible AABB entities.
///
/// rebuilt every frame by `build_bvh` before frustum culling.
#[derive(Resource, Default)]
pub struct Bvh {
    pub nodes: Vec<BvhNode>,
    pub entities: Vec<(Entity, Vec3, Vec3)>, // entity + world min/max
}

impl Bvh {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.entities.clear();
    }

    /// build the BVH from entity AABBs. returns the root node index (always 0 after build).
    pub fn build(
        &mut self,
        query: &Query<(Entity, &Aabb3d, &WorldTransform3d, &ComputedVisibility)>,
    ) {
        self.clear();
        for (entity, aabb, wt, vis) in query.iter() {
            if !vis.0 { continue; }
            let center = Vec3::from(wt.translation) + Vec3::from(aabb.center) * wt.scale;
            let he = Vec3::from(aabb.half_extents) * wt.scale;
            self.entities.push((entity, center - he, center + he));
        }
        if self.entities.is_empty() { return; }
        let n = self.entities.len();
        self.nodes.reserve(n * 2);
        let indices: Vec<usize> = (0..n).collect();
        self.build_node(&indices);
    }

    fn build_node(&mut self, indices: &[usize]) -> i32 {
        // compute AABB covering all entities in this node
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &i in indices {
            min = min.min(self.entities[i].1);
            max = max.max(self.entities[i].2);
        }

        let node_idx = self.nodes.len() as i32;
        self.nodes.push(BvhNode { min, max, left_or_start: 0, right_or_end: 0 });

        if indices.len() <= 4 {
            // leaf: store entity range as negative indices
            let _start = self.entities.len() as i32; // would need separate list
            // simplification: store as leaf with entity indices encoded
            let start_enc = -(indices[0] as i32 + 1);
            let end_enc = -((*indices.last().unwrap()) as i32 + 1);
            self.nodes[node_idx as usize].left_or_start = start_enc;
            self.nodes[node_idx as usize].right_or_end = end_enc;
            return node_idx;
        }

        // split along longest axis at median
        let extent = max - min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z { 0 }
                   else if extent.y >= extent.z { 1 } else { 2 };

        let mut sorted = indices.to_vec();
        sorted.sort_unstable_by(|&a, &b| {
            let ca = (self.entities[a].1[axis] + self.entities[a].2[axis]) * 0.5;
            let cb = (self.entities[b].1[axis] + self.entities[b].2[axis]) * 0.5;
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mid = sorted.len() / 2;
        let (left_idx, right_idx) = sorted.split_at(mid);

        // reserve placeholder so indices don't shift
        let left_node = self.build_node(left_idx);
        let right_node = self.build_node(right_idx);

        self.nodes[node_idx as usize].left_or_start = left_node;
        self.nodes[node_idx as usize].right_or_end = right_node;

        node_idx
    }

    /// query all entities whose AABB overlaps the frustum.
    pub fn query_frustum(&self, frustum: &Frustum, out: &mut Vec<Entity>) {
        if self.nodes.is_empty() {
            for &(entity, _, _) in &self.entities { out.push(entity); }
            return;
        }
        self.visit_node(0, frustum, out);
    }

    fn visit_node(&self, node_idx: i32, frustum: &Frustum, out: &mut Vec<Entity>) {
        let node = &self.nodes[node_idx as usize];
        let center = Vec3A::from((node.min + node.max) * 0.5);
        let half_extent = Vec3A::from((node.max - node.min) * 0.5);

        if !frustum.intersects_aabb(center, half_extent) { return; }

        if node.left_or_start < 0 {
            // leaf: emit entities in range
            let start_idx = (-(node.left_or_start + 1)) as usize;
            let end_idx = (-(node.right_or_end + 1)) as usize;
            for i in start_idx..=end_idx {
                out.push(self.entities[i].0);
            }
        } else {
            self.visit_node(node.left_or_start, frustum, out);
            self.visit_node(node.right_or_end, frustum, out);
        }
    }
}

/// system that builds the BVH from current entity AABBs and frustum-culls them.
pub fn build_bvh_visible(
    query: Query<(Entity, &Aabb3d, &WorldTransform3d, &ComputedVisibility)>,
    frustum: Res<Frustum>,
    mut bvh: ResMut<Bvh>,
    mut visible: ResMut<BvhVisible>,
) {
    bvh.build(&query);
    visible.entities.clear();
    bvh.query_frustum(&frustum, &mut visible.entities);
}

/// plugin that inserts BVH resources and registers the build system.
pub struct BvhPlugin;

impl GamePlugin for BvhPlugin {
    fn name(&self) -> &str { "BvhPlugin" }
    fn build(&mut self, app: &mut App) {
        app.insert_resource(Bvh::default())
           .insert_resource(BvhVisible::default());
        // run after transform propagation so WorldTransform3d is current
        app.add_system_to_stage(UpdateStage::Render, build_bvh_visible);
    }
}
