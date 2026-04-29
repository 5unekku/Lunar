//! texture atlas integration for the render system.
#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]
//!
//! provides [`TextureAtlas`] resource that wraps a packed atlas texture
//! with named region lookup. sprites can reference atlas regions via
//! [`DrawKind::Sprite`] with an optional `atlas_region` field.
//!
//! # example
//!
//! ```ignore
//! use engine_render::atlas::TextureAtlas;
//! use engine_atlas::{AtlasManifest, AtlasRegion};
//!
//! // load atlas texture via asset server
//! let atlas_texture = asset_server.load_texture("sprites_atlas.mi");
//! let manifest = AtlasManifest::from_bytes(&manifest_bytes)?;
//!
//! let texture_atlas = TextureAtlas::new(atlas_texture, manifest);
//! let region = texture_atlas.region("player_idle");
//! ```

use engine_assets::Handle;
use engine_assets::Texture;
use engine_atlas::{AtlasManifest, AtlasRegion};

/// a loaded texture atlas with GPU texture handle and region lookup.
pub struct TextureAtlas {
    /// handle to the atlas GPU texture
    pub texture: Handle<Texture>,
    /// manifest describing region layout
    pub manifest: AtlasManifest,
    /// pre-computed UV regions for fast lookup
    regions: std::collections::HashMap<String, AtlasRegion>,
}

impl TextureAtlas {
    /// create a new texture atlas from a loaded texture and manifest.
    #[must_use]
    pub fn new(texture: Handle<Texture>, manifest: AtlasManifest) -> Self {
        let regions = manifest.resolve_regions();
        Self {
            texture,
            manifest,
            regions,
        }
    }

    /// look up a region by name.
    ///
    /// returns the [`AtlasRegion`] with UV coordinates for this sprite.
    /// # Panics
    ///
    /// panics if the region does not exist.
    #[must_use]
    pub fn region(&self, name: &str) -> &AtlasRegion {
        self.regions
            .get(name)
            .unwrap_or_else(|| panic!("atlas region '{name}' not found"))
    }

    /// look up a region by name, returning None if not found.
    #[must_use]
    pub fn try_region(&self, name: &str) -> Option<&AtlasRegion> {
        self.regions.get(name)
    }

    /// get the atlas texture handle.
    #[must_use]
    pub const fn texture_handle(&self) -> &Handle<Texture> {
        &self.texture
    }

    /// get all region names.
    pub fn region_names(&self) -> impl Iterator<Item = &String> {
        self.regions.keys()
    }
}
