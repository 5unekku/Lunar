//! text rendering via a glyph atlas texture.
//!
//! provides a glyph atlas for rasterizing and caching font characters,
//! plus layout functions for positioning text on screen.

use engine_math::Vec2;
use std::collections::HashMap;

/// metrics for a single rasterized glyph in the atlas.
///
/// stores the position, size, and bearing information needed to
/// correctly place and render a character.
#[derive(Debug, Clone)]
pub struct GlyphInfo {
    /// x offset of the glyph within the atlas texture.
    pub x: u32,
    /// y offset of the glyph within the atlas texture.
    pub y: u32,
    /// glyph width in pixels.
    pub width: u32,
    /// glyph height in pixels.
    pub height: u32,
    /// horizontal bearing: offset from cursor to glyph left edge.
    pub bearing_x: f32,
    /// vertical bearing: offset from baseline to glyph top edge.
    pub bearing_y: f32,
    /// how far to advance the cursor after rendering this glyph.
    pub advance: f32,
}

/// cache key for looking up rasterized glyphs.
///
/// uses a quantized (rounded) font size to avoid floating-point
/// hash issues. uniquely identifies a glyph by font, character, and size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// font identifier.
    font_id: u32,
    /// the unicode character.
    char_code: char,
    /// font size in pixels (rounded to nearest integer).
    font_size: u16,
}

/// a packed glyph atlas texture.
///
/// stores rasterized glyphs in a single 2D pixel buffer.
/// glyphs are placed sequentially, wrapping to new rows as needed.
/// the atlas is uploaded to the GPU as a texture for rendering.
///
/// fonts are registered via [`register_font`] with raw bytes; the atlas
/// parses and caches them internally so callers never touch fontdue/freetype directly.
#[derive(Debug)]
#[allow(dead_code)]
pub struct GlyphAtlas {
    /// atlas texture width in pixels.
    pub width: u32,
    /// atlas texture height in pixels.
    pub height: u32,
    /// packed RGBA pixel data.
    pub pixels: Vec<u8>,
    /// cached glyphs keyed by (`font_id`, character, quantized font size).
    pub glyphs: HashMap<GlyphKey, GlyphInfo>,
    /// parsed fonts keyed by font_id.
    font_cache: HashMap<u32, fontdue::Font>,
    /// current x position for the next glyph insertion.
    cursor_x: u32,
    /// current y position for the next glyph insertion.
    cursor_y: u32,
    /// height of the current row in the atlas.
    row_height: u32,
}

