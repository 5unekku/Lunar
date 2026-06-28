//! headless render probe: render a minimal PBR-lit scene (ground cube + directional
//! sun + camera) through the engine, off-screen, and dump the raw rgba to /tmp so a
//! lit-vs-black render can be diagnosed without the editor or a display.
//!
//! inspect the dump with:
//!   magick -size {w}x{h} -depth 8 rgba:/tmp/probe.rgba -alpha off /tmp/probe.png

use lunar::lunar_3d::scene_format_3d::{
	CameraDef, DirectionalLightDef, EntityDefinition3d, MaterialDef, MeshRef, SceneDefinition3d,
	SceneLoader3d,
};
use lunar::lunar_3d::Plugin3d;
use lunar::lunar_assets::AssetPlugin;
use lunar::lunar_render_3d::{RenderConfig3d, RenderEngine3d, RenderPlugin3d};
use lunar::prelude::*;

const WIDTH: u32 = 848;
const HEIGHT: u32 = 445;

/// the same shape as the editor fixture: a green PBR ground, a sun, a camera.
fn probe_scene() -> SceneDefinition3d {
	SceneDefinition3d {
		name: "probe".to_string(),
		entities: vec![
			EntityDefinition3d {
				id: Some("camera".to_string()),
				position: (0.0, 1.7, 5.0),
				rotation: (-10.0, 0.0, 0.0),
				camera: Some(CameraDef { fov_y: 60.0, near: 0.1, far: 1000.0 }),
				..EntityDefinition3d::default()
			},
			EntityDefinition3d {
				id: Some("ground".to_string()),
				scale: (10.0, 0.1, 10.0),
				mesh: Some(MeshRef::Primitive("cube".to_string())),
				material: Some(MaterialDef {
					base_color: "#386010".to_string(),
					roughness: 0.9,
					..MaterialDef::default()
				}),
				..EntityDefinition3d::default()
			},
			EntityDefinition3d {
				id: Some("sun".to_string()),
				rotation: (-45.0, 30.0, 0.0),
				directional_light: Some(DirectionalLightDef {
					color: "#fffae8".to_string(),
					illuminance: 50_000.0,
				}),
				..EntityDefinition3d::default()
			},
		],
	}
}

fn spawn_probe_scene(mut commands: Commands, mut registry: ResMut<MeshRegistry>) {
	// disable temporal/post AA so the composite pass writes the headless target directly,
	// giving a clean read of the tonemap output (STAA's temporal accumulation otherwise
	// confounds single-frame shader experiments).
	commands.insert_resource(lunar::lunar_render_3d::DevRenderProfile::classic());
	let scene = probe_scene();
	SceneLoader3d::spawn_scene(&mut commands, &mut registry, &scene, None);
}

fn main() {
	let instance = wgpu::Instance::default();
	let config = RenderConfig3d { width: WIDTH, height: HEIGHT, vsync: false, ..Default::default() };
	let engine = RenderEngine3d::headless(&instance, &config);

	let mut app = App::new();
	app.insert_resource(WindowSettings::new(WIDTH, HEIGHT, false));
	app.insert_resource(engine);
	app.add_plugin(Plugin3d);
	app.add_plugin(RenderPlugin3d);
	app.add_plugin(AssetPlugin);
	app.add_startup_system(spawn_probe_scene);

	// tick a handful of frames so startup spawns, transforms propagate, and the scene renders.
	for _ in 0..8 {
		app.tick(1.0 / 60.0);
	}

	let image = app
		.engine()
		.world()
		.get_resource::<RenderEngine3d>()
		.and_then(RenderEngine3d::read_headless_rgba);

	match image {
		Some((bytes, width, height)) => {
			std::fs::write("/tmp/probe.rgba", &bytes).expect("write probe.rgba");
			// sample a few pixels straight from the buffer (bgra order from the headless target).
			let sample = |x: u32, y: u32| {
				let i = ((y * width + x) * 4) as usize;
				(bytes[i + 2], bytes[i + 1], bytes[i]) // r, g, b
			};
			println!("PROBE: rendered {width}x{height} -> /tmp/probe.rgba");
			println!("PROBE: ground-center rgb = {:?}", sample(width / 2, height * 3 / 4));
			println!("PROBE: ground-left   rgb = {:?}", sample(width / 6, height * 4 / 5));
			println!("PROBE: sky-top       rgb = {:?}", sample(width / 2, height / 10));
		}
		None => println!("PROBE: read_headless_rgba returned None (no headless target)"),
	}
}
