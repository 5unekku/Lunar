//! combined BSP plugin: registers both BVH and portal culling systems.

use lunar_core::{App, GamePlugin};

use crate::{BvhPlugin, PortalPlugin};

/// combined plugin: registers BVH spatial acceleration and portal area culling.
///
/// adds both [`BvhPlugin`] and [`PortalPlugin`]. game code can add them individually
/// if only one is needed.
pub struct BspPlugin;

impl GamePlugin for BspPlugin {
    fn name(&self) -> &str { "BspPlugin" }
    fn build(&mut self, app: &mut App) {
        BvhPlugin.build(app);
        PortalPlugin.build(app);
    }
}
