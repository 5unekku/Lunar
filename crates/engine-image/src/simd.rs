/// SIMD capability level detected at runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdLevel {
    Scalar,
    Sse2,
    Avx2,
    Neon,
    WasmSimd128,
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
            return SimdLevel::Neon;
        }
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        {
            return SimdLevel::WasmSimd128;
        }
        SimdLevel::Scalar
    }
}

/// Convert sRGB bytes to linear f32 values.
pub fn srgb_to_linear_simd(input: &[u8], output: &mut [f32], _level: SimdLevel) {
    assert_eq!(
        input.len() * 4,
        output.len(),
        "output must be 4x input length (rgba -> 4xf32 per pixel)"
    );
    for (i, &byte) in input.iter().enumerate() {
        let s = byte as f32 / 255.0;
        output[i] = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
    }
}

/// Premultiply alpha in-place.
pub fn premultiply_alpha_simd(rgba: &mut [u8], _level: SimdLevel) {
    assert_eq!(
        rgba.len() % 4,
        0,
        "rgba buffer must be a multiple of 4 bytes"
    );
    for chunk in rgba.chunks_exact_mut(4) {
        let a = chunk[3] as f32 / 255.0;
        chunk[0] = (chunk[0] as f32 * a) as u8;
        chunk[1] = (chunk[1] as f32 * a) as u8;
        chunk[2] = (chunk[2] as f32 * a) as u8;
    }
}

/// Swap RGBA to BGRA in-place (byte swap per pixel).
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
