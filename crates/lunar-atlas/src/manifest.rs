//! binary manifest for a texture atlas.
//!
//! the manifest maps region names to pixel coordinates within the atlas texture.
//! UV coordinates are computed at load time from pixel positions.
//!
//! # binary format
//!
//! ```text
//! Header (18 bytes):
//!   - magic: [u8; 4] = b'ATLS'
//!   - version: u16
//!   - atlas_width: u32
//!   - atlas_height: u32
//!   - region_count: u32
//!
//! Regions (repeated region_count times):
//!   - name_length: u16
//!   - name: [u8; name_length]  (UTF-8)
//!   - x, y, w, h: u32 each  (pixel coordinates within atlas)
//! ```

use rustc_hash::FxHashMap as HashMap;
use std::io::{self, Read, Write};

use crate::region::AtlasRegion;

/// magic bytes for the atlas manifest
const MAGIC: [u8; 4] = *b"ATLS";
/// current manifest version
const VERSION: u16 = 1;

/// error type for manifest parsing
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid magic bytes: expected 'ATLS', got {0:?}")]
    InvalidMagic([u8; 4]),

    #[error("unsupported manifest version: {0}")]
    UnsupportedVersion(u16),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("region name is not valid UTF-8: {0}")]
    InvalidName(#[from] std::string::FromUtf8Error),

    #[error("unexpected end of data")]
    Truncated,
}

/// a single region entry in the manifest
#[derive(Debug, Clone)]
pub struct ManifestRegion {
    /// pixel x within atlas
    pub x: u32,
    /// pixel y within atlas
    pub y: u32,
    /// pixel width
    pub w: u32,
    /// pixel height
    pub h: u32,
}

/// a loaded atlas manifest.
///
/// contains the atlas dimensions and a map of region names to pixel coordinates.
#[derive(Debug, Clone)]
pub struct AtlasManifest {
    /// atlas texture width in pixels
    pub atlas_width: u32,
    /// atlas texture height in pixels
    pub atlas_height: u32,
    /// named regions with pixel coordinates
    pub regions: HashMap<String, ManifestRegion>,
}

