# Lunar Image Format (`.mi` / MRIF - Lunar Image Format)

## Purpose

Internal library crate for the Lunar game engine. Replaces raw PNG/WebP/BMP files in compiled asset bundles. Not a general-purpose image format — designed specifically for fast decode and direct GPU upload in a game engine context.

Think of it like a `.so` or shared library: you link it in, call `encode()` and `decode()`, and get back raw RGBA pixels.

## Design Goals

- **Lossless**: pixel-perfect roundtrip from source RGBA data
- **High compression**: zstd for excellent ratio/speed tradeoff
- **SIMD-friendly decode**: output is always 32bpp RGBA (4 bytes/pixel), aligned for vectorized writes
- **Cross-platform**: no endianness issues, works on WASM, native, mobile
- **Fast decode**: minimal allocations, streaming-friendly chunk layout
- **Alpha support**: native RGBA, not RGB with separate alpha plane
- **Simple API**: `encode(pixels) -> bytes`, `decode(bytes) -> Image`

## Binary Format Specification

### File Layout

```
+------------------+
|     Header       |  16 bytes fixed
+------------------+
|    Chunk 0       |  (pixel data, required, zstd compressed)
+------------------+
|    Chunk 1       |  (optional: metadata)
+------------------+
|    Chunk 2       |  (optional: ICC profile)
+------------------+
```

### Header (16 bytes)

All multi-byte fields are **little-endian**.

```
Offset  Size  Field           Type    Description
------  ----  --------------  ----------------------------------
0       4     Magic           [u8;4]  b'MRIF' = 0x4D, 0x52, 0x49, 0x46
4       2     Version         u16     Format version (currently 1)
6       2     Flags           u16     Bit flags (see below)
8       4     Width           u32     Image width in pixels
12      4     Height          u32     Image height in pixels
```

**Flags** (u16 LE):
```
bit 0   has_alpha       - Image contains alpha channel
bit 1   has_metadata    - Metadata chunk present
bit 2   has_icc         - ICC profile chunk present
bit 3   premultiplied   - Alpha is premultiplied
bits 4-15  reserved     - Must be zero
```

### Chunk Layout

Each chunk has a 16-byte header followed by compressed data.

```
Offset  Size  Field             Type    Description
------  ----  ----------------  ----------------------------------
0       1     Chunk Type        u8      0x00=PixelData, 0x01=Metadata, 0x02=ICC
1       3     Reserved          [u8;3]  Zero padding (alignment)
4       4     Uncompressed Size u32     Size of data after decompression
8       4     Compressed Size   u32     Size of compressed data in bytes
12      4     Zstd Dict ID      u32     Dictionary ID (0 = no dictionary)
16      N     Compressed Data   [u8;N]  Zstd frame (N = Compressed Size)
```

### Pixel Data Chunk (Type 0x00)

**Required.** Exactly one per file.

- **Uncompressed size**: `width * height * 4` bytes
- **Layout**: row-major, top-to-bottom, left-to-right
- **Pixel format**: R, G, B, A (one byte each, 0-255)
- **Compression**: zstd, level 3 (encode), level 0 (decode)
- **No row padding**: rows are contiguous (no stride padding)

The decoder decompresses directly into a `Vec<u8>` of exactly `width * height * 4` bytes. This buffer can be uploaded to a GPU texture without transformation.

### Metadata Chunk (Type 0x01)

**Optional.** Present if `flags & 0x02 != 0`.

- **Uncompressed**: UTF-8 JSON string
- **Content**: arbitrary key-value pairs
- **Example**: `{"source":"player.png","format":"png","created":"2024-01-01T00:00:00Z"}`

### ICC Profile Chunk (Type 0x02)

**Optional.** Present if `flags & 0x04 != 0`.

- **Uncompressed**: raw ICC v4 profile bytes
- **Use**: color-managed rendering pipelines

## Compression Details

### Zstd Configuration

| Parameter | Value | Reason |
|-----------|-------|--------|
| Encode level | 3 | Good balance of speed and ratio |
| Decode level | 0 (default) | Fastest possible decode |
| Checksum | disabled | Skip frame checksum for speed |
| Frame count | 1 | Single frame per chunk |
| Dictionary | none (ID = 0) | No shared dictionary (simpler) |
| Window log | 17 (128KB) | Limits memory during decode |
| Content size | written | Decoder knows exact output size |

### Why Zstd

