//! sprite-storm: 20k drifting sprites plus 200 text labels, rendered through the
//! real 2d ecs path (auto_sprite_system / auto_text_system) into an offscreen
//! target. stresses 2d batching, the glyph atlas, and per-frame transform +
//! draw-command churn.

use lunar::lunar_math::Vec2;
use lunar::prelude::*;

use crate::common::{checker_texture, Rng};

const SPRITES: usize = 20_000;
const LABELS: usize = 200;
const FIELD_X: f32 = 640.0;
const FIELD_Y: f32 = 360.0;

/// per-sprite drift velocity in world units per tick-second.
#[derive(Component, Clone, Copy)]
struct Drift {
	velocity: Vec2,
}

pub fn register(app: &mut App) {
	app.add_startup_system(spawn);
	app.add_system_to_stage(UpdateStage::Update, drift_system);
}

fn spawn(mut commands: Commands, mut assets: ResMut<AssetServer>) {
	let tex = checker_texture(&mut assets, 16, 4, [255, 210, 120, 255], [200, 90, 60, 255]);
	let font = assets.load_font("fonts/Inconsolata.ttf");

	let mut rng = Rng::new(0x5271_7E00);
	for _ in 0..SPRITES {
		let position = Vec2::new(rng.range(-FIELD_X, FIELD_X), rng.range(-FIELD_Y, FIELD_Y));
		let velocity = Vec2::new(rng.range(-40.0, 40.0), rng.range(-40.0, 40.0));
		let tint = Color::rgb(rng.range(0.4, 1.0), rng.range(0.4, 1.0), rng.range(0.4, 1.0));
		commands.spawn((
			Transform { translation: position, rotation: rng.range(0.0, std::f32::consts::TAU), scale: Vec2::splat(1.0) },
			Sprite::new(tex).with_size(Vec2::splat(8.0)).with_color(tint),
			Drift { velocity },
		));
	}

	// text labels scattered over the field, exercising the glyph atlas.
	for i in 0..LABELS {
		let position = Vec2::new(rng.range(-FIELD_X, FIELD_X), rng.range(-FIELD_Y, FIELD_Y));
		commands.spawn((
			Transform::from_xy(position.x, position.y),
			Text::new(format!("lbl{i:03}"), font)
				.with_size(14.0)
				.with_color(Color::WHITE),
		));
	}
}

/// drift every sprite and wrap it back into the field so the scene never empties.
fn drift_system(time: Res<Time>, mut sprites: Query<(&Drift, &mut Transform)>) {
	let dt = time.delta_seconds();
	for (drift, mut transform) in &mut sprites {
		transform.translation += drift.velocity * dt;
		if transform.translation.x > FIELD_X {
			transform.translation.x -= FIELD_X * 2.0;
		} else if transform.translation.x < -FIELD_X {
			transform.translation.x += FIELD_X * 2.0;
		}
		if transform.translation.y > FIELD_Y {
			transform.translation.y -= FIELD_Y * 2.0;
		} else if transform.translation.y < -FIELD_Y {
			transform.translation.y += FIELD_Y * 2.0;
		}
	}
}
