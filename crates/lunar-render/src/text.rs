//! text rendering via a glyph atlas texture.
//!
//! provides a glyph atlas for rasterizing and caching font glyphs using
//! cosmic-text, plus layout functions for positioning text on screen.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent};
use lunar_math::Vec2;
use rustc_hash::FxHashMap as HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

/// position + size of a glyph in the atlas, plus placement offsets.
#[derive(Clone)]
struct AtlasEntry {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    left: i32,
    top: i32,
}

/// a single positioned glyph quad for rendering.
#[derive(Debug, Clone, Copy)]
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

/// LRU cache for laid-out text quads, keyed by (font_id, content_hash, font_size_bits, wrap_bits).
///
/// quads are stored with positions relative to origin (0,0). callers add the world position
/// when reading. the cache is capped at `cap` entries; least-recently-used entry is evicted
/// when full. must be cleared when the glyph atlas resets (scale change) since UV coords change.
pub struct TextLayoutCache {
    map: HashMap<[u32; 4], (Vec<TextGlyphQuad>, u64)>,
    lru_gen: u64,
    cap: usize,
}

impl TextLayoutCache {
    pub fn new(cap: usize) -> Self {
        Self { map: HashMap::with_capacity_and_hasher(cap + 1, Default::default()), lru_gen: 0, cap }
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// retrieve cached origin-relative quads if present, updating their LRU generation.
    pub fn get(&mut self, font_id: u32, content: &str, font_size: f32, wrap_width: Option<f32>) -> Option<&Vec<TextGlyphQuad>> {
        let key = cache_key(font_id, content, font_size, wrap_width);
        let generation = self.lru_gen;
        if let Some(entry) = self.map.get_mut(&key) {
            entry.1 = generation;
            Some(&entry.0)
        } else {
            None
        }
    }

    /// insert origin-relative quads into the cache, evicting the LRU entry if at capacity.
    pub fn insert(&mut self, font_id: u32, content: &str, font_size: f32, wrap_width: Option<f32>, quads: Vec<TextGlyphQuad>) {
        let key = cache_key(font_id, content, font_size, wrap_width);
        if self.map.len() >= self.cap && !self.map.contains_key(&key) {
            let lru = self.map.iter()
                .min_by_key(|(_, (_, g))| g)
                .map(|(k, _)| *k);
            if let Some(k) = lru { self.map.remove(&k); }
        }
        self.lru_gen += 1;
        self.map.insert(key, (quads, self.lru_gen));
    }
}

fn cache_key(font_id: u32, content: &str, font_size: f32, wrap_width: Option<f32>) -> [u32; 4] {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let content_hash = hasher.finish();
    let wrap_bits = wrap_width.map_or(0u32, f32::to_bits);
    [font_id, (content_hash >> 32) as u32, content_hash as u32, wrap_bits ^ f32::to_bits(font_size)]
}

/// shelf-packed RGBA glyph atlas backed by cosmic-text.
pub struct GlyphAtlas {
    /// atlas texture width in pixels.
    pub width: u32,
    /// atlas texture height in pixels.
    pub height: u32,
    /// packed RGBA pixel data.
    pub pixels: Vec<u8>,
    font_system: FontSystem,
    swash_cache: SwashCache,
    entries: HashMap<cosmic_text::CacheKey, Option<AtlasEntry>>,
    font_families: HashMap<u32, String>,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    /// physical pixels per game pixel.
    pub scale: f32,
    /// true when the cpu pixel buffer changed since last gpu upload.
    pub dirty: bool,
}

impl GlyphAtlas {
    /// create a new empty atlas.
    pub fn new(width: u32, height: u32) -> Self {
        let font_system = FontSystem::new_with_locale_and_db(
            "en-US".to_string(),
            cosmic_text::fontdb::Database::new(),
        );
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
            font_system,
            swash_cache: SwashCache::new(),
            entries: HashMap::default(),
            font_families: HashMap::default(),
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            scale: 1.0,
            dirty: false,
        }
    }

