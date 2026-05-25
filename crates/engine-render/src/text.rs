//! text rendering via a glyph atlas texture.
//!
//! provides a glyph atlas for rasterizing and caching font characters,
//! plus layout functions for positioning text on screen.

use engine_math::Vec2;
use std::collections::HashMap;

/// metrics for a single rasterized glyph in the atlas.
#[derive(Debug, Clone)]
pub struct GlyphInfo {
    /// x pixel offset of the glyph within the atlas texture.
    pub x: u32,
    /// y pixel offset of the glyph within the atlas texture.
    pub y: u32,
    /// glyph bitmap width in pixels (at display resolution).
    pub width: u32,
    /// glyph bitmap height in pixels (at display resolution).
    pub height: u32,
    /// horizontal bearing in game units: offset from cursor to glyph left edge.
    pub bearing_x: f32,
    /// vertical bearing in game units: distance from baseline to top of bitmap
    /// (positive = above baseline). used as: `glyph_y = baseline_y - bearing_y`.
    pub bearing_y: f32,
    /// cursor advance in game units.
    pub advance: f32,
}

/// cache key for a rasterized glyph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    font_id: u32,
    char_code: char,
    /// font size in game pixels (rounded to nearest integer).
    font_size: u16,
}

/// a packed glyph atlas texture.
///
/// stores rasterized glyphs in a single 2D pixel buffer.
/// glyphs are placed sequentially, wrapping to new rows as needed.
///
/// the atlas rasterizes glyphs at `font_size * scale` for sharp rendering
/// when the game viewport is upscaled (e.g. 640×480 in a 1280×720 window).
/// all [`GlyphInfo`] metrics are stored in game units (divided by scale) so
/// callers compute layout in game space without knowing the physical scale.
///
/// call [`set_scale`] once per frame before rasterizing new glyphs.
/// if the scale changes, all cached entries are cleared and re-rasterized
/// on demand.
#[derive(Debug)]
#[allow(dead_code)]
pub struct GlyphAtlas {
    /// atlas texture width in pixels.
    pub width: u32,
    /// atlas texture height in pixels.
    pub height: u32,
    /// packed RGBA pixel data.
    pub pixels: Vec<u8>,
    /// cached glyphs keyed by (font_id, char, game font_size).
    pub glyphs: HashMap<GlyphKey, GlyphInfo>,
    /// parsed fonts keyed by font_id.
    font_cache: HashMap<u32, fontdue::Font>,
    /// current x position for the next glyph insertion.
    cursor_x: u32,
    /// current y position for the next glyph insertion.
    cursor_y: u32,
    /// height of the current row in the atlas.
    row_height: u32,
    /// physical pixels per game pixel. set via [`set_scale`].
    pub scale: f32,
}

impl GlyphAtlas {
    /// create a new empty atlas. `scale` is normally 1.0; use [`set_scale`] to update it.
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
            scale: 1.0,
        }
    }

    /// update the rasterization scale. returns `true` if the scale changed
    /// (caller should clear text caches and re-upload the atlas texture).
    ///
    /// glyphs are rasterized at `font_size * scale` for crisp rendering
    /// when the viewport is upscaled. all cached entries are cleared when
    /// the scale changes because bitmaps at the old scale are stale.
    pub fn set_scale(&mut self, scale: f32) -> bool {
        let clamped = scale.clamp(0.25, 8.0);
        // round to 2dp to avoid constant invalidation from float noise
        let rounded = (clamped * 100.0).round() / 100.0;
        if (self.scale - rounded).abs() > 0.005 {
            self.scale = rounded;
            self.glyphs.clear();
            self.pixels.fill(0);
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            true
        } else {
            false
        }
    }

    /// register a font from raw bytes. no-op if already registered.
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

    /// rasterize a glyph at the current scale and insert it into the atlas.
    ///
    /// returns `false` if the atlas is full, the font is not registered, or
    /// the glyph cannot be rasterized. no-op (returns `true`) if already cached.
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

        // rasterize at display resolution for crisp output under viewport scaling
        let display_size = font_size * self.scale;
        let (metrics, bitmap) = match self.font_cache.get(&font_id) {
            Some(font) => font.rasterize(char_code, display_size),
            None => return false,
        };

        let gw = u32::try_from(metrics.width).unwrap_or(0);
        let gh = u32::try_from(metrics.height).unwrap_or(0);

        // convert display-resolution metrics back to game units
        let scale = self.scale;
        #[allow(clippy::cast_precision_loss)]
        let bearing_y = (metrics.ymin + metrics.height as i32) as f32 / scale;
        #[allow(clippy::cast_precision_loss)]
        let bearing_x = metrics.xmin as f32 / scale;
        let advance = metrics.advance_width / scale;

        if gw == 0 || gh == 0 {
            self.glyphs.insert(
                key,
                GlyphInfo {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    bearing_x,
                    bearing_y,
                    advance,
                },
            );
            return true;
        }

        // wrap to next row if needed
        if self.cursor_x + gw > self.width {
            self.cursor_x = 0;
            self.cursor_y += self.row_height;
            self.row_height = 0;
        }

        if self.cursor_y + gh > self.height {
            log::warn!(
                "glyph atlas full — '{char_code}' at {font_size}px dropped. \
                 increase atlas dimensions to fit more glyphs."
            );
            return false;
        }

        for gy in 0..gh {
            let src_row_start = (gy * gw) as usize;
            let src_row = &bitmap[src_row_start..src_row_start + gw as usize];
            let dst_y = self.cursor_y + gy;
            let dst_x = self.cursor_x;
            let dst_row_start = ((dst_y * self.width + dst_x) * 4) as usize;
            for (gx, &alpha) in src_row.iter().enumerate() {
                let dst_idx = dst_row_start + gx * 4;
                self.pixels[dst_idx] = 0xff;
                self.pixels[dst_idx + 1] = 0xff;
                self.pixels[dst_idx + 2] = 0xff;
                self.pixels[dst_idx + 3] = alpha;
            }
        }

        self.glyphs.insert(
            key,
            GlyphInfo {
                x: self.cursor_x,
                y: self.cursor_y,
                width: gw,
                height: gh,
                bearing_x,
                bearing_y,
                advance,
            },
        );

        self.cursor_x += gw;
        if gh > self.row_height {
            self.row_height = gh;
        }

        true
    }

    /// look up a cached glyph. returns `None` if not yet rasterized.
    pub fn get_glyph(&self, font_id: u32, char_code: char, font_size: f32) -> Option<&GlyphInfo> {
        let key = GlyphKey {
            font_id,
            char_code,
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            font_size: font_size.round().max(0.0).min(f32::from(u16::MAX)) as u16,
        };
        self.glyphs.get(&key)
    }

    /// kern advance between two adjacent characters at the given game size.
    /// returns 0.0 if the font has no kern pair for these characters.
    pub fn kern(&self, font_id: u32, left: char, right: char, font_size: f32) -> f32 {
        self.font_cache
            .get(&font_id)
            .and_then(|font| font.horizontal_kern(left, right, font_size * self.scale))
            .map(|k| k / self.scale)
            .unwrap_or(0.0)
    }

    /// get the raw pixel data for uploading to the GPU.
    #[allow(dead_code)]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// layout a string of text and return positioned glyph quads.
