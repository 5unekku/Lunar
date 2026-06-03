//! GLTF/GLB level mesh loader.
//!
//! converts a GLTF file into a list of [`crate::BspInputMesh`]s suitable for
//! passing to [`crate::compile_bsp`]. mesh node transforms are fully applied
//! (world-space vertices), so the BSP is built in the same coordinate system
//! the engine uses at runtime.
//!
//! area ids are not stored in GLTF natively; assign them via [`crate::BspInputMesh`]
//! after loading, or embed them as mesh name prefixes (e.g. `area0_floor`).

use crate::BspInputMesh;
use lunar_math::Vec3;

/// load all meshes from a GLTF or GLB file into [`BspInputMesh`] structs.
///
/// node transforms are accumulated from the scene root so all vertices are in
/// world space. multi-mesh nodes produce one `BspInputMesh` per primitive.
///
/// area ids are parsed from mesh names: a name prefix of `areaNN_` (where NN is
/// a decimal integer) sets `area_id = Some(NN)`. meshes without this prefix get
/// `area_id = None`.
///
/// # Errors
///
/// returns an error string if the file cannot be read or parsed.
pub fn load_gltf_meshes(path: &str) -> Result<Vec<BspInputMesh>, String> {
	let (doc, buffers, _images) =
		gltf::import(path).map_err(|error| format!("gltf load error for '{path}': {error}"))?;

	let scene = doc
		.default_scene()
		.or_else(|| doc.scenes().next())
		.ok_or_else(|| format!("gltf '{path}' has no scenes"))?;

	let mut meshes: Vec<BspInputMesh> = Vec::new();
	let identity = [
		[1.0f32, 0.0, 0.0, 0.0],
		[0.0, 1.0, 0.0, 0.0],
		[0.0, 0.0, 1.0, 0.0],
		[0.0, 0.0, 0.0, 1.0],
	];
	for node in scene.nodes() {
		collect_node(&node, &identity, &buffers, &mut meshes);
	}
	Ok(meshes)
}

fn collect_node(
	node: &gltf::Node,
	parent_transform: &[[f32; 4]; 4],
	buffers: &[gltf::buffer::Data],
	meshes: &mut Vec<BspInputMesh>,
) {
	let local = node.transform().matrix();
	let world = mat4_mul(parent_transform, &local);

	if let Some(mesh) = node.mesh() {
		let area_id = parse_area_id(mesh.name().unwrap_or(""));
		for prim in mesh.primitives() {
			let reader = prim.reader(|buf| Some(&buffers[buf.index()]));
			let positions: Vec<[f32; 3]> = match reader.read_positions() {
				Some(iter) => iter.collect(),
				None => continue,
			};
			let indices: Vec<u32> = match reader.read_indices() {
				Some(iter) => iter.into_u32().collect(),
				None => {
					// no index buffer: generate sequential indices
					(0..positions.len() as u32).collect()
				}
			};

			let vertices: Vec<Vec3> = positions
				.iter()
				.map(|p| transform_point(&world, Vec3::new(p[0], p[1], p[2])))
				.collect();

			if vertices.len() >= 3 && !indices.is_empty() {
				meshes.push(BspInputMesh {
					vertices,
					indices,
					area_id,
				});
			}
		}
	}

	for child in node.children() {
		collect_node(&child, &world, buffers, meshes);
	}
}

fn parse_area_id(name: &str) -> Option<u32> {
	// match names like "area0_floor", "area12_wall", etc.
	let without_prefix = name.strip_prefix("area")?;
	let end = without_prefix
		.find(|c: char| !c.is_ascii_digit())
		.unwrap_or(without_prefix.len());
	if end == 0 {
		return None;
	}
	without_prefix[..end].parse().ok()
}

fn transform_point(m: &[[f32; 4]; 4], p: Vec3) -> Vec3 {
	let x = m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0];
	let y = m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1];
	let z = m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2];
	Vec3::new(x, y, z)
}

fn mat4_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
	let mut out = [[0.0f32; 4]; 4];
	for row in 0..4 {
		for col in 0..4 {
			for k in 0..4 {
				out[row][col] += a[row][k] * b[k][col];
			}
		}
	}
	out
}