impl AtlasManifest {
    /// parse a manifest from binary data.
    ///
    /// # Errors
    ///
    /// returns [`ManifestError`] if the data is truncated, has an invalid magic number,
    /// unsupported version, or contains invalid utf-8.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ManifestError> {
        let mut cursor = io::Cursor::new(data);
        Self::read_from(&mut cursor)
    }

    /// read a manifest from a reader.
    ///
    /// # Errors
    ///
    /// returns [`ManifestError`] if the data is truncated, has an invalid magic number,
    /// unsupported version, or contains invalid utf-8.
    ///
    /// # Panics
    ///
    /// panics if internal slice indexing fails (should never happen with valid input).
    pub fn read_from<R: Read>(mut reader: R) -> Result<Self, ManifestError> {
        // read header
        let mut magic = [0u8; 4];
        reader
            .read_exact(&mut magic)
            .map_err(|_| ManifestError::Truncated)?;
        if magic != MAGIC {
            return Err(ManifestError::InvalidMagic(magic));
        }

        let mut version_buf = [0u8; 2];
        reader
            .read_exact(&mut version_buf)
            .map_err(|_| ManifestError::Truncated)?;
        let version = u16::from_le_bytes(version_buf);
        if version != VERSION {
            return Err(ManifestError::UnsupportedVersion(version));
        }

        let mut dims_buf = [0u8; 8];
        reader
            .read_exact(&mut dims_buf)
            .map_err(|_| ManifestError::Truncated)?;
        let atlas_width = u32::from_le_bytes(dims_buf[..4].try_into().unwrap());
        let atlas_height = u32::from_le_bytes(dims_buf[4..8].try_into().unwrap());

        let mut count_buf = [0u8; 4];
        reader
            .read_exact(&mut count_buf)
            .map_err(|_| ManifestError::Truncated)?;
        let region_count = u32::from_le_bytes(count_buf);

        let mut regions = HashMap::with_capacity_and_hasher(region_count as usize, Default::default());
        for _ in 0..region_count {
            let mut name_len_buf = [0u8; 2];
            reader
                .read_exact(&mut name_len_buf)
                .map_err(|_| ManifestError::Truncated)?;
            let name_len = u16::from_le_bytes(name_len_buf) as usize;

            let mut name_bytes = vec![0u8; name_len];
            reader
                .read_exact(&mut name_bytes)
                .map_err(|_| ManifestError::Truncated)?;
            let name = String::from_utf8(name_bytes)?;

            let mut coords_buf = [0u8; 16];
            reader
                .read_exact(&mut coords_buf)
                .map_err(|_| ManifestError::Truncated)?;
            let x = u32::from_le_bytes(coords_buf[0..4].try_into().unwrap());
            let y = u32::from_le_bytes(coords_buf[4..8].try_into().unwrap());
            let w = u32::from_le_bytes(coords_buf[8..12].try_into().unwrap());
            let h = u32::from_le_bytes(coords_buf[12..16].try_into().unwrap());

            regions.insert(name, ManifestRegion { x, y, w, h });
        }

        Ok(Self {
            atlas_width,
            atlas_height,
            regions,
        })
    }

    /// serialize this manifest to binary bytes.
    /// # Panics
    ///
    /// panics if writing to the internal buffer fails (should never happen for `Vec<u8>`).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.write_to(&mut buf).expect("failed to write manifest");
        buf
    }

    /// write this manifest to a writer.
    ///
    /// # Errors
    ///
    /// returns [`io::Error`] if the underlying writer fails.
    pub fn write_to<W: Write>(&self, mut writer: W) -> io::Result<()> {
        writer.write_all(&MAGIC)?;
        writer.write_all(&VERSION.to_le_bytes())?;
        writer.write_all(&self.atlas_width.to_le_bytes())?;
        writer.write_all(&self.atlas_height.to_le_bytes())?;
        let region_count = u32::try_from(self.regions.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "too many regions to serialize"))?;
        writer.write_all(&region_count.to_le_bytes())?;

        for (name, region) in &self.regions {
            let name_bytes = name.as_bytes();
            let name_len = u16::try_from(name_bytes.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "region name too long to serialize"))?;
            writer.write_all(&name_len.to_le_bytes())?;
            writer.write_all(name_bytes)?;
            writer.write_all(&region.x.to_le_bytes())?;
            writer.write_all(&region.y.to_le_bytes())?;
            writer.write_all(&region.w.to_le_bytes())?;
            writer.write_all(&region.h.to_le_bytes())?;
        }

        Ok(())
    }

    /// resolve all regions into [`AtlasRegion`]s with computed UV coordinates.
    #[must_use]
    pub fn resolve_regions(&self) -> HashMap<String, AtlasRegion> {
        self.regions
            .iter()
            .map(|(name, r)| {
                let region = AtlasRegion::from_pixels(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    self.atlas_width,
                    self.atlas_height,
                );
                (name.clone(), region)
            })
            .collect()
    }
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    fn make_manifest() -> AtlasManifest {
        let mut regions = HashMap::default();
        regions.insert(
            "player".into(),
            ManifestRegion {
                x: 0,
                y: 0,
                w: 32,
                h: 32,
            },
        );
        regions.insert(
            "enemy".into(),
            ManifestRegion {
                x: 32,
                y: 0,
                w: 16,
                h: 16,
            },
        );
        AtlasManifest {
            atlas_width: 64,
            atlas_height: 64,
            regions,
        }
    }

    #[test]
    fn roundtrip_binary() {
        let original = make_manifest();
        let bytes = original.to_bytes();
        let decoded = AtlasManifest::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.atlas_width, original.atlas_width);
        assert_eq!(decoded.atlas_height, original.atlas_height);
        assert_eq!(decoded.regions.len(), original.regions.len());
        for (name, region) in &original.regions {
            let got = decoded.regions.get(name).unwrap();
            assert_eq!(got.x, region.x);
            assert_eq!(got.y, region.y);
            assert_eq!(got.w, region.w);
            assert_eq!(got.h, region.h);
        }
    }

    #[test]
    fn empty_manifest_roundtrip() {
        let m = AtlasManifest {
            atlas_width: 1,
            atlas_height: 1,
            regions: HashMap::default(),
        };
        let bytes = m.to_bytes();
        let decoded = AtlasManifest::from_bytes(&bytes).unwrap();
        assert!(decoded.regions.is_empty());
    }

    #[test]
    fn reject_bad_magic() {
        let err = AtlasManifest::from_bytes(b"BADS\x01\x00").unwrap_err();
        assert!(matches!(err, ManifestError::InvalidMagic(_)));
    }

    #[test]
    fn reject_unsupported_version() {
        let data = [b'A', b'T', b'L', b'S', 0xFF, 0xFF];
        let err = AtlasManifest::from_bytes(&data).unwrap_err();
        assert!(matches!(err, ManifestError::UnsupportedVersion(0xFFFF)));
    }

    #[test]
    fn reject_truncated() {
        let err = AtlasManifest::from_bytes(b"ATLS").unwrap_err();
        assert!(matches!(err, ManifestError::Truncated));
    }

    #[test]
    fn resolve_regions_computes_uv() {
        let m = make_manifest();
        let resolved = m.resolve_regions();
        let player = resolved.get("player").unwrap();
        assert_eq!(player.uv_min.x, 0.0);
        assert_eq!(player.uv_max.x, 0.5);
        let enemy = resolved.get("enemy").unwrap();
        assert_eq!(enemy.uv_min.x, 0.5);
    }
}
