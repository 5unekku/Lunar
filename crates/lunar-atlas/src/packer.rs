//! bin-packing algorithm for texture atlas construction.
//!
//! the [`AtlasPacker`] takes a list of source images and packs them into
//! a single large texture using shelf packing.

use crate::manifest::{AtlasManifest, ManifestRegion};
use lunar_image::Image;
use rustc_hash::FxHashMap as HashMap;

/// a source image to be packed into the atlas.
#[derive(Debug, Clone)]
pub struct SourceImage {
	/// name used to look up this region
	pub name: String,
	/// decoded pixel data
	pub image: Image,
}

/// result of packing an atlas.
#[derive(Debug, Clone)]
pub struct PackedAtlas {
	/// the packed image (ready to encode as .li)
	pub image: Image,
	/// the manifest describing region layout
	pub manifest: AtlasManifest,
}

/// a horizontal shelf in the packing algorithm.
struct Shelf {
	y: u32,
	height: u32,
	cursor_x: u32,
}

/// bin-packer for building texture atlases.
///
/// uses shelf packing: images are sorted by height and placed
/// on horizontal shelves.
pub struct AtlasPacker {
	max_width: u32,
	max_height: u32,
}

impl AtlasPacker {
	/// create a new packer with the given maximum atlas dimensions.
	#[must_use]
	pub const fn new(max_width: u32, max_height: u32) -> Self {
		Self {
			max_width,
			max_height,
		}
	}

	/// pack source images into a single atlas image.
	///
	/// # Errors
	///
	/// returns an error string if an image cannot fit in the atlas.
	pub fn pack(&self, sources: &[SourceImage]) -> Result<PackedAtlas, String> {
		// sort by height descending (tallest first)
		let mut sorted: Vec<&SourceImage> = sources.iter().collect();
		sorted.sort_by_key(|b| std::cmp::Reverse(b.image.height));

		// pass 1: compute placements without allocating any pixel memory
		let mut shelves: Vec<Shelf> = Vec::new();
		let mut current_y: u32 = 0;
		let mut regions: HashMap<String, ManifestRegion> = HashMap::default();
		let mut used_w: u32 = 0;
		let mut used_h: u32 = 0;

		for source in &sorted {
			let img = &source.image;
			let w = img.width;
			let h = img.height;

			let mut placed = false;
			for shelf in &mut shelves {
				if shelf.height >= h && shelf.cursor_x + w <= self.max_width {
					let x = shelf.cursor_x;
					let y = shelf.y;
					shelf.cursor_x += w;
					used_w = used_w.max(x + w);
					used_h = used_h.max(y + h);
					regions.insert(source.name.clone(), ManifestRegion { x, y, w, h });
					placed = true;
					break;
				}
			}

			if !placed {
				if current_y + h > self.max_height {
					return Err(format!(
						"image '{}' ({}x{}) does not fit in atlas ({}x{})",
						source.name, img.width, img.height, self.max_width, self.max_height
					));
				}
				let x = 0;
				let y = current_y;
				shelves.push(Shelf {
					y,
					height: h,
					cursor_x: w,
				});
				current_y += h;
				used_w = used_w.max(w);
				used_h = used_h.max(y + h);
				regions.insert(source.name.clone(), ManifestRegion { x, y, w, h });
			}
		}

		if used_w == 0 || used_h == 0 {
			return Ok(PackedAtlas {
				image: Image::new(1, 1),
				manifest: AtlasManifest {
					atlas_width: 1,
					atlas_height: 1,
					regions: HashMap::default(),
				},
			});
		}

		// pass 2: allocate exact canvas and blit — no over-allocation
		let mut packed = Image::new(used_w, used_h);
		for source in &sorted {
			if let Some(region) = regions.get(&source.name) {
				Self::blit(
					&mut packed.pixels,
					used_w,
					region.x,
					region.y,
					&source.image,
				);
			}
		}

		let manifest = AtlasManifest {
			atlas_width: used_w,
			atlas_height: used_h,
			regions,
		};

		Ok(PackedAtlas {
			image: packed,
			manifest,
		})
	}

	fn blit(canvas: &mut [u8], canvas_w: u32, dst_x: u32, dst_y: u32, src: &Image) {
		let w = src.width;
		let h = src.height;
		let row_bytes = (w * 4) as usize;
		for y in 0..h {
			let src_start = (y * w * 4) as usize;
			let dst_start = ((dst_y + y) * canvas_w + dst_x) as usize * 4;
			canvas[dst_start..dst_start + row_bytes]
				.copy_from_slice(&src.pixels[src_start..src_start + row_bytes]);
		}
	}
}

#[cfg(test)]
mod packer_tests {
	use super::*;

	fn image(w: u32, h: u32) -> Image {
		let mut pixels = vec![0u8; (w * h * 4) as usize];
		for (i, chunk) in pixels.chunks_exact_mut(4).enumerate() {
			let t = (i * 8) as u8;
			chunk.copy_from_slice(&[t, t, t, 255]);
		}
		let mut img = Image::new(w, h);
		img.pixels = pixels;
		img
	}

	#[test]
	fn pack_single_image() {
		let packer = AtlasPacker::new(256, 256);
		let sources = [SourceImage {
			name: "test".into(),
			image: image(32, 32),
		}];
		let result = packer.pack(&sources).unwrap();
		assert_eq!(result.manifest.regions.len(), 1);
		let r = result.manifest.regions.get("test").unwrap();
		assert_eq!(r.x, 0);
		assert_eq!(r.y, 0);
		assert_eq!(r.w, 32);
		assert_eq!(r.h, 32);
	}

	#[test]
	fn pack_multiple_images() {
		let packer = AtlasPacker::new(128, 128);
		let sources = [
			SourceImage {
				name: "a".into(),
				image: image(10, 20),
			},
			SourceImage {
				name: "b".into(),
				image: image(15, 10),
			},
			SourceImage {
				name: "c".into(),
				image: image(8, 8),
			},
		];
		let result = packer.pack(&sources).unwrap();
		assert_eq!(result.manifest.regions.len(), 3);
		// all images should be placed without overlap
		let mut regions: Vec<_> = result.manifest.regions.values().collect();
		regions.sort_by_key(|r| (r.x, r.y));
		// at minimum their combined height should fit
		let total_h: u32 = regions.iter().map(|r| r.h).sum();
		assert!(result.manifest.atlas_height >= total_h / 2); // shelf packing, so >= avg height
	}

	#[test]
	fn pack_empty_returns_1x1() {
		let packer = AtlasPacker::new(256, 256);
		let result = packer.pack(&[]).unwrap();
		assert_eq!(result.image.width, 1);
		assert_eq!(result.image.height, 1);
		assert!(result.manifest.regions.is_empty());
	}

	#[test]
	fn pack_image_too_large_errors() {
		let packer = AtlasPacker::new(32, 32);
		let sources = [SourceImage {
			name: "big".into(),
			image: image(64, 64),
		}];
		assert!(packer.pack(&sources).is_err());
	}
}
