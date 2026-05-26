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
