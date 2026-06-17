//! platform demo — scene setup in Rust, FPS movement in C#.
//!
//! build and run via:
//!   cargo xtask run
//!
//! controls (handled by the C# FpsController):
//!   WASD  — move
//!   mouse — look
//!   Escape — quit

use bevy_ecs::system::Command;
use lunar::prelude::*;
use lunar_ffi::set_main_camera_entity;
use lunar_plugin_loader::CsPlugin;
use std::path::PathBuf;

// ── scene constants ───────────────────────────────────────────────────────────

const GRASS_COLOR:   Color = Color::rgba(0.22, 0.52, 0.09, 1.0);
const SKY_COLOR:     Color = Color::rgba(0.40, 0.65, 1.00, 1.0);
const SUN_COLOR:     Color = Color::rgba(1.00, 0.98, 0.85, 1.0);
const HALF_PLATFORM: f32   = 2.0;
const EYE_HEIGHT:    f32   = 1.7;
const FOV_DEFAULT:   f32   = 90.0;
const NEAR:          f32   = 0.1;
const FAR:           f32   = 1000.0;

// ── systems ───────────────────────────────────────────────────────────────────

fn scene_setup(
    mut commands: Commands,
    mut registry: ResMut<MeshRegistry>,
    mut settings: ResMut<WindowSettings>,
) {
    settings.cursor_locked = true;

    commands.insert_resource(QualitySettings {
        staa: true,
        msaa_samples: 4,
        render_scale: 1.0,
        ..QualitySettings::minimum()
    });
    commands.insert_resource(Sky {
        sky_color: SKY_COLOR,
        sun_color: SUN_COLOR,
        show_sun: true,
        ..Sky::default()
    });

    let floor_mesh = registry.add_mesh(primitives::quad_mesh(HALF_PLATFORM, HALF_PLATFORM));
    let floor_mat  = registry.add_material(MaterialData {
        base_color: GRASS_COLOR,
        shading: ShadingModel::Unlit,
        ..MaterialData::default()
    });
    commands.spawn(Mesh3dBundle {
        local: LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
        mesh: Mesh3d(floor_mesh),
        material: Material3d(floor_mat),
        ..Mesh3dBundle::default()
    });

    let camera = commands.spawn(Camera3dBundle {
        local: LocalTransform3d::from_xyz(0.0, EYE_HEIGHT, 0.0),
        camera: Camera3d {
            projection: Projection::Perspective {
                fov_y: FOV_DEFAULT.to_radians(),
                near: NEAR,
                far: FAR,
            },
            ..Camera3d::default()
        },
        ..Camera3dBundle::default()
    });
    let camera_index = camera.id().index_u32();
    commands.queue(SetMainCameraCmd { index: camera_index });
}

/// deferred command: sets the FFI camera global after Commands flush.
struct SetMainCameraCmd { index: u32 }

impl Command for SetMainCameraCmd {
    fn apply(self, _world: &mut World) {
        set_main_camera_entity(self.index);
    }
}

// ── plugin ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct PlatformDemoCs;

impl GamePlugin for PlatformDemoCs {
    fn name(&self) -> &str { "platform-demo-cs" }

    fn build(&mut self, app: &mut App) {
        let path = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./lunar_scripts.so"));

        app.add_plugin(CsPlugin::new(path));
        app.add_startup_system(scene_setup);
    }
}

// ── entry point ───────────────────────────────────────────────────────────────

fn main() {
    lunar::bootstrap_3d::<PlatformDemoCs>(RenderConfig3d {
        title: "Platform Demo (C# scripting)".to_string(),
        width: 1280,
        height: 720,
        vsync: false,
        frame_cap: 0,
        tick_rate: TickRate::Hz60,
        ..Default::default()
    });
}
