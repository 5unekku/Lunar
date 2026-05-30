//! first-person platform demo.
//!
//! a 4×4 m grass platform with hard walls, a solid blue sky, and a sun disc.
//! demonstrates the 3d rendering pipeline end-to-end with no asset files.
//!
//! usage: platform_demo [fov]   fov in degrees, 70-150 (default 90)
//!
//! controls:
//!   WASD        — move
//!   mouse       — look
//!   minus / equals — decrease / increase FOV by 5°
//!   Escape      — quit
//!   F11 / F     — toggle fullscreen

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
// right stick look sensitivity in radians per second
const STICK_LOOK_SPEED: f32 = 2.5;
// analog stick deadzone (applied before movement/look)
const DEADZONE: f32 = 0.15;
// fov range in degrees
const FOV_MIN: f32 = 70.0;
const FOV_MAX: f32 = 150.0;
const FOV_DEFAULT: f32 = 90.0;
const FOV_STEP: f32 = 5.0;
// camera near/far planes
const NEAR: f32 = 0.1;
const FAR: f32 = 1000.0;

// ── resources ────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct FovDeg(f32);

// ── helpers ──────────────────────────────────────────────────────────────────

/// rescales an axis value to [0, 1] outside the deadzone, returning 0 inside it.
fn apply_deadzone(value: f32) -> f32 {
    let sign = value.signum();
    let magnitude = value.abs();
    if magnitude < DEADZONE {
        return 0.0;
    }
    sign * (magnitude - DEADZONE) / (1.0 - DEADZONE)
}

// ── systems ─────────────────────────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut registry: ResMut<MeshRegistry>,
    mut settings: ResMut<WindowSettings>,
) {
    // simple demo: no post-processing effects
    commands.insert_resource(QualitySettings::minimum());

    // lock cursor for mouse-look
    settings.cursor_locked = true;

    // fov from first CLI arg, clamped to [FOV_MIN, FOV_MAX]
    let fov_deg = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(FOV_DEFAULT)
        .clamp(FOV_MIN, FOV_MAX);
    commands.insert_resource(FovDeg(fov_deg));

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

    // camera at eye height with the configured FOV
    commands.spawn(Camera3dBundle {
        local: LocalTransform3d::from_xyz(0.0, EYE_HEIGHT, 0.0),
        camera: Camera3d {
            projection: Projection::Perspective {
                fov_y: fov_deg.to_radians(),
                near: NEAR,
                far: FAR,
            },
            ..Camera3d::default()
        },
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

    let dt = time.delta_seconds();

    // ── look input: mouse + right stick ───────────────────────────────────
    let (dx, dy) = input.mouse_delta();
    let stick_rx = apply_deadzone(input.gamepad(0).map_or(0.0, |gp| gp.axis(GamepadAxis::RightStickX)));
    let stick_ry = apply_deadzone(input.gamepad(0).map_or(0.0, |gp| gp.axis(GamepadAxis::RightStickY)));

    *yaw -= dx * SENSITIVITY + stick_rx * STICK_LOOK_SPEED * dt;
    *pitch = (*pitch + dy * SENSITIVITY + stick_ry * STICK_LOOK_SPEED * dt)
        .clamp(0.001, std::f32::consts::PI - 0.001);

    let Ok(mut transform) = camera.single_mut() else { return; };

    // ── move input: WASD + left stick ─────────────────────────────────────
    // camera forward is -Z in local space, rotated by yaw around Y.
    // from_rotation_y(yaw) * (0,0,-1) = (-sin(yaw), 0, -cos(yaw))
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right   = Vec3::new(-forward.z, 0.0, forward.x);

    let stick_mx = apply_deadzone(input.gamepad(0).map_or(0.0, |gp| gp.axis(GamepadAxis::LeftStickX)));
    let stick_my = apply_deadzone(input.gamepad(0).map_or(0.0, |gp| gp.axis(GamepadAxis::LeftStickY)));

    let mut move_x = stick_mx;
    let mut move_z = stick_my;
    if input.is_key_held(KeyCode::D) { move_x += 1.0; }
    if input.is_key_held(KeyCode::A) { move_x -= 1.0; }
    if input.is_key_held(KeyCode::S) { move_z += 1.0; }
    if input.is_key_held(KeyCode::W) { move_z -= 1.0; }
    // clamp combined input to unit length so diagonal + stick don't stack
    let input_len = (move_x * move_x + move_z * move_z).sqrt();
    let (move_x, move_z) = if input_len > 1.0 {
        (move_x / input_len, move_z / input_len)
    } else {
        (move_x, move_z)
    };

    let mut pos = transform.translation;
    let speed = WALK_SPEED * dt;
    pos += forward * (-move_z * speed) + right * (move_x * speed);

    // keep inside the platform
    let limit = HALF_PLATFORM - 0.1;
    pos.x = pos.x.clamp(-limit, limit);
    pos.z = pos.z.clamp(-limit, limit);
    pos.y = EYE_HEIGHT;

    transform.translation = pos;
    // yaw around world Y, then pitch around local X.
    // zenith pitch: 0 = up, π/2 = level, π = down.
    // rotation_x(π/2 - pitch): at pitch=π/2 → 0 (level); at pitch=0 → +π/2 (up); at pitch=π → -π/2 (down).
    transform.rotation = Quat::from_rotation_y(*yaw)
        * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - *pitch);
}

fn fov_controller(
    input: Res<InputState>,
    mut fov: ResMut<FovDeg>,
    mut camera: Query<&mut Camera3d>,
) {
    let mut delta = 0.0f32;
    if input.is_key_just_pressed(KeyCode::Equals) { delta += FOV_STEP; }
    if input.is_key_just_pressed(KeyCode::Minus)  { delta -= FOV_STEP; }
    if delta == 0.0 { return; }

    fov.0 = (fov.0 + delta).clamp(FOV_MIN, FOV_MAX);
    let Ok(mut cam) = camera.single_mut() else { return; };
    if let Projection::Perspective { near, far, .. } = cam.projection {
        cam.projection = Projection::Perspective { fov_y: fov.0.to_radians(), near, far };
    }
}

fn quit_on_escape(input: Res<InputState>) {
    let keyboard_quit = input.is_key_just_pressed(KeyCode::Escape);
    let controller_quit = input.gamepad(0)
        .is_some_and(|gp| gp.is_button_just_pressed(GamepadButton::Start)
            || gp.is_button_just_pressed(GamepadButton::Back));
    if keyboard_quit || controller_quit {
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
        app.add_system_to_stage(UpdateStage::Update, fov_controller);
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
        tick_rate: TickRate::Hz60,
    });
}
