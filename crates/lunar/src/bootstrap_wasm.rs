//! WASM bootstrap — WebGPU canvas init and requestAnimationFrame game loop

/// bootstrap a WASM lunar game via WebGPU and requestAnimationFrame.
///
/// finds a `<canvas id="lunar-canvas">` in the page, initializes the WebGPU
/// render engine, and drives the game loop via RAF. call this from the
/// `#[wasm_bindgen(start)]` entry point.
///
/// # panics
///
/// panics if no canvas with id `"lunar-canvas"` exists in the DOM, or if the
/// browser does not support WebGPU.
///
/// # example
///
/// ```ignore
/// use lunar::prelude::*;
/// use wasm_bindgen::prelude::*;
///
/// #[wasm_bindgen(start)]
/// pub async fn start() {
///     lunar::bootstrap_wasm::<MyGame>(RenderConfig::default()).await;
/// }
/// ```
pub async fn bootstrap_wasm<Plugin: lunar_core::GamePlugin + Default + 'static>(
    config: lunar_render::RenderConfig,
) {
    console_log::init_with_level(log::Level::Debug)
        .unwrap_or_else(|_| log::warn!("logger already initialized"));

    use lunar_assets::AssetPlugin;
    use lunar_core::{App, AvailableResolutions, STANDARD_RESOLUTIONS, WindowSettings};
    use lunar_input::InputPlugin;
    use lunar_render::{RenderEngine, RenderPlugin, wasm_set_render_engine};
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let canvas = RenderEngine::find_canvas("lunar-canvas")
        .expect("expected <canvas id=\"lunar-canvas\"> in the HTML");

    let surface = RenderEngine::create_canvas_surface(&instance, &canvas)
        .expect("failed to create WebGPU surface from canvas");

    let engine = RenderEngine::from_surface(&instance, surface, config.clone()).await;
    wasm_set_render_engine(engine);

    let mut app = App::new();
    let mut initial_settings = WindowSettings::new(config.width, config.height, config.vsync);
    initial_settings.target_aspect = config.target_aspect;
    initial_settings.allow_resize  = config.allow_resize;
    app.insert_resource(initial_settings);
    // wasm has no display mode API — use the curated standard list
    app.insert_resource(AvailableResolutions(STANDARD_RESOLUTIONS.to_vec()));
    app.add_plugin(RenderPlugin);
    app.add_plugin(InputPlugin);
    app.add_plugin(AssetPlugin);
    app.add_plugin(Plugin::default());

    // app.tick() handles startup on first call, so the RAF closure is uniform
    let app = Rc::new(RefCell::new(app));
    let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();

    *g.borrow_mut() = Some(Closure::new({
        let app = app.clone();
        move || {
            app.borrow_mut().tick(config.tick_rate.delta_seconds());
            web_sys::window()
                .unwrap()
                .request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref())
                .unwrap();
        }
    }));

    web_sys::window()
        .unwrap()
        .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())
        .unwrap();
}
