# lunar_image

Lunar Image Format (LIF / `.li`) — a fast, lossless, zstd-compressed
internal image format for the Lunar engine.

# Quick start

```
use lunar_image::{encode, decode};

let pixels: Vec<u8> = (0..16 * 16 * 4).map(|i| i as u8).collect();
let bytes = encode(16, 16, &pixels).unwrap();
let image = decode(&bytes).unwrap();
assert_eq!(image.width, 16);
assert_eq!(image.pixels, pixels);
```

## Re-exports
- Image = decode::Image — a decoded image in RGBA format.
- decode = decode::decode — decode .li format bytes to an RGBA image.
- EncodeOptions = encode::EncodeOptions — options for encoding an image to .li format.
- encode = encode::encode — encode RGBA pixels to .li format bytes using default options.
- encode_with_opts = encode::encode_with_opts — encode RGBA pixels to .li format bytes with custom options.
- DecodeError = error::DecodeError — errors that can occur when decoding a .li image.
- EncodeError = error::EncodeError — errors that can occur when encoding a .li image.
- SimdLevel = simd::SimdLevel — SIMD capability level detected at runtime.
- premultiply_alpha_simd = simd::premultiply_alpha_simd — premultiply RGB channels by the alpha value in-place.
- rgba_to_bgra_simd = simd::rgba_to_bgra_simd — convert RGBA pixel data to BGRA order by swapping red and blue channels.
- srgb_to_linear_simd = simd::srgb_to_linear_simd — convert sRGB byte values to linear f32 color values.

## Structs

### EncodeOptions

options for encoding an image to .li format.

controls compression, alpha handling, and optional metadata.
use [`EncodeOptions::default()`] for sensible defaults, then override
specific fields as needed.

### Image

a decoded image in RGBA format.

contains the image dimensions and raw pixel data (RGBA8 order).
use [`decode`] to load a .li file into this type.

## Enums

### DecodeError

errors that can occur when decoding a .li image.

covers header validation, chunk parsing, and decompression failures.

### EncodeError

errors that can occur when encoding a .li image.

covers dimension validation, pixel buffer validation, and compression failures.

### SimdLevel

SIMD capability level detected at runtime.

used to select the optimal implementation for image processing
operations. detected automatically via [`SimdLevel::detect`].

## Functions

### decode

decode .li format bytes to an RGBA image.

parses the file header and decompresses the pixel data.
returns an error if the file is malformed or incomplete.

# Errors
returns an error if the data is not a valid .li file, if pixel data is missing,
or if decompression fails.

### encode

encode RGBA pixels to .li format bytes using default options.

the pixel buffer must contain exactly `width * height * 4` bytes
in RGBA order. returns the encoded .li file data on success.

# Errors
returns an error if the pixel buffer size does not match `width * height * 4`.

### encode_with_opts

encode RGBA pixels to .li format bytes with custom options.

the pixel buffer must contain exactly `width * height * 4` bytes
in RGBA order. returns the encoded .li file data on success.

# Errors
returns an error if the pixel buffer size does not match `width * height * 4`,
or if zstd compression fails.

### premultiply_alpha_simd

premultiply RGB channels by the alpha value in-place.

converts RGBA pixel data so that RGB values are scaled by alpha.
the buffer must be a multiple of 4 bytes (complete RGBA pixels).

# Panics
panics if the rgba buffer length is not a multiple of 4.

### rgba_to_bgra_simd

convert RGBA pixel data to BGRA order by swapping red and blue channels.

both input and output buffers must be the same size and a multiple
of 4 bytes (complete RGBA pixels).

# Panics
panics if input and output lengths differ, or if input is not a multiple of 4.

### srgb_to_linear_simd

convert sRGB byte values to linear f32 color values.

each input byte (0-255) is normalized to 0.0-1.0 and converted
using the standard sRGB transfer function. the output array must
be exactly 4 times the input length (one f32 per channel per pixel).

# Panics
panics if the output length is not exactly 4 times the input length.
