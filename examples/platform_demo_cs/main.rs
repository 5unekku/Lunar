//! platform demo — all scene setup and game logic in C#, zero Rust game code.
//!
//! build and run via:
//!   cargo xtask run

use lunar::prelude::*;
use lunar_plugin_loader::CsPlugin;
use std::path::PathBuf;

#[derive(Default)]
struct PlatformDemoCs;

impl GamePlugin for PlatformDemoCs {
    fn name(&self) -> &str { "platform-demo-cs" }

    fn build(&mut self, app: &mut App) {
        app.add_plugin(CsPlugin::new(plugin_path()).with_hot_reload());
    }
}

/// returns the path to the C# plugin .so.
///
/// in single-binary mode (LUNAR_EMBEDDED=1), the .so is embedded as bytes and
/// extracted to a temp directory on first run. otherwise the first CLI argument
/// (or the default `./lunar_scripts.so`) is used.
#[cfg(lunar_embed_plugin)]
fn plugin_path() -> PathBuf {
    static BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded_plugin.bin"));
    let dir = std::env::temp_dir().join("lunar_cs_plugin");
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let path = dir.join(plugin_lib_name());
    std::fs::write(&path, BYTES).expect("failed to extract embedded C# plugin");
    path
}

#[cfg(not(lunar_embed_plugin))]
fn plugin_path() -> PathBuf {
    std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(plugin_lib_name()))
}

fn plugin_lib_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "lunar_scripts.dll",
        "macos"   => "lunar_scripts.dylib",
        _         => "lunar_scripts.so",
    }
}

fn main() {
    lunar::bootstrap_3d::<PlatformDemoCs>(RenderConfig3d {
        title: "Platform Demo (C# scripting)".to_string(),
        width: 1280,
        height: 720,
        vsync: false,
        frame_cap: 0,
        tick_rate: TickRate::Hz60,
        ..Default::default()
    });
}
