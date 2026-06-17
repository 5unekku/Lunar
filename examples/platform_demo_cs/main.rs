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
        let path = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./lunar_scripts.so"));
        app.add_plugin(CsPlugin::new(path));
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
