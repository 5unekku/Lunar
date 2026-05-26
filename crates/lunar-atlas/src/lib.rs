//! texture atlas system for the Lunar game engine.
//!
//! packs many small images into one large GPU texture, reducing draw calls
//! from N (one per texture) to 1 (single atlas texture).
//!
//! # architecture
//!
//! - [`AtlasManifest`] — binary manifest describing region layout
//! - [`AtlasRegion`] — UV coordinate sub-rect within an atlas texture
//! - [`AtlasPacker`] — bin-packing algorithm for building atlases
//!
//! # example
//!
//! ```ignore
//! use lunar_atlas::{AtlasManifest, AtlasRegion};
//!
//! // load manifest via asset server
//! let manifest = AtlasManifest::load("sprites_atlas.manifest")?;
//!
//! // look up a region by name
//! let region = manifest.region("player_idle").unwrap();
//! // use region.uv_min and region.uv_max for rendering
//! ```

mod manifest;
mod packer;
mod region;

pub use manifest::{AtlasManifest, ManifestError};
pub use packer::{AtlasPacker, PackedAtlas};
pub use region::AtlasRegion;
