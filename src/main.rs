//! native smoke-test entry point.
//!
//! `cargo run` boots the engine with no game logic — opens a window, clears
//! the surface, ticks the loop. Proves the bootstrap path compiles and runs.
//! Real games define their own `GamePlugin` and call `lunar::bootstrap`
//! (or use the `lunar_app!` macro).

use lunar::prelude::*;

#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct EmptyPlugin;

impl GamePlugin for EmptyPlugin {
	fn name(&self) -> &str {
		"EmptyPlugin"
	}
}

fn main() {
	// wasm uses the lunar-web bin; gating here keeps `-p lunar-game` wasm builds green
	#[cfg(not(target_arch = "wasm32"))]
	lunar::bootstrap::<EmptyPlugin>(Default::default());
}
