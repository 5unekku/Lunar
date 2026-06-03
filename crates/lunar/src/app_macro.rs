/// convenience macro to bootstrap a lunar game on native targets.
///
/// generates a `main` that hands off to [`bootstrap`](crate::bootstrap), which
/// initializes SDL3, creates a window, sets up the wgpu render surface, adds all
/// built-in plugins, and runs the game loop. this is purely sugar — calling
/// `lunar::bootstrap::<MyGame>(config)` directly is equivalent.
///
/// fullscreen: F11 or Alt+Enter. game code can rebind F11 via [`ActionMap`](crate::ActionMap).
/// set `target_aspect` in the [`RenderConfig`](crate::RenderConfig) to lock the window aspect ratio.
///
/// # usage
///
/// ```ignore
/// use lunar::lunar_app;
///
/// #[derive(Default)]
/// struct MyGame;
///
/// impl lunar::GamePlugin for MyGame {
///     fn name(&self) -> &str { "MyGame" }
///     fn build(&mut self, app: &mut lunar::App) {
///         app.add_system(my_system);
///     }
/// }
///
/// lunar_app!(MyGame);
/// ```
///
/// # with a custom render config
///
/// ```ignore
/// lunar_app!(MyGame, lunar::RenderConfig { width: 1920, height: 1080, ..Default::default() });
/// ```
#[macro_export]
macro_rules! lunar_app {
	($game_plugin:ty) => {
		$crate::lunar_app!($game_plugin, $crate::RenderConfig::default());
	};
	($game_plugin:ty, $render_config:expr) => {
		fn main() {
			$crate::bootstrap::<$game_plugin>($render_config);
		}
	};
}
