fn main() {
    println!("cargo:rerun-if-env-changed=LUNAR_CS_PLUGIN_PATH");
    let Ok(path) = std::env::var("LUNAR_CS_PLUGIN_PATH") else { return };
    println!("cargo:rerun-if-changed={path}");
    let out = std::env::var("OUT_DIR").unwrap();
    let dst = std::path::Path::new(&out).join("embedded_plugin.bin");
    std::fs::copy(&path, &dst).expect("failed to embed C# plugin");
    println!("cargo:rustc-cfg=lunar_embed_plugin");
}