impl GlyphAtlas {
    /// create a new empty atlas with the given dimensions.
    /// the pixel buffer is initialized to transparent black.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
            glyphs: HashMap::new(),
            font_cache: HashMap::new(),
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
        }
    }

    /// register a font from raw bytes.
    ///
    /// the atlas parses and caches the font internally. subsequent calls to
    /// [`rasterize_glyph`] with this `font_id` will use the cached font.
    /// no-op if the font_id is already registered.
    #[allow(dead_code)]
    pub fn register_font(&mut self, font_id: u32, data: &[u8]) {
        if self.font_cache.contains_key(&font_id) {
            return;
        }
        match fontdue::Font::from_bytes(data, fontdue::FontSettings::default()) {
            Ok(font) => {
                self.font_cache.insert(font_id, font);
            }
            Err(e) => log::warn!("failed to parse font {font_id}: {e}"),
        }
    }

    /// rasterize a glyph and insert it into the atlas.
    ///
    /// returns `false` if the atlas doesn't have enough space, the font_id is
    /// not registered, or the glyph can't be rasterized. if the glyph is already
    /// cached, this is a no-op and returns `true`.
    ///
    /// call [`register_font`] before rasterizing glyphs for a new font.
    #[allow(dead_code)]
    pub fn rasterize_glyph(&mut self, font_id: u32, char_code: char, font_size: f32) -> bool {
        let key = GlyphKey {
            font_id,
            char_code,
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            font_size: font_size.round().max(0.0).min(f32::from(u16::MAX)) as u16,
        };
        if self.glyphs.contains_key(&key) {
            return true;
        }

        // borrow font immutably to rasterize, then drop the borrow before touching pixels
        let (metrics, bitmap) = match self.font_cache.get(&font_id) {
            Some(font) => font.rasterize(char_code, font_size),
            None => return false,
        };

        let gw = u32::try_from(metrics.width).unwrap_or(0);
        let gh = u32::try_from(metrics.height).unwrap_or(0);

        if gw == 0 || gh == 0 {
            // space or invisible glyph — no bitmap, but still needs advance/bearing
            let info = GlyphInfo {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                #[allow(clippy::cast_precision_loss)]
                bearing_x: metrics.xmin as f32,
                // ymin is bottom of bitmap relative to baseline (negative for descenders).
                // top of bitmap = ymin + height; for zero-size glyphs height is 0 so this is ymin.
                #[allow(clippy::cast_precision_loss)]
                bearing_y: (metrics.ymin + metrics.height as i32) as f32,
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

        // copy bitmap into atlas pixels using row-copy for better performance
        for gy in 0..gh {
            let src_row_start = (gy * gw) as usize;
            let src_row = &bitmap[src_row_start..src_row_start + gw as usize];
            let dst_y = self.cursor_y + gy;
            let dst_x = self.cursor_x;
            let dst_row_start = ((dst_y * self.width + dst_x) * 4) as usize;

            for (gx, &alpha) in src_row.iter().enumerate() {
                let dst_idx = dst_row_start + gx * 4;
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
            #[allow(clippy::cast_precision_loss)]
            bearing_x: metrics.xmin as f32,
            // top of bitmap above baseline = ymin + height (positive = above baseline).
            // used as: glyph_y = baseline_y - bearing_y (screen y-down coords).
            #[allow(clippy::cast_precision_loss)]
            bearing_y: (metrics.ymin + metrics.height as i32) as f32,
            advance: metrics.advance_width,
        };
        self.glyphs.insert(key, info);

        self.cursor_x += gw;
        if gh > self.row_height {
            self.row_height = gh;
        }

        true
    }

    /// look up cached glyph info for a character.
    /// returns `None` if the glyph hasn't been rasterized yet.
    pub fn get_glyph(&self, font_id: u32, char_code: char, font_size: f32) -> Option<&GlyphInfo> {
        let key = GlyphKey {
            font_id,
            char_code,
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            font_size: font_size.round().max(0.0).min(f32::from(u16::MAX)) as u16,
        };
        self.glyphs.get(&key)
    }

    /// get the raw pixel data for uploading to the GPU.
    #[allow(dead_code)]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// layout a string of text and return positioned glyph quads.
///
/// iterates over each character, looks up its glyph in the atlas,
/// and computes screen-space positions and UV coordinates.
/// each quad contains the position, size, and UV bounds for rendering.
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
                #[allow(clippy::cast_precision_loss)]
                let uv_x = info.x as f32 / atlas.width as f32;
                #[allow(clippy::cast_precision_loss)]
                let uv_y = info.y as f32 / atlas.height as f32;
                #[allow(clippy::cast_precision_loss)]
                let uv_w = info.width as f32 / atlas.width as f32;
                #[allow(clippy::cast_precision_loss)]
                let uv_h = info.height as f32 / atlas.height as f32;

                let glyph_x = cursor_x + info.bearing_x;
                let glyph_y = baseline_y - info.bearing_y;

                quads.push(TextGlyphQuad {
                    position: Vec2::new(glyph_x, glyph_y),
                    #[allow(clippy::cast_precision_loss)]
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
///
/// contains the screen-space position, pixel size, and UV coordinates
/// within the glyph atlas. used to generate vertex data for text rendering.
#[derive(Debug, Clone)]
pub struct TextGlyphQuad {
    /// top-left position in screen space.
    pub position: Vec2,
    /// size in pixels.
    pub size: Vec2,
    /// minimum UV coordinate in the atlas (top-left of glyph).
    pub uv_min: Vec2,
    /// maximum UV coordinate in the atlas (bottom-right of glyph).
    pub uv_max: Vec2,
}

/// measure the total advance width of a text string at a given font size.
///
/// sums the advance values of all rasterized glyphs in the string.
/// invisible or uncached glyphs are skipped.
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

/// layout text with word-wrapping at `max_width` pixels.
///
/// splits `text` on word boundaries, accumulates words per line until
/// the line would exceed `max_width`, then starts a new line.
/// each returned `Vec<TextGlyphQuad>` is one visual line, positioned at
/// `position + Vec2::new(0, line * line_height)`.
///
/// `line_height` defaults to `font_size * 1.25` when passed as 0.0.
pub fn layout_text_wrapped(
    atlas: &GlyphAtlas,
    font_id: u32,
    text: &str,
    font_size: f32,
    position: Vec2,
    max_width: f32,
    line_height: f32,
) -> Vec<Vec<TextGlyphQuad>> {
    let effective_line_height = if line_height > 0.0 {
        line_height
    } else {
        font_size * 1.25
    };
    let mut lines: Vec<Vec<TextGlyphQuad>> = Vec::new();
    let mut line_y = 0.0f32;

    for paragraph in text.split('\n') {
        let words: Vec<&str> = paragraph.split_whitespace().collect();
        let mut current_line = String::new();
        let mut current_width = 0.0f32;

        for word in words {
            let word_width = measure_text(atlas, font_id, word, font_size);
            let space_width = if current_line.is_empty() {
                0.0
            } else {
                measure_text(atlas, font_id, " ", font_size)
            };

            if !current_line.is_empty() && current_width + space_width + word_width > max_width {
                // flush current line
                let line_pos = Vec2::new(position.x, position.y + line_y);
                lines.push(layout_text(
                    atlas,
                    font_id,
                    &current_line,
                    font_size,
                    line_pos,
                ));
                line_y += effective_line_height;
                current_line = word.to_string();
                current_width = word_width;
            } else {
                if !current_line.is_empty() {
                    current_line.push(' ');
                    current_width += space_width;
                }
                current_line.push_str(word);
                current_width += word_width;
            }
        }

        // flush remaining line (including empty paragraphs as blank lines)
        let line_pos = Vec2::new(position.x, position.y + line_y);
        if current_line.is_empty() {
            lines.push(Vec::new());
        } else {
            lines.push(layout_text(
                atlas,
                font_id,
                &current_line,
                font_size,
                line_pos,
            ));
        }
        line_y += effective_line_height;
    }

    lines
}
