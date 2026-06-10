use std::{env, fmt::Write as _, fs, path::PathBuf};

// bake the srgb→linear lookup table at build time. emitted as exact f32 bit
// patterns so the values are reproducible regardless of the host's libm.
fn main() {
	let mut source = String::from(
		"/// srgb byte → linear f32, baked by build.rs (one entry per possible byte).\n\
		 pub(crate) static SRGB_TO_LINEAR_LUT: [f32; 256] = [\n",
	);
	for i in 0..256u32 {
		let s = i as f32 / 255.0;
		let linear = if s <= 0.04045 {
			s / 12.92
		} else {
			((s + 0.055) / 1.055).powf(2.4)
		};
		writeln!(source, "\tf32::from_bits({:#010x}), // {linear:.8}", linear.to_bits()).unwrap();
	}
	source.push_str("];\n");

	let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
	fs::write(out_dir.join("srgb_lut.rs"), source).unwrap();
}