///
/// `position` is the **top-left corner** of the text line — the baseline is
/// computed internally from the tallest ascender in the string, so callers
/// never need to think about baselines. kern pairs are applied automatically.
///
/// quad sizes and positions are in game units regardless of the atlas scale.
pub fn layout_text(
    atlas: &GlyphAtlas,
    font_id: u32,
    text: &str,
    font_size: f32,
    position: Vec2,
) -> Vec<TextGlyphQuad> {
    // find the tallest ascender so position.y is the top of the line, not the baseline
    let ascender = text
        .chars()
        .filter_map(|ch| atlas.get_glyph(font_id, ch, font_size))
        .filter(|info| info.width > 0 && info.height > 0)
        .map(|info| info.bearing_y)
        .fold(0.0f32, f32::max);

    let baseline_y = position.y + ascender;
    let mut cursor_x = position.x;
    let mut prev_char: Option<char> = None;
    let mut quads = Vec::new();

    for ch in text.chars() {
        if let Some(prev) = prev_char {
            cursor_x += atlas.kern(font_id, prev, ch, font_size);
        }

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

                // quad size in game units — bitmap is at display res, divide by scale
                let game_w = info.width as f32 / atlas.scale;
                let game_h = info.height as f32 / atlas.scale;

                quads.push(TextGlyphQuad {
                    position: Vec2::new(glyph_x, glyph_y),
                    size: Vec2::new(game_w, game_h),
                    uv_min: Vec2::new(uv_x, uv_y),
                    uv_max: Vec2::new(uv_x + uv_w, uv_y + uv_h),
                });
            }
            cursor_x += info.advance;
        }

        prev_char = Some(ch);
    }

    quads
}

/// a single positioned glyph quad for rendering.
#[derive(Debug, Clone)]
pub struct TextGlyphQuad {
    /// top-left position in game-unit screen space.
    pub position: Vec2,
    /// size in game units.
    pub size: Vec2,
    /// minimum UV coordinate in the atlas (top-left of glyph).
    pub uv_min: Vec2,
    /// maximum UV coordinate in the atlas (bottom-right of glyph).
    pub uv_max: Vec2,
}

/// measure the total advance width of a text string in game units.
/// includes kern adjustments between adjacent characters.
#[allow(dead_code)]
pub fn measure_text(atlas: &GlyphAtlas, font_id: u32, text: &str, font_size: f32) -> f32 {
    let mut width = 0.0f32;
    let mut prev_char: Option<char> = None;
    for ch in text.chars() {
        if let Some(prev) = prev_char {
            width += atlas.kern(font_id, prev, ch, font_size);
        }
        if let Some(info) = atlas.get_glyph(font_id, ch, font_size) {
            width += info.advance;
        }
        prev_char = Some(ch);
    }
    width
}

/// layout text with word-wrapping at `max_width` game units.
///
/// `position` is the top-left of the first line. `line_height` is the vertical
/// spacing per line; pass 0.0 for `font_size * 1.25`.
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

    // cache space width once per call
    let space_width = measure_text(atlas, font_id, " ", font_size);

    for paragraph in text.split('\n') {
        let words: Vec<&str> = paragraph.split_whitespace().collect();
        let mut current_line = String::new();
        let mut current_width = 0.0f32;

        for word in words {
            let word_width = measure_text(atlas, font_id, word, font_size);
            let gap = if current_line.is_empty() {
                0.0
            } else {
                space_width
            };

            if !current_line.is_empty() && current_width + gap + word_width > max_width {
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
                    current_width += gap;
                }
                current_line.push_str(word);
                current_width += word_width;
            }
        }

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
