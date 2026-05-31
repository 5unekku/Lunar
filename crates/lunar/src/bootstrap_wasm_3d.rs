/// bootstrap a wasm 3d lunar game via WebGPU and requestAnimationFrame.
///
/// finds `<canvas id="lunar-canvas">` in the page, initializes the 3d WebGPU
/// render engine, and drives the game loop via RAF.
///
/// the 3d renderer runs at `RenderTier::Mid` on WebGPU — compute shaders (GTAO,
/// STAA, particles) are active; GPU-driven culling and HZB are disabled since
/// WebGPU lacks `INDIRECT_EXECUTION`.
///
/// # example
///
/// ```ignore
/// use wasm_bindgen::prelude::*;
///
/// #[wasm_bindgen(start)]
/// pub async fn start() {
///     lunar::bootstrap_wasm_3d::<MyGame>(
///         lunar::lunar_render_3d::RenderConfig3d::default()
///     ).await;
/// }
/// ```
pub async fn bootstrap_wasm_3d<Plugin: lunar_core::GamePlugin + Default + 'static>(
    config: lunar_render_3d::RenderConfig3d,
) {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Debug)
        .unwrap_or_else(|_| log::warn!("logger already initialized"));

    use lunar_3d::Plugin3d;
    use lunar_assets::AssetPlugin;
    use lunar_core::{App, AvailableResolutions, STANDARD_RESOLUTIONS, WindowSettings};
    use lunar_input::{InputPlugin, setup_web_input};
    use lunar_render_3d::{RenderEngine3d, RenderPlugin3d};
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let canvas = RenderEngine3d::find_canvas("lunar-canvas")
        .expect("expected <canvas id=\"lunar-canvas\"> in the HTML");

    setup_web_input(canvas.unchecked_ref());

    let surface = RenderEngine3d::create_canvas_surface(&instance, &canvas)
        .expect("failed to create WebGPU surface from canvas");

    let engine = RenderEngine3d::from_surface(&instance, surface, &config).await;

    let mut app = App::new();

    let mut initial_settings = WindowSettings::new(config.width, config.height, config.vsync);
    initial_settings.target_aspect = config.target_aspect;
    initial_settings.allow_resize  = config.allow_resize;
    app.insert_resource(initial_settings);
    // wasm has no display mode API — use the curated standard list
    app.insert_resource(AvailableResolutions(STANDARD_RESOLUTIONS.to_vec()));
    app.insert_resource(engine);

    app.add_plugin(Plugin3d);
    app.add_plugin(RenderPlugin3d);
    app.add_plugin(InputPlugin);
    app.add_plugin(AssetPlugin);
    app.add_plugin(Plugin::default());

    let mut canvas_w = canvas.width();
    let mut canvas_h = canvas.height();

    let app = Rc::new(RefCell::new(app));
    let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();

    *g.borrow_mut() = Some(Closure::new({
        let app   = app.clone();
        let canvas = canvas.clone();
        move || {
            // ResizeObserver updates canvas.width/height when the CSS size changes;
            // detect that here and notify the renderer + window settings.
            let new_w = canvas.width();
            let new_h = canvas.height();
            if (new_w != canvas_w || new_h != canvas_h) && new_w > 0 && new_h > 0 {
                {
                    let mut app_ref = app.borrow_mut();
                    let world = app_ref.world_mut();
                    if let Some(mut engine) = world.get_resource_mut::<lunar_render_3d::RenderEngine3d>() {
                        engine.resize(new_w, new_h);
                    }
                    if let Some(mut ws) = world.get_resource_mut::<lunar_core::WindowSettings>() {
                        ws.width  = new_w;
                        ws.height = new_h;
                    }
                }
                canvas_w = new_w;
                canvas_h = new_h;
            }

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
