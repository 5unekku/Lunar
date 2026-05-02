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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_pixels_origin() {
        let r = AtlasRegion::from_pixels(0, 0, 100, 50, 200, 100);
        assert_eq!(r.uv_min.x, 0.0);
        assert_eq!(r.uv_min.y, 0.0);
        assert_eq!(r.uv_max.x, 0.5);
        assert_eq!(r.uv_max.y, 0.5);
    }

    #[test]
    fn from_pixels_middle() {
        let r = AtlasRegion::from_pixels(100, 50, 100, 50, 200, 100);
        assert!((r.uv_min.x - 0.5).abs() < 1e-6);
        assert!((r.uv_min.y - 0.5).abs() < 1e-6);
        assert!((r.uv_max.x - 1.0).abs() < 1e-6);
        assert!((r.uv_max.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn from_pixels_single_pixel() {
        let r = AtlasRegion::from_pixels(5, 5, 1, 1, 10, 10);
        assert!((r.uv_min.x - 0.5).abs() < 1e-6);
        assert!((r.uv_max.x - 0.6).abs() < 1e-6);
    }

    #[test]
    fn from_uv_identity() {
        let r = AtlasRegion::from_uv(Vec2::new(0.25, 0.25), Vec2::new(0.75, 0.75));
        assert_eq!(r.uv_min.x, 0.25);
        assert_eq!(r.uv_min.y, 0.25);
        assert_eq!(r.uv_max.x, 0.75);
        assert_eq!(r.uv_max.y, 0.75);
    }
}
