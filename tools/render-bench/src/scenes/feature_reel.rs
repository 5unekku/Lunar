//! feature-reel: the "mixed everything" 3d scene that anchors the golden-frame
//! gate. alongside clipmap terrain, atmospheric sky, mixed pbr meshes, cascade +
//! point shadows and a full light rig, it spawns every scene-content feature pass
//! the other scenes never draw: water, decals, particle emitters and detail sprites.
//! (water and detail sprites were left out while rt-02 / rt-03 aborted the engine;
//! see docs/superpowers/audits/2026-07-harness-runtime-bugs.md.)

use lunar::lunar_3d::{
	Aabb3d, ComputedVisibility, Decal, DetailDensity, ParticleEmitter, Terrain, Visibility, Water,
};
use lunar::lunar_math::Vec3A;
use lunar::lunar_render_3d::AtmosphericScattering;
use lunar::prelude::*;

use crate::common::{checker_texture, f32_to_f16_bits};

const HEIGHTMAP: u32 = 256;
const TERRAIN_WORLD: f32 = 400.0;

pub fn register(app: &mut App) {
	app.add_startup_system(spawn);
}

fn spawn(mut commands: Commands, mut registry: ResMut<MeshRegistry>, mut assets: ResMut<AssetServer>) {
	// atmospheric sky drives the ambient + sun-disc look.
	commands.insert_resource(AtmosphericScattering::default());

	// ── terrain ──────────────────────────────────────────────────────────────
	commands.spawn((
		Terrain {
			heightmap: rolling_heightmap(HEIGHTMAP),
			heightmap_width: HEIGHTMAP,
			heightmap_height: HEIGHTMAP,
			world_size: TERRAIN_WORLD,
			height_scale: 40.0,
			tint: Color::rgb(0.45, 0.55, 0.35),
			dirty: true,
			..Terrain::default()
		},
		LocalTransform3d::default(),
		WorldTransform3d::default(),
	));

	// flat ground quad under the terrain edges.
	let ground_mat = registry.add_material(MaterialData {
		shading: ShadingModel::Pbr,
		base_color: Color::rgb(0.4, 0.5, 0.3),
		roughness: 0.95,
		..MaterialData::default()
	});
	let ground_mesh = registry.add_mesh(primitives::quad_mesh(TERRAIN_WORLD * 0.5, TERRAIN_WORLD * 0.5));
	commands.spawn((
		Mesh3dBundle::at(Vec3::new(0.0, 0.05, 0.0), ground_mesh, ground_mat),
		StaticMesh,
	));

	// ── mixed meshes: alternating spheres and cylinders in a ring ──────────────
	let sphere = registry.add_mesh(primitives::sphere_mesh(3.0, 24, 16));
	let cylinder = registry.add_mesh(primitives::cylinder_mesh(2.0, 8.0, 20, true));
	let metal_tex = checker_texture(&mut assets, 32, 4, [200, 200, 210, 255], [90, 90, 100, 255]);
	let metal = registry.add_material(MaterialData {
		shading: ShadingModel::Pbr,
		base_color: Color::rgb(0.8, 0.8, 0.85),
		diffuse: Some(metal_tex),
		roughness: 0.25,
		metallic: 0.9,
		..MaterialData::default()
	});
	for i in 0..12 {
		let angle = i as f32 / 12.0 * std::f32::consts::TAU;
		let mesh = if i % 2 == 0 { sphere } else { cylinder };
		commands.spawn((
			ShadowMesh3dBundle {
				base: Mesh3dBundle {
					local: LocalTransform3d::from_xyz(angle.cos() * 30.0, 8.0, angle.sin() * 30.0),
					mesh: Mesh3d(mesh),
					material: Material3d(metal),
					..Mesh3dBundle::default()
				},
				aabb: Aabb3d { center: Vec3A::ZERO, half_extents: Vec3A::splat(4.0) },
				shadow_caster: ShadowCaster,
			},
			StaticMesh,
		));
	}

	// ── scene-content feature passes ─────────────────────────────────────────
	// water: a pond in front of the ring (gerstner waves + refraction of the scene)
	let pond = registry.add_mesh(primitives::quad_mesh(40.0, 40.0));
	commands.spawn((
		Water::default(),
		Mesh3d(pond),
		LocalTransform3d::from_xyz(0.0, 1.5, 60.0),
		WorldTransform3d::default(),
	));

	// decals: projected onto the ground inside the ring
	for i in 0..4 {
		let angle = i as f32 / 4.0 * std::f32::consts::TAU;
		commands.spawn((
			Decal::default(),
			LocalTransform3d {
				translation: Vec3::new(angle.cos() * 12.0, 0.5, angle.sin() * 12.0),
				scale: Vec3::new(6.0, 2.0, 6.0),
				..LocalTransform3d::default()
			},
			WorldTransform3d::default(),
		));
	}

	// particle emitters: two fountains (spawn accumulation is tick-deterministic)
	for x in [-20.0, 20.0] {
		commands.spawn((
			ParticleEmitter::default(),
			LocalTransform3d {
				translation: Vec3::new(x, 2.0, 30.0),
				rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
				..LocalTransform3d::default()
			},
			WorldTransform3d::default(),
		));
	}

	// detail sprites: ground cover around the ring, driven by a checker density map
	let grass_atlas = checker_texture(&mut assets, 64, 16, [90, 150, 60, 255], [60, 110, 40, 255]);
	let grass_density = checker_texture(&mut assets, 64, 8, [255, 0, 0, 255], [96, 0, 0, 255]);
	commands.spawn((
		DetailDensity {
			texture: grass_atlas,
			density_map: grass_density,
			world_origin: Vec2::new(-50.0, -50.0),
			world_size: Vec2::new(100.0, 100.0),
			max_dist: 120.0,
			size_range: [0.3, 0.7],
			variant_count: 4,
			density_scale: 2.0,
			grid_step: 0.5,
		},
		LocalTransform3d::from_xyz(0.0, 0.1, 0.0),
		WorldTransform3d::default(),
		Visibility::Visible,
		ComputedVisibility(true),
	));

	// ── lights ───────────────────────────────────────────────────────────────
	commands.spawn(DirectionalLightBundle {
		local: LocalTransform3d {
			rotation: Quat::from_rotation_x(-0.7) * Quat::from_rotation_y(0.4),
			..LocalTransform3d::default()
		},
		light: DirectionalLight {
			color: Color::rgb(1.0, 0.95, 0.85),
			illuminance: 60_000.0,
			casts_shadows: true,
		},
		..DirectionalLightBundle::default()
	});
	for i in 0..6 {
		let angle = i as f32 / 6.0 * std::f32::consts::TAU;
		commands.spawn(PointLightBundle {
			local: LocalTransform3d::from_xyz(angle.cos() * 40.0, 12.0, angle.sin() * 40.0),
			light: PointLight {
				color: Color::rgb(0.9, 0.6, 0.4),
				intensity: 800.0,
				radius: 30.0,
				casts_shadows: i == 0,
				..PointLight::default()
			},
			..PointLightBundle::default()
		});
	}

	// ── camera ───────────────────────────────────────────────────────────────
	commands.spawn(Camera3dBundle {
		local: LocalTransform3d {
			translation: Vec3::new(0.0, 45.0, 120.0),
			rotation: Quat::from_rotation_x(-0.28),
			..LocalTransform3d::default()
		},
		camera: Camera3d {
			projection: Projection::Perspective { fov_y: 60f32.to_radians(), near: 0.5, far: 3000.0 },
			priority: 0,
			active: true,
		},
		..Camera3dBundle::default()
	});
}

/// a smooth rolling heightmap as R16Float texels (little-endian), for the
/// clipmap terrain. deterministic — a fixed analytic surface, no rng.
fn rolling_heightmap(size: u32) -> Vec<u8> {
	let mut bytes = Vec::with_capacity((size * size * 2) as usize);
	for y in 0..size {
		for x in 0..size {
			let u = x as f32 / size as f32;
			let v = y as f32 / size as f32;
			// two low-frequency sinusoids, normalized to [0, 1].
			let h = 0.5 + 0.25 * (u * 6.0).sin() * (v * 5.0).cos() + 0.15 * (u * 13.0 + v * 7.0).sin();
			let bits = f32_to_f16_bits(h.clamp(0.0, 1.0));
			bytes.extend_from_slice(&bits.to_le_bytes());
		}
	}
	bytes
}
