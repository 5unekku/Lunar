use lunar_core::{App, GamePlugin, UpdateStage};

use crate::camera::{ActiveCamera3d, AmbientLight, update_active_camera};
use crate::collision::{CollisionWorld3d, build_collision_world_3d};
use crate::systems::{TransformScratch3d, propagate_transforms_3d};
use crate::visibility::{
    CullSoa, Frustum, ViewportAspect, VisibilityScratch, build_cull_soa, propagate_visibility,
    update_frustum,
};

/// core 3D plugin.
///
/// registers all 3D systems and inserts default resources.
///
/// Update stage (registration order preserved):
/// 1. `advance_animations` — write joint local transforms from active clips
///
/// Render stage (run before `render_3d_system` so the renderer sees current-frame transforms):
/// 2. `propagate_transforms_3d` — propagate local → world transforms
/// 3. `update_active_camera` — select highest-priority active Camera3d
/// 4. `update_frustum` — recompute frustum from active camera + ViewportAspect
/// 5. `propagate_visibility` — propagate Visibility → ComputedVisibility
pub struct Plugin3d;

impl GamePlugin for Plugin3d {
    fn name(&self) -> &'static str {
        "Plugin3d"
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(ActiveCamera3d::default());
        app.insert_resource(AmbientLight::default());
        app.insert_resource(CollisionWorld3d::default());
        app.insert_resource(Frustum::default());
        app.insert_resource(ViewportAspect::default());
        app.insert_resource(TransformScratch3d::default());
        app.insert_resource(VisibilityScratch::default());
        app.insert_resource(CullSoa::default());
        app.insert_resource(crate::mesh_registry::MeshRegistry::default());

        app.add_system_to_stage(UpdateStage::Physics, build_collision_world_3d);

        app.add_system_to_stage(UpdateStage::Update, crate::animation::advance_animations);

        // these run in Render (before render_3d_system) so game logic in Update
        // can write LocalTransform3d this frame and the renderer sees it immediately.
        app.add_system_to_stage(UpdateStage::Render, propagate_transforms_3d);
        app.add_system_to_stage(UpdateStage::Render, update_active_camera);
        app.add_system_to_stage(UpdateStage::Render, update_frustum);
        app.add_system_to_stage(UpdateStage::Render, propagate_visibility);
        app.add_system_to_stage(UpdateStage::Render, build_cull_soa);

        log::info!("Plugin3d: 3d systems registered");
    }
}
