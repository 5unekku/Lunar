mod game;

fn main() {
	lunar::bootstrap::<game::ShooterGame>(lunar::prelude::RenderConfig {
		vsync: false,
		..Default::default()
	});
}
