//! texture atlas system for the Lunar game engine.
//!
//! packs many small images into one large GPU texture, reducing draw calls
//! from N (one per texture) to 1 (single atlas texture).
//!
//! # architecture
//!
//! - [`AtlasManifest`] — binary manifest describing region layout
//! - [`TextureAtlas`] — loaded atlas with GPU texture + region lookup
//! - [`AtlasPacker`] — bin-packing algorithm for building atlases
//!
//! # example
//!
//! ```ignore
//! use engine_atlas::{TextureAtlas, AtlasManifest};
//!
//! // load manifest and atlas texture via asset server
//! let manifest = AtlasManifest::load("sprites_atlas.manifest")?;
//! let atlas = TextureAtlas::new(atlas_texture_handle, manifest);
//!
//! // look up a region by name
//! let region = atlas.region("player_idle");
//! // use region.uv_min and region.uv_max for rendering
//! ```

mod manifest;
mod packer;
mod region;

pub use manifest::{AtlasManifest, ManifestError};
pub use packer::{AtlasPacker, PackedAtlas};
pub use region::AtlasRegion;
