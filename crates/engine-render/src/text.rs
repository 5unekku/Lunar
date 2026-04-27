//! text rendering via a glyph atlas texture

use engine_math::Vec2;
use std::collections::HashMap;

/// a single glyph in the atlas.
#[derive(Debug, Clone)]
pub struct GlyphInfo {
    /// x offset within the atlas texture.
    pub x: u32,
    /// y offset within the atlas texture.
    pub y: u32,
    /// glyph width in pixels.
    pub width: u32,
    /// glyph height in pixels.
    pub height: u32,
    /// horizontal bearing (offset from cursor to glyph left edge).
    pub bearing_x: f32,
    /// vertical bearing (offset from baseline to glyph top edge).
    pub bearing_y: f32,
    /// how far to advance the cursor after this glyph.
    pub advance: f32,
}

/// key for the glyph cache — uses a quantized font size to avoid f32 hash issues.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    font_id: u32,
    char_code: char,
    font_size: u16,
}

/// a glyph atlas texture managed on the GPU.
#[derive(Debug)]
#[allow(dead_code)]
pub struct GlyphAtlas {
    /// atlas texture width in pixels.
    pub width: u32,
    /// atlas texture height in pixels.
    pub height: u32,
    /// packed pixel data (rgba).
    pub pixels: Vec<u8>,
    /// cached glyphs by (font_id, char, quantized_font_size).
    pub glyphs: HashMap<GlyphKey, GlyphInfo>,
    /// current insertion cursor in the atlas.
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
}

impl GlyphAtlas {
    /// create a new empty atlas at the given size.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
            glyphs: HashMap::new(),
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
        }
    }

    /// rasterize a glyph and insert it into the atlas. returns false if the atlas is full.
    #[allow(dead_code)]
    pub fn rasterize_glyph(
        &mut self,
        font: &fontdue::Font,
        font_id: u32,
        char_code: char,
        font_size: f32,
    ) -> bool {
        let key = GlyphKey {
            font_id,
            char_code,
            font_size: font_size.round() as u16,
        };
        if self.glyphs.contains_key(&key) {
            return true;
        }

        let (metrics, bitmap) = font.rasterize(char_code, font_size);

        let gw = metrics.width as u32;
        let gh = metrics.height as u32;

        if gw == 0 || gh == 0 {
            // space or invisible glyph
            let info = GlyphInfo {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                bearing_x: metrics.xmin as f32,
                bearing_y: -(metrics.ymin as f32),
                advance: metrics.advance_width,
            };
            self.glyphs.insert(key, info);
            return true;
        }

        // wrap to next row if needed
        if self.cursor_x + gw > self.width {
            self.cursor_x = 0;
            self.cursor_y += self.row_height;
            self.row_height = 0;
        }

        // atlas full?
        if self.cursor_y + gh > self.height {
            return false;
        }

        // copy bitmap into atlas pixels
        for gy in 0..gh {
            for gx in 0..gw {
                let src_idx = (gy * gw + gx) as usize;
                let dst_x = self.cursor_x + gx;
                let dst_y = self.cursor_y + gy;
                let dst_idx = ((dst_y * self.width + dst_x) * 4) as usize;

                let alpha = bitmap[src_idx];
                self.pixels[dst_idx] = 0xff; // r
                self.pixels[dst_idx + 1] = 0xff; // g
                self.pixels[dst_idx + 2] = 0xff; // b
                self.pixels[dst_idx + 3] = alpha; // a
            }
        }

        let info = GlyphInfo {
            x: self.cursor_x,
            y: self.cursor_y,
            width: gw,
            height: gh,
            bearing_x: metrics.xmin as f32,
            bearing_y: -(metrics.ymin as f32),
            advance: metrics.advance_width,
        };
        self.glyphs.insert(key, info);

        self.cursor_x += gw;
        if gh > self.row_height {
            self.row_height = gh;
        }

        true
    }

    /// get glyph info for a character.
    pub fn get_glyph(&self, font_id: u32, char_code: char, font_size: f32) -> Option<&GlyphInfo> {
        let key = GlyphKey {
            font_id,
            char_code,
            font_size: font_size.round() as u16,
        };
        self.glyphs.get(&key)
    }

    /// get the atlas pixel data for GPU upload.
    #[allow(dead_code)]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// layout a string of text and return the positioned glyph quads.
/// each quad is (uv_min, uv_max, position, size).
pub fn layout_text(
    atlas: &GlyphAtlas,
    font_id: u32,
    text: &str,
    font_size: f32,
    position: Vec2,
) -> Vec<TextGlyphQuad> {
    let mut quads = Vec::new();
    let mut cursor_x = position.x;
    let baseline_y = position.y;

    for ch in text.chars() {
        if let Some(info) = atlas.get_glyph(font_id, ch, font_size) {
            if info.width > 0 && info.height > 0 {
                let uv_x = info.x as f32 / atlas.width as f32;
                let uv_y = info.y as f32 / atlas.height as f32;
                let uv_w = info.width as f32 / atlas.width as f32;
                let uv_h = info.height as f32 / atlas.height as f32;

                let glyph_x = cursor_x + info.bearing_x;
                let glyph_y = baseline_y - info.bearing_y;

                quads.push(TextGlyphQuad {
                    position: Vec2::new(glyph_x, glyph_y),
                    size: Vec2::new(info.width as f32, info.height as f32),
                    uv_min: Vec2::new(uv_x, uv_y),
                    uv_max: Vec2::new(uv_x + uv_w, uv_y + uv_h),
                });
            }
            cursor_x += info.advance;
        }
    }

    quads
}

/// a single positioned glyph quad for rendering.
#[derive(Debug, Clone)]
pub struct TextGlyphQuad {
    /// top-left position in screen space.
    pub position: Vec2,
    /// size in pixels.
    pub size: Vec2,
    /// minimum UV in the atlas.
    pub uv_min: Vec2,
    /// maximum UV in the atlas.
    pub uv_max: Vec2,
}

/// measure the width of a text string at a given font size.
#[allow(dead_code)]
pub fn measure_text(atlas: &GlyphAtlas, font_id: u32, text: &str, font_size: f32) -> f32 {
    let mut width = 0.0f32;
    for ch in text.chars() {
        if let Some(info) = atlas.get_glyph(font_id, ch, font_size) {
            width += info.advance;
        }
    }
    width
}