    /// update the rasterization scale. returns `true` if the scale changed.
    pub fn set_scale(&mut self, scale: f32) -> bool {
        let clamped = scale.clamp(0.25, 8.0);
        let rounded = (clamped * 100.0).round() / 100.0;
        if (self.scale - rounded).abs() > 0.005 {
            self.scale = rounded;
            self.entries.clear();
            self.pixels.fill(0);
            self.swash_cache = SwashCache::new();
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// register a font from raw bytes. no-op if already registered.
    pub fn register_font(&mut self, font_id: u32, data: &[u8]) {
        if self.font_families.contains_key(&font_id) {
            return;
        }
        // collect existing face ids before loading
        let before: rustc_hash::FxHashSet<cosmic_text::fontdb::ID> =
            self.font_system.db().faces().map(|f| f.id).collect();
        self.font_system.db_mut().load_font_data(data.to_vec());
        // find newly added face and grab its first family name
        let family_name = self
            .font_system
            .db()
            .faces()
            .find(|f| !before.contains(&f.id))
            .and_then(|f| f.families.first())
            .map(|(name, _)| name.clone());
        match family_name {
            Some(name) => {
                self.font_families.insert(font_id, name);
            }
            None => log::warn!("failed to detect family name for font {font_id}"),
        }
    }

    /// ensure a glyph is in the atlas. stores `None` for zero-size or failed glyphs.
    fn ensure_glyph(&mut self, cache_key: cosmic_text::CacheKey) {
        if self.entries.contains_key(&cache_key) {
            return;
        }
        // collect image data then drop borrow before writing to self.pixels
        let image_data = self
            .swash_cache
            .get_image(&mut self.font_system, cache_key)
            .as_ref()
            .map(|img| {
                (
                    img.data.to_vec(),
                    img.placement,
                    img.content,
                )
            });
        let Some((data, placement, content)) = image_data else {
            self.entries.insert(cache_key, None);
            return;
        };
        let gw = placement.width;
        let gh = placement.height;
        if gw == 0 || gh == 0 || data.is_empty() {
            self.entries.insert(cache_key, None);
            return;
        }

        // wrap to next shelf row if needed
        if self.cursor_x + gw > self.width {
            self.cursor_x = 0;
            self.cursor_y += self.row_height;
            self.row_height = 0;
        }
        if self.cursor_y + gh > self.height {
            log::warn!("glyph atlas full — glyph dropped. increase atlas dimensions.");
            self.entries.insert(cache_key, None);
            return;
        }

        match content {
            SwashContent::Mask => {
                // 1 byte per pixel (alpha coverage)
                for gy in 0..gh {
                    for gx in 0..gw {
                        let src = data[(gy * gw + gx) as usize];
                        let dst =
                            ((self.cursor_y + gy) * self.width + self.cursor_x + gx) as usize * 4;
                        self.pixels[dst] = 0xff;
                        self.pixels[dst + 1] = 0xff;
                        self.pixels[dst + 2] = 0xff;
                        self.pixels[dst + 3] = src;
                    }
                }
            }
            SwashContent::Color => {
                // 4 bytes per pixel RGBA
                for gy in 0..gh {
                    for gx in 0..gw {
                        let src = ((gy * gw + gx) * 4) as usize;
                        let dst =
                            ((self.cursor_y + gy) * self.width + self.cursor_x + gx) as usize * 4;
                        self.pixels[dst..dst + 4].copy_from_slice(&data[src..src + 4]);
                    }
                }
            }
            SwashContent::SubpixelMask => {
                // treat like Mask, use R channel as alpha
                for gy in 0..gh {
                    for gx in 0..gw {
                        let src = ((gy * gw + gx) * 3) as usize;
                        let alpha = data[src]; // R channel
                        let dst =
                            ((self.cursor_y + gy) * self.width + self.cursor_x + gx) as usize * 4;
                        self.pixels[dst] = 0xff;
                        self.pixels[dst + 1] = 0xff;
                        self.pixels[dst + 2] = 0xff;
                        self.pixels[dst + 3] = alpha;
                    }
                }
            }
        }

        self.entries.insert(
            cache_key,
            Some(AtlasEntry {
                x: self.cursor_x,
                y: self.cursor_y,
                width: gw,
                height: gh,
                left: placement.left,
                top: placement.top,
            }),
        );

        self.cursor_x += gw;
        if gh > self.row_height {
            self.row_height = gh;
        }
        self.dirty = true;
    }

    /// get the raw pixel data for uploading to the GPU.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// look up the font family name for a given font id.
fn family_name_for(atlas: &GlyphAtlas, font_id: u32) -> Option<String> {
    atlas.font_families.get(&font_id).cloned()
}

/// layout a string into an existing vec (cleared first). reuses the vec's allocation.
///
/// `position` is the top-left of the text block in game units.
pub fn layout_text_into(
    atlas: &mut GlyphAtlas,
    font_id: u32,
    text: &str,
    font_size: f32,
    position: Vec2,
    out: &mut Vec<TextGlyphQuad>,
) {
    out.clear();
    let scale = atlas.scale;
    let display_size = font_size * scale;
    let family = family_name_for(atlas, font_id);
    let mut buffer = Buffer::new(
        &mut atlas.font_system,
        Metrics::new(display_size, display_size * 1.2),
    );
    buffer.set_size(&mut atlas.font_system, None, None);
    {
        let attrs = family
            .as_deref()
            .map_or_else(Attrs::new, |n| Attrs::new().family(Family::Name(n)));
        buffer.set_text(&mut atlas.font_system, text, attrs, Shaping::Advanced);
    }
    buffer.shape_until_scroll(&mut atlas.font_system, false);

    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, run.line_y), 1.0);
            atlas.ensure_glyph(physical.cache_key);
            let Some(Some(entry)) = atlas.entries.get(&physical.cache_key) else {
                continue;
            };
            let entry = entry.clone();
            #[allow(clippy::cast_precision_loss)]
            let bx = physical.x as f32 + entry.left as f32;
            #[allow(clippy::cast_precision_loss)]
            let by = physical.y as f32 - entry.top as f32;
            let game_x = position.x + bx / scale;
            let game_y = position.y + by / scale;
            let game_w = entry.width as f32 / scale;
            let game_h = entry.height as f32 / scale;
            #[allow(clippy::cast_precision_loss)]
            let uv_min = Vec2::new(
                entry.x as f32 / atlas.width as f32,
                entry.y as f32 / atlas.height as f32,
            );
            #[allow(clippy::cast_precision_loss)]
            let uv_max = Vec2::new(
                (entry.x + entry.width) as f32 / atlas.width as f32,
                (entry.y + entry.height) as f32 / atlas.height as f32,
            );
            out.push(TextGlyphQuad {
                position: Vec2::new(game_x, game_y),
                size: Vec2::new(game_w, game_h),
                uv_min,
                uv_max,
            });
        }
    }
}


