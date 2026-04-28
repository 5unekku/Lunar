//! bin-packing algorithm for texture atlas construction.
//!
//! the [`AtlasPacker`] takes a list of source images and packs them into
//! a single large texture using shelf packing.

use crate::manifest::{AtlasManifest, ManifestRegion};
use engine_image::Image;
use std::collections::HashMap;

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
    /// the packed image (ready to encode as .mi)
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

        let mut shelves: Vec<Shelf> = Vec::new();
        let mut current_y: u32 = 0;
        let mut regions: HashMap<String, ManifestRegion> = HashMap::new();

        // track actual used dimensions
        let mut used_w: u32 = 0;
        let mut used_h: u32 = 0;

        // pre-allocate canvas at max size
        let canvas_size = (self.max_width * self.max_height * 4) as usize;
        let mut canvas = vec![0u8; canvas_size];

        for source in sorted {
            let img = &source.image;
            let w = img.width;
            let h = img.height;

            // try to place on an existing shelf
            let mut placed = false;
            for shelf in &mut shelves {
                if shelf.height >= h && shelf.cursor_x + w <= self.max_width {
                    let x = shelf.cursor_x;
                    let y = shelf.y;
                    shelf.cursor_x += w;

                    Self::blit(&mut canvas, self.max_width, x, y, img);
                    used_w = used_w.max(x + w);
                    used_h = used_h.max(y + h);

                    regions.insert(source.name.clone(), ManifestRegion { x, y, w, h });
                    placed = true;
                    break;
                }
            }

            if !placed {
                // need a new shelf
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

                Self::blit(&mut canvas, self.max_width, x, y, img);
                used_w = used_w.max(w);
                used_h = used_h.max(y + h);

                regions.insert(source.name.clone(), ManifestRegion { x, y, w, h });
            }
        }

        // handle empty case
        if used_w == 0 || used_h == 0 {
            return Ok(PackedAtlas {
                image: Image::new(1, 1),
                manifest: AtlasManifest {
                    atlas_width: 1,
                    atlas_height: 1,
                    regions: HashMap::new(),
                },
            });
        }

        // trim to actual used dimensions — row-copy is much faster than pixel-by-pixel
        let mut packed = Image::new(used_w, used_h);
        let row_bytes = (used_w * 4) as usize;
        for y in 0..used_h {
            let src_start = (y * self.max_width * 4) as usize;
            let dst_start = (y * used_w * 4) as usize;
            packed.pixels[dst_start..dst_start + row_bytes]
                .copy_from_slice(&canvas[src_start..src_start + row_bytes]);
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
