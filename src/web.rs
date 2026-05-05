//! web entry point for WASM target
//!
//! drives the game via WebGPU canvas and requestAnimationFrame.
//! the HTML page must contain `<canvas id="lunar-canvas">`.

use lunar::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// minimal web demo — clears to the engine default color each frame.
/// replace with your own GamePlugin to ship a real WASM game.
#[derive(Default)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct WebDemo;

impl GamePlugin for WebDemo {
    fn name(&self) -> &str {
        "WebDemo"
    }
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    panic!("lunar-web must be compiled for wasm32-unknown-unknown");
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub async fn start() {
    console_error_panic_hook::set_once();
    lunar::bootstrap_wasm::<WebDemo>(RenderConfig::default()).await;
}
