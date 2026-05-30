use std::{env, fs, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let src_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");

    let options = naga::back::spv::Options {
        lang_version: (1, 1),
        ..Default::default()
    };

    for entry in glob::glob(src_dir.join("**/*.wgsl").to_str().unwrap()).unwrap() {
        let wgsl_path = entry.unwrap();
        let src = fs::read_to_string(&wgsl_path).unwrap();

        let module = match naga::front::wgsl::parse_str(&src) {
            Ok(m) => m,
            Err(e) => panic!("{}: {e}", wgsl_path.display()),
        };
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}: {e}", wgsl_path.display()));

        let spv = naga::back::spv::write_vec(&module, &info, &options, None)
            .unwrap_or_else(|e| panic!("{}: {e}", wgsl_path.display()));

        let rel = wgsl_path.strip_prefix(&src_dir).unwrap();
        let spv_path = out_dir.join(rel).with_extension("spv");
        fs::create_dir_all(spv_path.parent().unwrap()).unwrap();
        fs::write(&spv_path, bytemuck::cast_slice::<u32, u8>(&spv)).unwrap();

        println!("cargo:rerun-if-changed={}", wgsl_path.display());
    }
}
