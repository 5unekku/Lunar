//! first-person platform demo.
//!
//! a 4×4 m grass platform with hard walls, a solid blue sky, and a sun disc.
//! demonstrates the 3d rendering pipeline end-to-end with no asset files.
//!
//! controls:
//!   WASD      — move
//!   mouse     — look
//!   Escape    — quit
//!   F11 / F   — toggle fullscreen

use lunar::prelude::*;

// ── colors ─────────────────────────────────────────────────────────────────

// grass: warm mid-green
const GRASS_COLOR: Color = Color::rgba(0.22, 0.52, 0.09, 1.0);
// sky: clear daytime blue (matches Sky::default)
const SKY_COLOR: Color = Color::rgba(0.40, 0.65, 1.00, 1.0);
// sun: warm off-white
const SUN_COLOR: Color = Color::rgba(1.00, 0.98, 0.85, 1.0);

// ── platform constants ──────────────────────────────────────────────────────

// half-size of the 4×4 m platform in world units
const HALF_PLATFORM: f32 = 2.0;
// player eye height in meters
const EYE_HEIGHT: f32 = 1.7;
// walk speed in m/s
const WALK_SPEED: f32 = 4.0;
// mouse sensitivity in radians per pixel
const SENSITIVITY: f32 = 0.002;

// ── systems ─────────────────────────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut registry: ResMut<MeshRegistry>,
    mut settings: ResMut<WindowSettings>,
) {
    // lock cursor for mouse-look
    settings.cursor_locked = true;

    // sky settings
    commands.insert_resource(Sky {
        sky_color: SKY_COLOR,
        sun_color: SUN_COLOR,
        show_sun: true,
        ..Sky::default()
    });

    // floor — 4×4 m horizontal quad
    let floor_mesh = registry.add_mesh(primitives::quad_mesh(HALF_PLATFORM, HALF_PLATFORM));
    let floor_mat = registry.add_material(MaterialData {
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

    // camera at eye height, facing -Z (into the platform)
    commands.spawn(Camera3dBundle {
        local: LocalTransform3d::from_xyz(0.0, EYE_HEIGHT, 0.0),
        ..Camera3dBundle::default()
    });
}

fn fps_controller(
    input: Res<InputState>,
    time: Res<Time>,
    mut camera: Query<&mut LocalTransform3d, With<Camera3d>>,
    mut yaw: Local<f32>,
    mut pitch: Local<f32>,
) {
    // initialize pitch to horizontal (zenith 90° = π/2)
    if *pitch == 0.0 {
        *pitch = std::f32::consts::FRAC_PI_2;
    }

    let (dx, dy) = input.mouse_delta();
    *yaw -= dx * SENSITIVITY;
    *pitch = (*pitch + dy * SENSITIVITY).clamp(0.001, std::f32::consts::PI - 0.001);

    let Ok(mut transform) = camera.single_mut() else { return; };

    // horizontal forward vector (no vertical component) for WASD movement
    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let right = Vec3::new(forward.z, 0.0, -forward.x);

    let mut pos = transform.translation;
    let speed = WALK_SPEED * time.delta_seconds();

    if input.is_key_held(KeyCode::W) { pos += forward * speed; }
    if input.is_key_held(KeyCode::S) { pos -= forward * speed; }
    if input.is_key_held(KeyCode::A) { pos -= right * speed; }
    if input.is_key_held(KeyCode::D) { pos += right * speed; }

    // keep inside the platform
    let limit = HALF_PLATFORM - 0.1;
    pos.x = pos.x.clamp(-limit, limit);
    pos.z = pos.z.clamp(-limit, limit);
    pos.y = EYE_HEIGHT;

    transform.translation = pos;
    // yaw around world Y, then pitch around local X
    // pitch = π/2 → level; 0 → straight up; π → straight down
    transform.rotation = Quat::from_rotation_y(*yaw)
        * Quat::from_rotation_x(*pitch - std::f32::consts::FRAC_PI_2);
}

fn quit_on_escape(input: Res<InputState>) {
    if input.is_key_just_pressed(KeyCode::Escape) {
        std::process::exit(0);
    }
}

// ── plugin ───────────────────────────────────────────────────────────────────

#[derive(Default)]
struct PlatformDemo;

impl GamePlugin for PlatformDemo {
    fn name(&self) -> &'static str {
        "PlatformDemo"
    }

    fn build(&mut self, app: &mut App) {
        app.add_startup_system(setup);
        app.add_system_to_stage(UpdateStage::Update, fps_controller);
        app.add_system_to_stage(UpdateStage::Update, quit_on_escape);
    }
}

// ── entry point ──────────────────────────────────────────────────────────────

fn main() {
    lunar::bootstrap_3d::<PlatformDemo>(RenderConfig3d {
        title: "Platform Demo".to_string(),
        width: 1280,
        height: 720,
        vsync: true,
        frame_cap: 0,
    });
}
