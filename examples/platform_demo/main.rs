//! first-person platform demo — minimal scene, pure Rust.
//!
//! controls:
//!   WASD  — move       mouse — look
//!   Escape — quit      F11   — fullscreen

use lunar::prelude::*;

const GRASS:     Color = Color::rgba(0.22, 0.52, 0.09, 1.0);
const SKY_COLOR: Color = Color::rgba(0.40, 0.65, 1.00, 1.0);
const SUN_COLOR: Color = Color::rgba(1.00, 0.98, 0.85, 1.0);

const HALF:        f32 = 2.0;
const EYE:         f32 = 1.7;
const WALK_SPEED:  f32 = 4.0;
const SENSITIVITY: f32 = 0.002;
const FOV:         f32 = 90.0;

fn scene_setup(mut commands: Commands, mut registry: ResMut<MeshRegistry>, mut settings: ResMut<WindowSettings>) {
    settings.cursor_locked = true;
    commands.insert_resource(QualitySettings::minimum().with_msaa(4).with_staa(true));
    commands.insert_resource(Sky::solid(SKY_COLOR, SUN_COLOR));

    let floor_mesh = registry.add_mesh(primitives::quad_mesh(HALF, HALF));
    let floor_mat  = registry.add_material(MaterialData::color(GRASS, ShadingModel::Unlit));
    commands.spawn(Mesh3dBundle::at(Vec3::ZERO, floor_mesh, floor_mat));
    commands.spawn(Camera3dBundle::perspective(Vec3::new(0.0, EYE, 0.0), FOV.to_radians(), 0.1, 1000.0));
}

fn fps_controller(
    input: Res<InputState>,
    time: Res<Time>,
    mut camera: Query<&mut LocalTransform3d, With<Camera3d>>,
    mut yaw: Local<f32>,
    mut pitch: Local<f32>,
) {
    if *pitch == 0.0 { *pitch = std::f32::consts::FRAC_PI_2; }

    let dt = time.real_delta_seconds();
    let (dx, dy) = input.mouse_delta();
    *yaw   -= dx * SENSITIVITY;
    *pitch  = (*pitch + dy * SENSITIVITY).clamp(0.001, std::f32::consts::PI - 0.001);

    let Ok(mut transform) = camera.single_mut() else { return };

    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right   = Vec3::new(-forward.z, 0.0, forward.x);

    let mut mx = 0.0_f32;
    let mut mz = 0.0_f32;
    if input.is_key_held(KeyCode::D) { mx += 1.0; }
    if input.is_key_held(KeyCode::A) { mx -= 1.0; }
    if input.is_key_held(KeyCode::S) { mz += 1.0; }
    if input.is_key_held(KeyCode::W) { mz -= 1.0; }
    let len = (mx * mx + mz * mz).sqrt();
    if len > 1.0 { mx /= len; mz /= len; }

    let speed = WALK_SPEED * dt;
    let mut pos = transform.translation;
    pos += forward * (-mz * speed) + right * (mx * speed);
    pos.x = pos.x.clamp(-(HALF - 0.1), HALF - 0.1);
    pos.z = pos.z.clamp(-(HALF - 0.1), HALF - 0.1);
    pos.y = EYE;
    transform.translation = pos;
    transform.rotation =
        Quat::from_rotation_y(*yaw) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - *pitch);
}

fn quit_on_escape(input: Res<InputState>) {
    if input.is_key_just_pressed(KeyCode::Escape) {
        #[cfg(not(target_arch = "wasm32"))]
        std::process::exit(0);
    }
}

#[derive(Default)]
struct PlatformDemo;

impl GamePlugin for PlatformDemo {
    fn name(&self) -> &str { "platform-demo" }
    fn build(&mut self, app: &mut App) {
        app.add_startup_system(scene_setup);
        app.add_system_to_stage(UpdateStage::Render, fps_controller);
        app.add_system_to_stage(UpdateStage::Update, quit_on_escape);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    lunar::bootstrap_3d::<PlatformDemo>(RenderConfig3d {
        title: "Platform Demo".to_string(),
        width: 1280,
        height: 720,
        tick_rate: TickRate::Hz60,
        ..Default::default()
    });
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub async fn start() {
    lunar::bootstrap_wasm_3d::<PlatformDemo>(RenderConfig3d {
        width: 1280,
        height: 720,
        tick_rate: TickRate::Hz60,
        ..Default::default()
    }).await;
}
