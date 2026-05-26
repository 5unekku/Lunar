use lunar_core::{App, GamePlugin, UpdateStage};

use crate::camera::{ActiveCamera3d, AmbientLight, update_active_camera};
use crate::collision::{CollisionWorld3d, build_collision_world_3d};
use crate::systems::{TransformScratch3d, propagate_transforms_3d};
use crate::visibility::{
    Frustum, ViewportAspect, VisibilityScratch, propagate_visibility, update_frustum,
};

/// core 3D plugin.
///
/// registers all 3D systems and inserts default resources. the Update stage systems
/// run in this order (registration order is preserved):
///
/// 1. `advance_animations` — write joint local transforms from active clips
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

        app.add_system_to_stage(UpdateStage::Physics, build_collision_world_3d);

        app.add_system_to_stage(UpdateStage::Update, crate::animation::advance_animations);
        app.add_system_to_stage(UpdateStage::Update, propagate_transforms_3d);
        app.add_system_to_stage(UpdateStage::Update, update_active_camera);
        app.add_system_to_stage(UpdateStage::Update, update_frustum);
        app.add_system_to_stage(UpdateStage::Update, propagate_visibility);

        log::info!("Plugin3d: 3d systems registered");
    }
}
