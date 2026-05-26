use bevy_ecs::prelude::*;
use lunar_core::{App, GamePlugin};

use crate::camera::{ActiveCamera3d, AmbientLight, update_active_camera};
use crate::systems::propagate_transforms_3d;

/// core 3D plugin.
///
/// registers:
/// - transform propagation (local → world through the hierarchy)
/// - active camera selection
/// - default resources: [`ActiveCamera3d`], [`AmbientLight`]
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
        app.add_system(propagate_transforms_3d);
        app.add_system(update_active_camera);
        log::info!("Plugin3d: 3D systems registered");
    }
}
