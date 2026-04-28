//! a named region within a texture atlas.
//!
//! an [`AtlasRegion`] defines a sub-rectangle of the atlas texture
//! using UV coordinates (0.0 to 1.0 range).

use engine_math::Vec2;

/// a region within a texture atlas.
///
/// UV coordinates are in 0.0 to 1.0 range relative to the full atlas texture.
#[derive(Debug, Clone)]
pub struct AtlasRegion {
    /// UV coordinates of the top-left corner
    pub uv_min: Vec2,
    /// UV coordinates of the bottom-right corner
    pub uv_max: Vec2,
}

impl AtlasRegion {
    /// create a new atlas region from pixel coordinates and atlas dimensions.
    #[must_use]
    pub fn from_pixels(x: u32, y: u32, w: u32, h: u32, atlas_w: u32, atlas_h: u32) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let aw = atlas_w as f32;
        #[allow(clippy::cast_precision_loss)]
        let ah = atlas_h as f32;
        Self {
            #[allow(clippy::cast_precision_loss)]
            uv_min: Vec2::new(x as f32 / aw, y as f32 / ah),
            #[allow(clippy::cast_precision_loss)]
            uv_max: Vec2::new((x + w) as f32 / aw, (y + h) as f32 / ah),
        }
    }

    /// create a new atlas region from UV coordinates directly.
    #[must_use]
    pub const fn from_uv(uv_min: Vec2, uv_max: Vec2) -> Self {
        Self { uv_min, uv_max }
    }
}