| Format | Compression Ratio | Decode Speed | SIMD | Streaming | WASM |
|--------|------------------|--------------|------|-----------|------|
| zstd | Excellent | Very Fast | Yes (internal) | Yes | Yes |
| lz4 | Good | Fastest | Yes | Yes | Yes |
| deflate | Good | Slow | No | Yes | Yes |
| qoi | N/A (repack) | Fastest | Yes | No | Yes |

Zstd gives the best compression while maintaining very fast decode. The `zstd` crate has a pure Rust fallback and optional C bindings. For WASM, the pure Rust path works fine.

## SIMD Optimization Strategy

### Where SIMD Helps

1. **Zstd internal decompression**: zstd already uses SIMD internally on supported platforms. No work needed.

2. **Post-decode processing** (optional, in the library):
   - sRGB to linear color conversion
   - Premultiplied alpha conversion
   - RGBA to BGRA byte swap (for APIs that need it)
   - Mipmap generation (downsample with SIMD)

### SIMD Detection

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdLevel {
    Scalar,     // Fallback, always works
    Sse2,       // x86_64 baseline
    Avx2,       // x86_64 wide vectors
    Neon,       // ARM64 (Apple Silicon, mobile)
    WasmSimd128, // WebAssembly
}

impl SimdLevel {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if std::is_x86_feature_detected!("avx2") {
                return SimdLevel::Avx2;
            }
            if std::is_x86_feature_detected!("sse2") {
                return SimdLevel::Sse2;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            return SimdLevel::Neon; // Always available on ARM64
        }
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        {
            return SimdLevel::WasmSimd128;
        }
        SimdLevel::Scalar
    }
}
```

### SIMD Post-Processing Functions

```rust
// sRGB to linear conversion (per-channel)
pub fn srgb_to_linear_simd(input: &[u8], output: &mut [f32], level: SimdLevel);

// Premultiply alpha
pub fn premultiply_alpha_simd(rgba: &mut [u8], level: SimdLevel);

// RGBA to BGRA byte swap
pub fn rgba_to_bgra_simd(input: &[u8], output: &mut [u8], level: SimdLevel);
```

These are optional utilities. The core `decode()` function returns raw RGBA and does no post-processing.

## Memory Model

### Decode Memory Usage

For an image of `W x H` pixels:

| Buffer | Size | Notes |
|--------|------|-------|
| Input slice | file size | Borrowed, no allocation |
| Zstd decompression buffer | compressed size | Temporary, freed after decode |
| Output pixel buffer | `W * H * 4` bytes | Returned to caller |
| **Peak memory** | `file_size + W * H * 4` | Roughly 2x raw pixel size |

### Encode Memory Usage

| Buffer | Size | Notes |
|--------|------|-------|
| Input pixels | `W * H * 4` bytes | Borrowed from caller |
| Zstd output buffer | ~`W * H * 4` bytes (overallocated) | Grows as needed |
| **Peak memory** | `~2 * W * H * 4` | Roughly 2x raw pixel size |

### Zero-Copy Considerations

The decode function returns an owned `Vec<u8>`. This is intentional:
- The pixel buffer needs to be mutable (for post-processing)
- GPU upload typically requires owned or mapped memory
- Zero-copy would require lifetime management that complicates the API

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("file too short for header")]
    TruncatedHeader,

    #[error("invalid magic bytes: expected 'MRIF', got {0:?}")]
    InvalidMagic([u8; 4]),

    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u16),

    #[error("unknown flag bits set: {0:#06x}")]
    UnknownFlags(u16),

    #[error("missing required pixel data chunk")]
    MissingPixelData,

    #[error("multiple pixel data chunks")]
    MultiplePixelData,

    #[error("pixel data size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: usize, actual: usize },

    #[error("zstd decompression failed: {0}")]
    ZstdError(#[from] zstd::Error),

    #[error("unexpected end of file")]
    TruncatedChunk,
}

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("image dimensions too large: {width}x{height}")]
    DimensionsTooLarge { width: u32, height: u32 },

    #[error("pixel buffer size mismatch: expected {expected}, got {actual}")]
    BufferSizeMismatch { expected: usize, actual: usize },

    #[error("zstd compression failed: {0}")]
    ZstdError(#[from] zstd::Error),
}
```

## Crate Structure

```
crates/engine-image/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Public API, re-exports
│   ├── format.rs       # Constants, header parsing, chunk types
│   ├── encode.rs       # Encoder: RGBA -> .mi bytes
│   ├── decode.rs       # Decoder: .mi bytes -> RGBA
│   ├── simd.rs         # SIMD detection + post-processing
│   └── error.rs        # Error types
└── tests/
    └── roundtrip.rs    # Encode -> decode -> compare tests
```

