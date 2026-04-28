//! SIMD utility functions for image processing.
//!
//! provides runtime-detected SIMD acceleration for common image operations.
//! falls back to scalar implementations when SIMD is unavailable.

/// SIMD capability level detected at runtime.
///
/// used to select the optimal implementation for image processing
/// operations. detected automatically via [`SimdLevel::detect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdLevel {
    /// no SIMD support, scalar fallback.
    Scalar,
    /// x86 SSE2 (128-bit vectors).
    Sse2,
    /// x86 AVX2 (256-bit vectors).
    Avx2,
    /// ARM NEON (128-bit vectors).
    Neon,
    /// WebAssembly SIMD128 (128-bit vectors).
    WasmSimd128,
}

impl SimdLevel {
    /// detect the best available SIMD level for the current platform.
    /// checks CPU features on `x86_64`, assumes NEON on aarch64,
    /// and checks for wasm simd128 on WebAssembly.
    #[must_use]
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if std::is_x86_feature_detected!("avx2") {
                return Self::Avx2;
            }
            if std::is_x86_feature_detected!("sse2") {
                return Self::Sse2;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            return SimdLevel::Neon;
        }
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        {
            return SimdLevel::WasmSimd128;
        }
        Self::Scalar
    }
}

/// convert sRGB byte values to linear f32 color values.
///
/// each input byte (0-255) is normalized to 0.0-1.0 and converted
/// using the standard sRGB transfer function. the output array must
/// be exactly 4 times the input length (one f32 per channel per pixel).
///
/// # Panics
/// panics if the output length is not exactly 4 times the input length.
pub fn srgb_to_linear_simd(input: &[u8], output: &mut [f32], _level: SimdLevel) {
    assert_eq!(
        input.len() * 4,
        output.len(),
        "output must be 4x input length (rgba -> 4xf32 per pixel)"
    );
    for (i, &byte) in input.iter().enumerate() {
        let s = f32::from(byte) / 255.0;
        output[i] = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
    }
}

/// premultiply RGB channels by the alpha value in-place.
///
/// converts RGBA pixel data so that RGB values are scaled by alpha.
/// the buffer must be a multiple of 4 bytes (complete RGBA pixels).
///
/// # Panics
/// panics if the rgba buffer length is not a multiple of 4.
pub fn premultiply_alpha_simd(rgba: &mut [u8], _level: SimdLevel) {
    assert_eq!(
        rgba.len() % 4,
        0,
        "rgba buffer must be a multiple of 4 bytes"
    );
    for chunk in rgba.chunks_exact_mut(4) {
        let a = f32::from(chunk[3]) / 255.0;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let r = (f32::from(chunk[0]) * a).clamp(0.0, 255.0) as u8;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let g = (f32::from(chunk[1]) * a).clamp(0.0, 255.0) as u8;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let b = (f32::from(chunk[2]) * a).clamp(0.0, 255.0) as u8;
        chunk[0] = r;
        chunk[1] = g;
        chunk[2] = b;
    }
}

/// convert RGBA pixel data to BGRA order by swapping red and blue channels.
///
/// both input and output buffers must be the same size and a multiple
/// of 4 bytes (complete RGBA pixels).
///
/// # Panics
/// panics if input and output lengths differ, or if input is not a multiple of 4.
pub fn rgba_to_bgra_simd(input: &[u8], output: &mut [u8], _level: SimdLevel) {
    assert_eq!(input.len(), output.len());
    assert_eq!(input.len() % 4, 0);
    for (src, dst) in input.chunks_exact(4).zip(output.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
}
