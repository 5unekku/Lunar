//! platform demo host — scene setup in Rust, FPS movement in C#.
//!
//! build the C# plugin first:
//!   cd examples/platform_demo_cs/plugin
//!   dotnet publish -r linux-x64 -c Release
//!
//! then run:
//!   cargo run --example platform_demo_cs -- path/to/lunar_scripts.so
//!
//! if no path is given, looks for ./lunar_scripts.so in the working directory.
//!
//! controls (handled by the C# FpsController):
//!   WASD         — move
//!   mouse        — look
//!   Escape       — quit

use bevy_ecs::system::Command;
use lunar::prelude::*;
use lunar_ffi::{dispatch_systems, init_registry, set_main_camera_entity, LunarSchedule};
use lunar_plugin_loader::PluginLoader;
use std::path::PathBuf;

// ── scene constants ───────────────────────────────────────────────────────────

const GRASS_COLOR: Color = Color::rgba(0.22, 0.52, 0.09, 1.0);
const SKY_COLOR:   Color = Color::rgba(0.40, 0.65, 1.00, 1.0);
const SUN_COLOR:   Color = Color::rgba(1.00, 0.98, 0.85, 1.0);
const HALF_PLATFORM: f32 = 2.0;
const EYE_HEIGHT:    f32 = 1.7;
const FOV_DEFAULT:   f32 = 90.0;
const NEAR:          f32 = 0.1;
const FAR:           f32 = 1000.0;

// ── systems ───────────────────────────────────────────────────────────────────

fn scene_setup(
    mut commands:  Commands,
    mut registry:  ResMut<MeshRegistry>,
    mut settings:  ResMut<WindowSettings>,
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

    // floor quad
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

    // camera
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

    // entity id is valid immediately even though spawn is deferred via Commands
    let camera_index = camera.id().index_u32();
    // set it in the FFI global after commands flush (so C# Update can read it)
    commands.queue(SetMainCameraCmd { index: camera_index });
}

/// deferred command: writes the camera entity index into the FFI global
/// after Commands have flushed and the entity actually exists in the world.
struct SetMainCameraCmd { index: u32 }

impl Command for SetMainCameraCmd {
    fn apply(self, _world: &mut World) {
        set_main_camera_entity(self.index);
    }
}

fn dispatch_ffi_update(world: &mut World) {
    dispatch_systems(world, LunarSchedule::Update);
}

// ── plugin ────────────────────────────────────────────────────────────────────

struct PlatformDemoCs;

impl Default for PlatformDemoCs {
    fn default() -> Self { PlatformDemoCs }
}

impl GamePlugin for PlatformDemoCs {
    fn name(&self) -> &'static str { "PlatformDemoCs" }

    fn build(&mut self, app: &mut App) {
        let plugin_path = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./lunar_scripts.so"));

        {
            let world = app.world_mut();
            init_registry(world);

            let mut loader = PluginLoader::new();
            loader
                .load(world, &plugin_path)
                .unwrap_or_else(|_| panic!("failed to load plugin: {}", plugin_path.display()));

            // dispatch startup so C# can register resources/components if it needs to
            dispatch_systems(world, LunarSchedule::Startup);

            // keep the library loaded for the process lifetime
            let _ = Box::leak(Box::new(loader));
        }

        app.add_startup_system(scene_setup);
        app.add_system_to_stage(UpdateStage::Update, dispatch_ffi_update);
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
