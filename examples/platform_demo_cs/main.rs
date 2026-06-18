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
        let plugin = build_cs_plugin();
        app.add_plugin(plugin);
    }
}

/// coreclr mode: xtask passes <host_dll_path> <plugin_dll_path> as CLI args.
#[cfg(feature = "coreclr")]
fn build_cs_plugin() -> CsPlugin {
    let host_dll   = std::env::args().nth(1).map(PathBuf::from)
        .expect("coreclr mode requires: <LunarHost.dll path> <plugin.dll path>");
    let plugin_dll = std::env::args().nth(2).map(PathBuf::from)
        .expect("coreclr mode requires: <LunarHost.dll path> <plugin.dll path>");
    CsPlugin::new(plugin_dll).with_host_dll(host_dll).with_hot_reload()
}

/// nativeaot mode: first CLI arg (or default `.so`) is the compiled plugin.
#[cfg(not(feature = "coreclr"))]
fn build_cs_plugin() -> CsPlugin {
    CsPlugin::new(plugin_path()).with_hot_reload()
}

#[cfg(not(feature = "coreclr"))]
fn plugin_path() -> PathBuf {
    #[cfg(lunar_embed_plugin)]
    {
        static BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded_plugin.bin"));
        let dir = std::env::temp_dir().join("lunar_cs_plugin");
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        let path = dir.join(plugin_lib_name());
        std::fs::write(&path, BYTES).expect("failed to extract embedded C# plugin");
        path
    }
    #[cfg(not(lunar_embed_plugin))]
    {
        std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(plugin_lib_name()))
    }
}

#[cfg(not(feature = "coreclr"))]
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
