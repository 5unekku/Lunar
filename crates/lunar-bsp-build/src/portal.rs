//! portal extraction for BSP compilation.
//!
//! two modes:
//! - designer hints: `BspPortalHint` structs placed directly by the level author.
//! - auto-detection: scan all pairs of BSP leaves for adjacent AABBs (AABBs that
//!   touch along one axis and overlap on the other two) and emit a portal for each
//!   adjacent pair that connects different area ids.

use lunar_bsp::level::PortalData;
use lunar_math::Vec3;

/// a portal hint provided by the level designer.
///
/// use this when auto-detection misses a connection (e.g. gaps in geometry
/// at a doorway, or a portal that spans non-adjacent BSP leaves).
#[derive(Clone)]
pub struct BspPortalHint {
	pub area_a: u32,
	pub area_b: u32,
	/// world-space center of the portal opening.
	pub center: Vec3,
	/// world-space half-extents of the portal opening.
	pub half_extents: Vec3,
}

/// extract portals from the compiled BSP.
///
/// if `hints` is non-empty, they are used as-is.
/// regardless, auto-detection also runs over all leaf pairs to find adjacencies
/// between leaves with different assigned areas.
///
/// `leaf_aabbs`: (min, max) per leaf in leaf-index order.
/// `leaf_areas`: area id per leaf (None = no area assigned).
pub fn extract_portals(
	leaf_aabbs: &[([f32; 3], [f32; 3])],
	leaf_areas: &[Option<u32>],
	hints: &[BspPortalHint],
) -> Vec<PortalData> {
	let mut portals: Vec<PortalData> = hints
		.iter()
		.map(|h| PortalData {
			area_a: h.area_a,
			area_b: h.area_b,
			center: [h.center.x, h.center.y, h.center.z],
			half_extents: [h.half_extents.x, h.half_extents.y, h.half_extents.z],
		})
		.collect();

	let leaf_count = leaf_aabbs.len();
	for a in 0..leaf_count {
		let area_a = match leaf_areas[a] {
			Some(id) => id,
			None => continue,
		};
		for b in (a + 1)..leaf_count {
			let area_b = match leaf_areas[b] {
				Some(id) => id,
				None => continue,
			};
			if area_a == area_b {
				continue;
			}

			if let Some(portal) =
				aabb_adjacency_portal(leaf_aabbs[a], leaf_aabbs[b], area_a, area_b)
			{
				// deduplicate: skip if a hint already covers this area pair
				let already = portals.iter().any(|p| {
					(p.area_a == area_a && p.area_b == area_b)
						|| (p.area_a == area_b && p.area_b == area_a)
				});
				if !already {
					portals.push(portal);
				}
			}
		}
	}
	portals
}

/// check whether two leaf AABBs are adjacent (touching along one axis, overlapping
/// on the other two). if so, return a `PortalData` at the shared face.
fn aabb_adjacency_portal(
	(amin, amax): ([f32; 3], [f32; 3]),
	(bmin, bmax): ([f32; 3], [f32; 3]),
	area_a: u32,
	area_b: u32,
) -> Option<PortalData> {
	const EPS: f32 = 1e-3;

	for axis in 0usize..3 {
		let a1 = (axis + 1) % 3;
		let a2 = (axis + 2) % 3;

		let touching =
			(amax[axis] - bmin[axis]).abs() < EPS || (bmax[axis] - amin[axis]).abs() < EPS;
		if !touching {
			continue;
		}

		// must overlap on the other two axes to be a real shared face
		let overlap1 = amax[a1] > bmin[a1] + EPS && bmax[a1] > amin[a1] + EPS;
		let overlap2 = amax[a2] > bmin[a2] + EPS && bmax[a2] > amin[a2] + EPS;
		if !overlap1 || !overlap2 {
			continue;
		}

		// portal is the intersection rectangle on the shared face
		let mut pmin = [0.0f32; 3];
		let mut pmax = [0.0f32; 3];
		for i in 0..3 {
			pmin[i] = amin[i].max(bmin[i]);
			pmax[i] = amax[i].min(bmax[i]);
		}
		// force the portal flat on the touching axis
		let touch_val = if (amax[axis] - bmin[axis]).abs() < EPS {
			amax[axis]
		} else {
			bmax[axis]
		};
		pmin[axis] = touch_val;
		pmax[axis] = touch_val;

		let center = [
			(pmin[0] + pmax[0]) * 0.5,
			(pmin[1] + pmax[1]) * 0.5,
			(pmin[2] + pmax[2]) * 0.5,
		];
		let half_extents = [
			((pmax[0] - pmin[0]) * 0.5).max(0.0),
			((pmax[1] - pmin[1]) * 0.5).max(0.0),
			((pmax[2] - pmin[2]) * 0.5).max(0.0),
		];

		return Some(PortalData {
			area_a,
			area_b,
			center,
			half_extents,
		});
	}
	None
}
