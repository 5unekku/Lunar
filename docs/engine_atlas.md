# engine_atlas

texture atlas system for the Lunar game engine.

packs many small images into one large GPU texture, reducing draw calls
from N (one per texture) to 1 (single atlas texture).

# example

```ignore
use engine_atlas::{AtlasManifest, AtlasRegion};

// load manifest via asset server
let manifest = AtlasManifest::load("sprites_atlas.manifest")?;

// look up a region by name
let region = manifest.region("player_idle").unwrap();
// use region.uv_min and region.uv_max for rendering
```

## Re-exports
- AtlasManifest = manifest::AtlasManifest — a loaded atlas manifest.
- ManifestError = manifest::ManifestError — error type for manifest parsing
- AtlasPacker = packer::AtlasPacker — bin-packer for building texture atlases.
- PackedAtlas = packer::PackedAtlas — result of packing an atlas.
- AtlasRegion = region::AtlasRegion — a region within a texture atlas.

## Structs

### AtlasManifest

a loaded atlas manifest.

contains the atlas dimensions and a map of region names to pixel coordinates.

### AtlasPacker

bin-packer for building texture atlases.

uses shelf packing: images are sorted by height and placed
on horizontal shelves.

### AtlasRegion

a region within a texture atlas.

UV coordinates are in 0.0 to 1.0 range relative to the full atlas texture.

### PackedAtlas

result of packing an atlas.

## Enums

### ManifestError

error type for manifest parsing