## Public API

```rust
use engine_image::{encode, decode, Image, EncodeOptions, DecodeError, EncodeError};

/// A decoded image in RGBA format.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,  // RGBA, 4 bytes per pixel, row-major
}

impl Image {
    /// Create a new empty image.
    pub fn new(width: u32, height: u32) -> Self;

    /// Total number of pixels.
    pub fn len(&self) -> usize;

    /// Whether the image is empty.
    pub fn is_empty(&self) -> bool;

    /// Byte size of the pixel buffer.
    pub fn byte_len(&self) -> usize;

    /// Check if a pixel coordinate is in bounds.
    pub fn contains(&self, x: u32, y: u32) -> bool;

    /// Get the RGBA value at a pixel coordinate.
    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4];

    /// Set the RGBA value at a pixel coordinate.
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]);
}

/// Encode RGBA pixels to .mi format bytes.
///
/// # Arguments
/// * `width` - Image width in pixels
/// * `height` - Image height in pixels
/// * `rgba` - Pixel data, 4 bytes per pixel (R, G, B, A), row-major order
///
/// # Returns
/// Encoded .mi file bytes
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, EncodeError>;

/// Encode with options.
pub fn encode_with_opts(
    width: u32,
    height: u32,
    rgba: &[u8],
    opts: EncodeOptions,
) -> Result<Vec<u8>, EncodeError>;

/// Decode .mi format bytes to an RGBA image.
///
/// # Arguments
/// * `data` - .mi file bytes
///
/// # Returns
/// Decoded image with raw RGBA pixels
pub fn decode(data: &[u8]) -> Result<Image, DecodeError>;

/// Encode options.
pub struct EncodeOptions {
    /// Zstd compression level (1-22, default 3).
    /// Higher = better compression but slower encode.
    /// Level 0 = no compression.
    pub compression_level: i32,

    /// Whether the image has alpha (default true).
    pub has_alpha: bool,

    /// Whether alpha is premultiplied (default false).
    pub premultiplied: bool,

    /// Optional metadata to embed in the file.
    pub metadata: Option<String>,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            compression_level: 3,
            has_alpha: true,
            premultiplied: false,
            metadata: None,
        }
    }
}
```

## Dependencies

```toml
[package]
name = "engine-image"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
zstd = { version = "0.13", default-features = false }
thiserror = "2"

[dev-dependencies]
image = "0.25"  # For test roundtrip with real files
```

**Note on zstd**: We use `default-features = false` to avoid pulling in the C bindings. The pure Rust `zstd-safe` + `zstd-sys` fallback is sufficient and works on WASM. If performance profiling shows the C bindings are needed on native targets, we can add a feature flag.

## WASM Compatibility

- **zstd**: Pure Rust path works on WASM
- **SIMD**: WASM SIMD128 extension is optional; the library falls back to scalar
- **No threading**: Decode is single-threaded (no `Send`/`Sync` requirements)
- **No filesystem**: All I/O is via borrowed `&[u8]` slices

## Integration with engine-assets

```rust
// In engine-assets, register the .mi loader
let mut loaders = AssetLoaders::new();
loaders.register("mi", EixLoader);

// MiLoader implementation
pub struct MiLoader;

impl AssetLoader for MiLoader {
    type Output = TextureData;

    fn load(&self, bytes: &[u8]) -> Result<TextureData, Box<dyn Error>> {
        let image = engine_image::decode(bytes)?;
        Ok(TextureData {
            width: image.width,
            height: image.height,
            format: TextureFormat::Rgba8Unorm,
            data: image.pixels,
        })
    }
}
```

## Performance Targets

| Operation | Target | Notes |
|-----------|--------|
| Encode 1024x1024 RGBA | < 100ms | zstd level 3 |
| Encode 4096x4096 RGBA | < 1000ms | zstd level 3 |
| Decode 1024x1024 RGBA | < 10ms | zstd level 0 decode |
| Decode 4096x4096 RGBA | < 100ms | zstd level 0 decode |
| Compression ratio | 40-70% of raw | Depends on image content |
| Memory during decode | ~2x raw size | compressed buffer + output buffer |

## Future Extensions

- **Tile-based encoding**: split large images into tiles for parallel decode
- **Mipmap chains**: store pre-computed mip levels in additional chunks
- **GPU-compressed formats**: store BCn/ASTC directly for zero-transcode upload
- **Streaming decode**: decode rows/tiles on demand for very large images
- **Shared dictionary**: pre-trained zstd dictionary for even better compression on similar images
