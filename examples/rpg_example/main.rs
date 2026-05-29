mod components;
mod plugin;
mod resources;

use plugin::RpgGame;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    lunar::bootstrap::<RpgGame>(lunar::prelude::RenderConfig {
        vsync: false,
        ..Default::default()
    });
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub async fn start() {
    lunar::bootstrap_wasm::<RpgGame>(lunar::prelude::RenderConfig {
        vsync: false,
        ..Default::default()
    })
    .await;
}
