mod components;
mod plugin;
mod resources;

use plugin::RpgGame;

fn main() {
    lunar::bootstrap::<RpgGame>(lunar::prelude::RenderConfig {
        vsync: false,
        ..Default::default()
    });
}
