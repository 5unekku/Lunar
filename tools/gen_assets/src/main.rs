use image::{RgbaImage, Rgba};
use std::path::Path;

fn make_sprite(width: u32, height: u32, r: u8, g: u8, b: u8) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba([r, g, b, 255]))
}

fn write_lossless_webp(image: &RgbaImage, path: &Path) {
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(std::fs::File::create(path).unwrap());
    image.write_with_encoder(encoder).unwrap();
}

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = manifest.parent().unwrap().parent().unwrap().join("assets/sprites");
    std::fs::create_dir_all(&out).unwrap();

    write_lossless_webp(&make_sprite(32, 48, 60, 120, 220), &out.join("player.webp")); // blue
    write_lossless_webp(&make_sprite(32, 48, 200, 60, 60), &out.join("npc1.webp"));    // red
    write_lossless_webp(&make_sprite(32, 48, 220, 200, 60), &out.join("npc2.webp"));   // yellow
    write_lossless_webp(&make_sprite(32, 48, 60, 180, 80), &out.join("npc3.webp"));    // green
}
