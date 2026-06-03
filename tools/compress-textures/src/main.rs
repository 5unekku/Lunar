// compress-textures: offline BC texture compression tool
//
// usage: compress-textures <assets_dir> [--quality fast|normal|high]
//
// converts source png/jpg assets to .bctex files (BC-compressed, engine-ready).
// format is selected per texture based on naming convention:
//   - _n / _normal / _nrm:  BC5 (two-channel normal map)
//   - rgba (has alpha):      BC7
//   - rgb  (no alpha):       BC1
//
// output .bctex files are written alongside the source in the same directory.
// source files are not modified.

use std::{
	env, fs,
	path::{Path, PathBuf},
};

use image_dds::{ImageFormat, Mipmaps, Quality, SurfaceRgba8};
use rayon::prelude::*;

fn main() {
	let args: Vec<String> = env::args().collect();
	if args.len() < 2 {
		eprintln!("usage: compress-textures <assets_dir> [--quality fast|normal|high]");
		std::process::exit(1);
	}

	let assets_dir = PathBuf::from(&args[1]);
	let quality_str = args
		.windows(2)
		.find(|w| w[0] == "--quality")
		.map(|w| w[1].as_str())
		.unwrap_or("normal");

	let quality = match quality_str {
		"fast" => Quality::Fast,
		"high" => Quality::Slow,
		_ => Quality::Normal,
	};

	let pattern = assets_dir.join("**/*.png").to_str().unwrap().to_owned();
	let paths: Vec<PathBuf> = glob::glob(&pattern)
		.unwrap()
		.filter_map(|e| e.ok())
		.collect();

	let total = paths.len();
	eprintln!("compressing {total} textures ({quality_str} quality)...");

	let results: Vec<(&PathBuf, Result<(), String>)> = paths
		.par_iter()
		.map(|path| (path, compress_one(path, quality)))
		.collect();

	let mut ok = 0;
	for (path, result) in &results {
		match result {
			Ok(()) => ok += 1,
			Err(msg) => eprintln!("error: {} — {msg}", path.display()),
		}
	}
	eprintln!("done: {ok}/{total} textures compressed");
	if ok < total {
		std::process::exit(1);
	}
}

fn compress_one(path: &Path, quality: Quality) -> Result<(), String> {
	let name = path.file_stem().unwrap().to_str().unwrap().to_lowercase();
	let is_normal = name.ends_with("_n")
		|| name.ends_with("_normal")
		|| name.ends_with("_nrm")
		|| name.ends_with("normals");

	let img = image::open(path).map_err(|e| e.to_string())?;
	let rgba = img.to_rgba8();
	let (width, height) = (rgba.width(), rgba.height());

	let format = if is_normal {
		ImageFormat::BC5RgUnorm
	} else {
		let has_alpha = rgba.pixels().any(|p| p[3] < 255);
		if has_alpha {
			ImageFormat::BC7RgbaUnorm
		} else {
			ImageFormat::BC1RgbaUnorm
		}
	};

	let surface = SurfaceRgba8::from_image(&rgba);
	let encoded = surface
		.encode(format, quality, Mipmaps::GeneratedAutomatic)
		.map_err(|e| e.to_string())?;

	let format_byte: u8 = match format {
		ImageFormat::BC1RgbaUnorm | ImageFormat::BC1RgbaUnormSrgb => 1,
		ImageFormat::BC3RgbaUnorm | ImageFormat::BC3RgbaUnormSrgb => 3,
		ImageFormat::BC5RgUnorm | ImageFormat::BC5RgSnorm => 5,
		ImageFormat::BC6hRgbUfloat | ImageFormat::BC6hRgbSfloat => 6,
		ImageFormat::BC7RgbaUnorm | ImageFormat::BC7RgbaUnormSrgb => 7,
		_ => return Err(format!("unsupported format {format:?}")),
	};

	// write .bctex header: magic, version, format, mip_count, width, height
	let mut out: Vec<u8> = Vec::with_capacity(16 + encoded.data.len());
	out.extend_from_slice(b"BCTX");
	out.push(1); // version
	out.push(format_byte);
	out.extend_from_slice(&(encoded.mipmaps as u16).to_le_bytes());
	out.extend_from_slice(&width.to_le_bytes());
	out.extend_from_slice(&height.to_le_bytes());
	// raw BC data (all mip levels concatenated)
	out.extend_from_slice(&encoded.data);

	let out_path = path.with_extension("bctex");
	fs::write(&out_path, &out).map_err(|e| e.to_string())?;
	Ok(())
}
