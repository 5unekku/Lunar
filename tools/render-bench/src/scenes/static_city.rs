//! static-city: 4096 textured, shadow-casting static buildings on a ground
//! plane under a single shadow-casting sun. stresses the static draw path —
//! frustum culling, the static-slot bookkeeping, shadow caster gather, and
//! bindless texture batching — with nothing moving frame to frame.

use lunar::lunar_3d::Aabb3d;
use lunar::lunar_math::Vec3A;
use lunar::prelude::*;

use crate::common::{checker_texture, noise_texture, Rng};

const GRID: i32 = 64; // 64 × 64 = 4096 buildings
const SPACING: f32 = 4.0;

pub fn register(app: &mut App) {
	app.add_startup_system(spawn);
}

fn spawn(mut commands: Commands, mut registry: ResMut<MeshRegistry>, mut assets: ResMut<AssetServer>) {
	let cube = registry.add_mesh(primitives::unit_cube());
	let ground_mesh = registry.add_mesh(primitives::quad_mesh(GRID as f32 * SPACING, GRID as f32 * SPACING));

	// a handful of textured pbr materials the buildings pick from, so the scene
	// exercises bindless texture indexing rather than one flat color.
	let palette = [
		([0.55, 0.55, 0.60], checker_texture(&mut assets, 64, 8, [190, 190, 200, 255], [120, 120, 130, 255])),
		([0.60, 0.45, 0.40], noise_texture(&mut assets, 64, 0x1111, [170, 130, 110])),
		([0.40, 0.50, 0.60], checker_texture(&mut assets, 64, 4, [110, 140, 170, 255], [70, 90, 120, 255])),
		([0.65, 0.62, 0.50], noise_texture(&mut assets, 64, 0x2222, [190, 180, 150])),
	];
	let materials: Vec<_> = palette
		.iter()
		.map(|(rgb, tex)| {
			registry.add_material(MaterialData {
				shading: ShadingModel::Pbr,
				base_color: Color::rgb(rgb[0], rgb[1], rgb[2]),
				diffuse: Some(*tex),
				roughness: 0.8,
				metallic: 0.0,
				..MaterialData::default()
			})
		})
		.collect();

	let ground_tex = checker_texture(&mut assets, 128, 16, [60, 65, 55, 255], [45, 50, 42, 255]);
	let ground_mat = registry.add_material(MaterialData {
		shading: ShadingModel::Pbr,
		base_color: Color::rgb(0.3, 0.32, 0.28),
		diffuse: Some(ground_tex),
		roughness: 0.95,
		..MaterialData::default()
	});
	commands.spawn((
		Mesh3dBundle::at(Vec3::ZERO, ground_mesh, ground_mat),
		StaticMesh,
	));

	let mut rng = Rng::new(0xC175_EED0);
	let half = GRID / 2;
	for gz in -half..half {
		for gx in -half..half {
			let width = rng.range(1.0, 2.2);
			let depth = rng.range(1.0, 2.2);
			let height = rng.range(2.0, 12.0);
			let material = materials[rng.index(materials.len())];
			let translation = Vec3::new(gx as f32 * SPACING, height * 0.5, gz as f32 * SPACING);

			commands.spawn((
				ShadowMesh3dBundle {
					base: Mesh3dBundle {
						local: LocalTransform3d {
							translation,
							rotation: Quat::IDENTITY,
							scale: Vec3::new(width, height, depth),
						},
						mesh: Mesh3d(cube),
						material: Material3d(material),
						..Mesh3dBundle::default()
					},
					// local-space bounds of the unit cube; the world transform's
					// scale expands it during culling.
					aabb: Aabb3d { center: Vec3A::ZERO, half_extents: Vec3A::splat(0.5) },
					shadow_caster: ShadowCaster,
				},
				StaticMesh,
			));
		}
	}

	// the sun: a single shadow-casting directional light angled across the grid.
	commands.spawn(DirectionalLightBundle {
		local: LocalTransform3d {
			rotation: Quat::from_rotation_x(-0.9) * Quat::from_rotation_y(0.5),
			..LocalTransform3d::default()
		},
		light: DirectionalLight {
			color: Color::rgb(1.0, 0.97, 0.9),
			illuminance: 80_000.0,
			casts_shadows: true,
		},
		..DirectionalLightBundle::default()
	});

	// elevated camera framing the whole city, pitched down onto the grid.
	commands.spawn(Camera3dBundle {
		local: LocalTransform3d {
			translation: Vec3::new(0.0, 70.0, 190.0),
			rotation: Quat::from_rotation_x(-0.32),
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
