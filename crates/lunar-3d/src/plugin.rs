use lunar_core::{App, GamePlugin, UpdateStage};

use crate::camera::{ActiveCamera3d, AmbientLight, update_active_camera};
use crate::collision::{CollisionWorld3d, build_collision_world_3d};
use crate::systems::propagate_transforms_3d;
use crate::visibility::{Frustum, propagate_visibility, update_frustum};

/// core 3D plugin.
///
/// registers:
/// - transform propagation (local → world through the hierarchy)
/// - visibility propagation (Visibility → ComputedVisibility)
/// - active camera selection and frustum update
/// - skeletal animation advancement
/// - 3D collision world rebuild
/// - default resources: [`ActiveCamera3d`], [`AmbientLight`], [`CollisionWorld3d`], [`Frustum`]
///
/// system order within the Update stage (in registration order):
/// 1. `advance_animations` — write joint local transforms from the active clip
/// 2. `propagate_transforms_3d` — propagate local → world transforms
/// 3. `update_active_camera` — select the highest-priority Camera3d
/// 4. `update_frustum` — recompute frustum from the active camera
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

        app.add_system_to_stage(UpdateStage::Physics, build_collision_world_3d);

        app.add_system_to_stage(UpdateStage::Update, crate::animation::advance_animations);
        app.add_system_to_stage(UpdateStage::Update, propagate_transforms_3d);
        app.add_system_to_stage(UpdateStage::Update, update_active_camera);
        app.add_system_to_stage(UpdateStage::Update, update_frustum);
        app.add_system_to_stage(UpdateStage::Update, propagate_visibility);

        log::info!("Plugin3d: 3d systems registered");
    }
}