/// measure the maximum line width of a string in game units.
#[allow(dead_code)]
pub fn measure_text(atlas: &mut GlyphAtlas, font_id: u32, text: &str, font_size: f32) -> f32 {
    let scale = atlas.scale;
    let display_size = font_size * scale;
    let family = family_name_for(atlas, font_id);
    let mut buffer = Buffer::new(
        &mut atlas.font_system,
        Metrics::new(display_size, display_size * 1.2),
    );
    buffer.set_size(&mut atlas.font_system, None, None);
    {
        let attrs = family
            .as_deref()
            .map_or_else(Attrs::new, |n| Attrs::new().family(Family::Name(n)));
        buffer.set_text(&mut atlas.font_system, text, attrs, Shaping::Advanced);
    }
    buffer.shape_until_scroll(&mut atlas.font_system, false);
    buffer
        .layout_runs()
        .map(|r| r.line_w)
        .fold(0.0f32, f32::max)
        / scale
}

/// layout wrapped text into an existing flat vec (cleared first). reuses the vec's allocation.
///
/// unlike `layout_text_wrapped`, all runs are flattened into a single `out` vec — use this
/// in the hot path to avoid per-frame inner-vec allocations.
#[allow(clippy::too_many_arguments)]
pub fn layout_text_wrapped_into(
    atlas: &mut GlyphAtlas,
    font_id: u32,
    text: &str,
    font_size: f32,
    position: Vec2,
    max_width: f32,
    line_height: f32,
    out: &mut Vec<TextGlyphQuad>,
) {
    out.clear();
    let effective_line_height = if line_height > 0.0 {
        line_height
    } else {
        font_size * 1.25
    };
    let scale = atlas.scale;
    let display_size = font_size * scale;
    let family = family_name_for(atlas, font_id);
    let mut buffer = Buffer::new(
        &mut atlas.font_system,
        Metrics::new(display_size, effective_line_height * scale),
    );
    buffer.set_size(&mut atlas.font_system, Some(max_width * scale), None);
    {
        let attrs = family
            .as_deref()
            .map_or_else(Attrs::new, |n| Attrs::new().family(Family::Name(n)));
        buffer.set_text(&mut atlas.font_system, text, attrs, Shaping::Advanced);
    }
    buffer.shape_until_scroll(&mut atlas.font_system, false);

    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, run.line_y), 1.0);
            atlas.ensure_glyph(physical.cache_key);
            let Some(Some(entry)) = atlas.entries.get(&physical.cache_key) else {
                continue;
            };
            let entry = entry.clone();
            #[allow(clippy::cast_precision_loss)]
            let bx = physical.x as f32 + entry.left as f32;
            #[allow(clippy::cast_precision_loss)]
            let by = physical.y as f32 - entry.top as f32;
            let game_x = position.x + bx / scale;
            let game_y = position.y + by / scale;
            let game_w = entry.width as f32 / scale;
            let game_h = entry.height as f32 / scale;
            #[allow(clippy::cast_precision_loss)]
            let uv_min = Vec2::new(
                entry.x as f32 / atlas.width as f32,
                entry.y as f32 / atlas.height as f32,
            );
            #[allow(clippy::cast_precision_loss)]
            let uv_max = Vec2::new(
                (entry.x + entry.width) as f32 / atlas.width as f32,
                (entry.y + entry.height) as f32 / atlas.height as f32,
            );
            out.push(TextGlyphQuad {
                position: Vec2::new(game_x, game_y),
                size: Vec2::new(game_w, game_h),
                uv_min,
                uv_max,
            });
        }
    }
}

