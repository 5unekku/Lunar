# Texture Atlas Design

> Date: 2026-04-28
> Status: Design phase

## Overview

A texture atlas packs many small images into one large GPU texture. Sprites reference UV sub-rectangles within the atlas instead of loading individual textures. This reduces draw calls from N (one per texture) to 1 (single atlas texture).

## Key Insight

The `.mi` format does NOT need to change. The atlas is a **separate concept** that sits above the image format:

- The **atlas texture** is just a regular `.mi` file (one big image)
- The **region mapping** (name → UV rect) is stored separately
- At runtime: one `Handle<Texture>` + one `HashMap<String, Rect>` = full atlas

## Architecture

### 1. Authoring Format (JSON5)

Game developers define an atlas in a JSON5 file:

```json5
// assets/sprites.atlas.json5  (authoring only — NOT used at runtime)
{
  "name": "sprites",
  "max_width": 2048,
  "max_height": 2048,
  "regions": [
    { "name": "player_idle", "source": "sprites/player_idle.mi" },
    { "name": "player_run", "source": "sprites/player_run.mi" },
    { "name": "enemy_basic", "source": "sprites/enemy.mi" },
    { "name": "bullet", "source": "sprites/bullet.mi" },
    // ... many more
  ]
}
```

### 2. Build-Time Atlas Compilation

During the asset pipeline (or at load time in dev mode):

1. Decode each source `.mi` file → `Image { width, height, pixels }`
2. Run bin-packing algorithm to place images into atlas canvas
3. Blit all images onto one big RGBA buffer
4. Encode the big buffer as a single `.mi` file → `sprites_atlas.mi`
5. Write the region mapping as a separate binary manifest → `sprites_atlas.manifest`

**Manifest format (binary, read at runtime — no JSON parsing):**
```
Header:
  - magic: [u8; 4] = b'ATLS'
  - version: u16
  - atlas_width: u32   (for UV computation)
  - atlas_height: u32
  - region_count: u32

Regions (repeated region_count times):
  - name_length: u16
  - name: [u8; name_length]  (UTF-8)
  - x, y, w, h: u32 each  (pixel coordinates within atlas)
```

UV coordinates are computed at load time: `uv_min = (x / atlas_width, y / atlas_height)`, `uv_max = ((x+w) / atlas_width, (y+h) / atlas_height)`. No padding needed — UVs are exact.

### 3. Runtime Types

```rust
/// A region within a texture atlas.
#[derive(Debug, Clone)]
pub struct AtlasRegion {
    /// UV coordinates of the top-left corner (0.0 to 1.0)
    pub uv_min: Vec2,
    /// UV coordinates of the bottom-right corner (0.0 to 1.0)
    pub uv_max: Vec2,
}

/// A loaded texture atlas.
#[derive(Resource)]
pub struct TextureAtlas {
    /// The packed atlas texture (one GPU texture)
    pub texture: Handle<Texture>,
    /// Named region lookup
    pub regions: HashMap<String, AtlasRegion>,
}

impl TextureAtlas {
    /// Get a region by name, panics if not found.
    pub fn region(&self, name: &str) -> &AtlasRegion { ... }
    
    /// Get a region by name, returns None if not found.
    pub fn get_region(&self, name: &str) -> Option<&AtlasRegion> { ... }
}
```

### 4. Render Integration

The `DrawKind::Sprite` already has UV coordinates implicitly set to `[0,0,1,1]`. For atlas sprites, we override the UVs:

```rust
// Option A: Add atlas_region field to DrawKind::Sprite
DrawKind::Sprite {
    texture: Some(atlas_texture_id),
    position,
    rotation,
    scale,
    tint,
    layer,
    uv_rect: Some(AtlasRegion { uv_min, uv_max }),  // NEW
}

// Option B: Use existing UV fields (if they exist)
// The render loop already uses uvs in vertex generation:
//   let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], ...];
// For atlas sprites, replace with region UVs.
```

**Current render code** (line 768-775 in lib.rs):
```rust
let uvs = [
    [0.0, 0.0],
    [1.0, 0.0],
    [0.0, 1.0],
    [0.0, 1.0],
    [1.0, 0.0],
    [1.0, 1.0],
];
```

For atlas sprites, these become the region's UV coordinates instead of `[0,0,1,1]`.

### 5. Bin-Packing Algorithm

Use **shelf packing** (simple, fast, good enough for game sprites):

1. Sort images by height (tallest first)
2. Place each image on the current shelf (row)
3. If it doesn't fit, start a new shelf
4. Track the rightmost position on each shelf

Alternative: **MaxRects** (better packing density, more complex). Can upgrade later.

### 6. Asset Server Integration

```rust
impl AssetServer {
    /// Load a texture atlas from a manifest file.
    pub fn load_atlas(&mut self, manifest_path: &str) -> Handle<TextureAtlas>;
    
    /// Check if an atlas is fully loaded.
    pub fn is_atlas_ready(&self, handle: &Handle<TextureAtlas>) -> bool;
}
```

### 7. New Crate or Existing?

**Option A: New `lunar-atlas` crate**
- Clean separation of concerns
- Depends on `lunar-image` (for .mi decode) and `lunar-assets` (for Handle<Texture>)
- Contains: bin-packing, manifest parsing, TextureAtlas resource

**Option B: Add to `lunar-assets`**
- Simpler, fewer crates
- But mixes concerns (asset loading vs atlas-specific logic)

**Recommendation: Option A** — new `lunar-atlas` crate. The atlas system is substantial enough (bin-packing, manifest format, build-time tooling) to warrant its own crate.

### 8. No Texture Bleeding

UV coordinates are computed precisely from pixel boundaries:
```
uv_min_x = x / atlas_width
uv_min_y = y / atlas_height
uv_max_x = (x + w) / atlas_width
uv_max_y = (y + h) / atlas_height
```

With exact UV math, no padding is needed. The GPU samples exactly the right pixels. Padding is a workaround for imprecise UVs — we won't have that problem.

## Dependencies

```toml
# lunar-atlas/Cargo.toml
[dependencies]
lunar-image = { path = "../lunar-image" }
lunar-assets = { path = "../lunar-assets" }
lunar-math = { path = "../lunar-math" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"  # for JSON5 authoring format parsing
```

## Performance Considerations

| Aspect | Impact |
|--------|
| Decode time | Same as any .mi file — one big decode instead of many small ones |
| GPU memory | Slightly more (padding waste) but fewer texture objects = less driver overhead |
| Draw calls | Massive reduction — 1 per atlas instead of N per individual texture |
| Batching | All atlas sprites batch into one draw call |
| Padding | 2px padding between regions prevents texture bleeding (minor memory cost) |

## Texture Bleeding Prevention

When using nearest-neighbor sampling (our current sampler), UV coordinates can sample adjacent pixels at region boundaries. Solutions:

1. **Padding**: 2px transparent border around each region (recommended, simple)
2. **UV clamping**: Shrink UVs slightly inward (complex, error-prone)
3. **Mipmaps**: Not used in our 2D pipeline (nearest-neighbor only)

We'll use **padding** — it's the simplest and most reliable approach.

## Future Extensions

- **Multiple atlases**: Group by usage pattern (characters, tiles, effects) to avoid loading one massive atlas
- **Dynamic atlas**: Add/remove regions at runtime (complex, probably not needed)
- **Atlas streaming**: Load atlas tiles on demand for very large atlases
- **GPU-compressed atlas**: Store BCn/ASTC in the atlas for zero-transcode upload (mentioned in .mi format future extensions)
