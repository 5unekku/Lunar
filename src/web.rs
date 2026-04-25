//! web entry point for WASM target
//!
//! this module provides the WASM-compatible entry point using wasm-bindgen
//! and web-sys for canvas-based rendering.

use wasm_bindgen::prelude::*;

/// initialize the engine for web target
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    log::info!("lunar engine starting (web target)...");

    // web-specific initialization
    // canvas setup, web audio, web input handling
    // the rest of the engine code is identical to native
}

// stub main for non-wasm compilation
fn main() {
    start();
}
