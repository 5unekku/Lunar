//! dynamic-swarm: 3000 orbiting shadow-casting cubes lit by 200 point lights.
//! every cube moves every tick, so this stresses the dynamic path — transform
//! propagation, per-frame cull SoA rebuild, dynamic uniform re-upload, and
//! clustered point-light assignment — the opposite of static-city.

use lunar::lunar_3d::Aabb3d;
use lunar::lunar_core::UpdateStage;
use lunar::lunar_math::Vec3A;
use lunar::prelude::*;

use crate::common::{checker_texture, Rng};

const CUBES: usize = 3000;
const LIGHTS: usize = 200;
const FIELD: f32 = 120.0;

/// orbit parameters for one swarm cube. the mover derives a fresh world position
/// from elapsed time every tick — deterministic, so goldens stay stable.
#[derive(Component, Clone, Copy)]
struct Orbit {
	center: Vec3,
	radius: f32,
	angular: f32,
	phase: f32,
	vertical: f32,
}

pub fn register(app: &mut App) {
	app.add_startup_system(spawn);
	app.add_system_to_stage(UpdateStage::Update, orbit_system);
}

fn spawn(mut commands: Commands, mut registry: ResMut<MeshRegistry>, mut assets: ResMut<AssetServer>) {
	let cube = registry.add_mesh(primitives::unit_cube());
	let tex = checker_texture(&mut assets, 32, 4, [220, 220, 230, 255], [40, 40, 50, 255]);
	let material = registry.add_material(MaterialData {
		shading: ShadingModel::Pbr,
		base_color: Color::rgb(0.8, 0.8, 0.85),
		diffuse: Some(tex),
		roughness: 0.4,
		metallic: 0.1,
		..MaterialData::default()
	});

	// ground plane to catch shadows and light pooling.
	let ground_tex = checker_texture(&mut assets, 128, 16, [50, 52, 58, 255], [38, 40, 46, 255]);
	let ground_mat = registry.add_material(MaterialData {
		shading: ShadingModel::Pbr,
		base_color: Color::rgb(0.3, 0.3, 0.34),
		diffuse: Some(ground_tex),
		roughness: 0.9,
		..MaterialData::default()
	});
	let ground_mesh = registry.add_mesh(primitives::quad_mesh(FIELD * 1.5, FIELD * 1.5));
	commands.spawn((Mesh3dBundle::at(Vec3::ZERO, ground_mesh, ground_mat), StaticMesh));

	let mut rng = Rng::new(0x5A7A_1234);
	for _ in 0..CUBES {
		let center = Vec3::new(
			rng.range(-FIELD, FIELD),
			rng.range(4.0, 40.0),
			rng.range(-FIELD, FIELD),
		);
		let orbit = Orbit {
			center,
			radius: rng.range(2.0, 10.0),
			angular: rng.range(0.3, 1.6),
			phase: rng.range(0.0, std::f32::consts::TAU),
			vertical: rng.range(1.0, 6.0),
		};
		let scale = rng.range(0.6, 1.6);
		commands.spawn((
			ShadowMesh3dBundle {
				base: Mesh3dBundle {
					local: LocalTransform3d {
						translation: center,
						rotation: Quat::IDENTITY,
						scale: Vec3::splat(scale),
					},
					mesh: Mesh3d(cube),
					material: Material3d(material),
					..Mesh3dBundle::default()
				},
				aabb: Aabb3d { center: Vec3A::ZERO, half_extents: Vec3A::splat(0.5) },
				shadow_caster: ShadowCaster,
			},
			orbit,
		));
	}

	// a field of colored point lights (non-shadow-casting: only a few point
	// shadow slots exist, and 200 shadowed lights is not the workload here).
	for _ in 0..LIGHTS {
		let color = Color::rgb(rng.range(0.3, 1.0), rng.range(0.3, 1.0), rng.range(0.3, 1.0));
		commands.spawn(PointLightBundle {
			local: LocalTransform3d::from_xyz(
				rng.range(-FIELD, FIELD),
				rng.range(3.0, 30.0),
				rng.range(-FIELD, FIELD),
			),
			light: PointLight {
				color,
				intensity: 600.0,
				radius: 22.0,
				casts_shadows: false,
				..PointLight::default()
			},
			..PointLightBundle::default()
		});
	}

	// a dim sun so the ground reads even away from the point lights.
	commands.spawn(DirectionalLightBundle {
		local: LocalTransform3d {
			rotation: Quat::from_rotation_x(-0.8),
			..LocalTransform3d::default()
		},
		light: DirectionalLight {
			color: Color::rgb(0.6, 0.65, 0.8),
			illuminance: 12_000.0,
			casts_shadows: true,
		},
		..DirectionalLightBundle::default()
	});

	commands.spawn(Camera3dBundle {
		local: LocalTransform3d {
			translation: Vec3::new(0.0, 60.0, 175.0),
			rotation: Quat::from_rotation_x(-0.3),
			..LocalTransform3d::default()
		},
		camera: Camera3d {
			projection: Projection::Perspective { fov_y: 60f32.to_radians(), near: 0.5, far: 2000.0 },
			priority: 0,
			active: true,
		},
		..Camera3dBundle::default()
	});
}

/// advance every orbiting cube to its position for the current elapsed time.
fn orbit_system(time: Res<Time>, mut cubes: Query<(&Orbit, &mut LocalTransform3d)>) {
	let t = time.elapsed_seconds();
	for (orbit, mut transform) in &mut cubes {
		let angle = orbit.phase + t * orbit.angular;
		transform.translation = orbit.center
			+ Vec3::new(
				angle.cos() * orbit.radius,
				(angle * 1.3).sin() * orbit.vertical,
				angle.sin() * orbit.radius,
			);
		transform.rotation = Quat::from_rotation_y(angle);
	}
}
