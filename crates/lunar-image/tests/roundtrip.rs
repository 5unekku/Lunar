use lunar_image::{EncodeOptions, decode, encode, encode_with_opts};

#[test]
fn roundtrip_basic() {
    let width = 64u32;
    let height = 64u32;
    let pixels: Vec<u8> = (0..(width * height * 4) as usize)
        .map(|i| (i % 256) as u8)
        .collect();

    let bytes = encode(width, height, &pixels).unwrap();
    let image = decode(&bytes).unwrap();

    assert_eq!(image.width, width);
    assert_eq!(image.height, height);
    assert_eq!(image.pixels, pixels);
}

#[test]
fn roundtrip_solid_color() {
    let width = 32u32;
    let height = 32u32;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = 255; // R
        chunk[1] = 128; // G
        chunk[2] = 64; // B
        chunk[3] = 200; // A
    }

    let bytes = encode(width, height, &pixels).unwrap();
    let image = decode(&bytes).unwrap();

    assert_eq!(image.pixels, pixels);
}

#[test]
fn roundtrip_with_metadata() {
    let width = 16u32;
    let height = 16u32;
    let pixels: Vec<u8> = (0..(width * height * 4) as usize)
        .map(|i| i as u8)
        .collect();

    let opts = EncodeOptions {
        metadata: Some(r#"{"source":"test.png"}"#.to_string()),
        ..Default::default()
    };
    let bytes = encode_with_opts(width, height, &pixels, &opts).unwrap();
    let image = decode(&bytes).unwrap();

    assert_eq!(image.pixels, pixels);
}

#[test]
fn roundtrip_1x1() {
    let pixels = [10, 20, 30, 255];
    let bytes = encode(1, 1, &pixels).unwrap();
    let image = decode(&bytes).unwrap();

    assert_eq!(image.width, 1);
    assert_eq!(image.height, 1);
    assert_eq!(image.pixels, pixels);
}

#[test]
fn roundtrip_alpha() {
    let width = 8u32;
    let height = 8u32;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (i, chunk) in pixels.chunks_exact_mut(4).enumerate() {
        chunk[0] = 255;
        chunk[1] = 0;
        chunk[2] = 0;
        chunk[3] = (i % 256) as u8;
    }

    let bytes = encode(width, height, &pixels).unwrap();
    let image = decode(&bytes).unwrap();

    assert_eq!(image.pixels, pixels);
}

#[test]
fn roundtrip_gradient_uses_filter() {
    // a smooth gradient is the delta filter's best case: the encoder should pick the
    // filtered path, and decode must reverse it to the exact original pixels.
    let (width, height) = (128u32, 128u32);
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            pixels[i] = x as u8;
            pixels[i + 1] = y as u8;
            pixels[i + 2] = ((x + y) / 2) as u8;
            pixels[i + 3] = 255;
        }
    }

    let bytes = encode(width, height, &pixels).unwrap();
    let image = decode(&bytes).unwrap();
    assert_eq!(image.pixels, pixels);
    // the gradient compresses far below raw size once filtered
    assert!(bytes.len() < pixels.len() / 50, "gradient did not compress well: {} bytes", bytes.len());
}

#[test]
fn roundtrip_photo_like() {
    // gradient plus deterministic noise — exercises the filter on high-entropy content
    let (width, height) = (96u32, 72u32);
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let mut state = 0x9e37_79b9u32;
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((state >> 27) as u8) & 0x0f;
            pixels[i] = (x as u8).wrapping_add(noise);
            pixels[i + 1] = (y as u8).wrapping_add(noise);
            pixels[i + 2] = ((x + y) / 2) as u8;
            pixels[i + 3] = 255;
        }
    }

    let image = decode(&encode(width, height, &pixels).unwrap()).unwrap();
    assert_eq!(image.pixels, pixels);
}

#[test]
fn error_bad_magic() {
    let bad = b"XXXX\x01\x00\x00\x00\x10\x00\x10\x00";
    let result = decode(bad);
    assert!(result.is_err());
}

#[test]
fn error_truncated_header() {
    let result = decode(b"MRIF");
    assert!(result.is_err());
}

#[test]
fn error_buffer_size_mismatch() {
    let result = encode(2, 2, &[0, 1, 2]); // 2x2 needs 16 bytes, got 3
    assert!(result.is_err());
}

#[test]
fn image_helpers() {
    let img = decode(&encode(4, 2, &[0; 32]).unwrap()).unwrap();
    assert_eq!(img.len(), 8);
    assert!(!img.is_empty());
    assert_eq!(img.byte_len(), 32);
    assert!(img.contains(0, 0));
    assert!(img.contains(3, 1));
    assert!(!img.contains(4, 0));
    assert!(!img.contains(0, 2));
}

#[test]
fn get_set_pixel() {
    let mut img = decode(&encode(2, 2, &[0; 16]).unwrap()).unwrap();
    img.set_pixel(1, 0, [255, 128, 64, 200]);
    let px = img.get_pixel(1, 0);
    assert_eq!(px, [255, 128, 64, 200]);
}
