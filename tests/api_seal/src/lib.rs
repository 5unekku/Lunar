//! API seal test — verifies game code can use the full ECS contract through
//! `lunar` alone, without naming `bevy_ecs` anywhere.
//!
//! If this compiles, the seal holds. If it ever fails, the abstraction is
//! leaking and the fix is in the engine, not here.

#![allow(dead_code)]

use lunar::prelude::*;

// component derive — generated impl routes through lunar's hidden bevy_ecs
#[derive(Component)]
struct Player {
    health: u32,
}

#[derive(Component)]
struct Velocity(Vec2);

// resource derive
#[derive(Resource, Default)]
struct Score(u32);

// message derive
#[derive(Message)]
struct PlayerDied;

// event derive
#[derive(Event)]
struct LevelLoaded;

// systems using sealed prelude types only
fn spawn_player(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let texture = assets.load_texture("player.png");
    commands.spawn((
        Player { health: 100 },
        Velocity(Vec2::ZERO),
        Transform::default(),
        // high-level component-driven rendering — no DrawCommand in sight
        Sprite::new(texture)
            .with_size(Vec2::new(32.0, 32.0))
            .with_color(Color::WHITE)
            .with_layer(layers::GAME),
    ));
}

fn spawn_label(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let font = assets.load_font("ui.ttf");
    commands.spawn((
        Transform::from_xy(10.0, 10.0),
        Text::new("Score: 0", font)
            .with_size(20.0)
            .with_color(Color::WHITE),
    ));
}

fn draw_hud(mut queue: ResMut<RenderQueue>) {
    // imperative escape hatch for HUD / debug — still part of the sealed API
    queue.draw_rect(
        Vec2::ZERO,
        Vec2::new(200.0, 40.0),
        Color::rgba(0.0, 0.0, 0.0, 0.6),
    );
}

fn move_players(time: Res<Time>, mut query: Query<(&mut Transform, &Velocity), With<Player>>) {
    let delta = time.delta_seconds();
    for (mut transform, velocity) in &mut query {
        transform.translation.x += velocity.0.x * delta;
        transform.translation.y += velocity.0.y * delta;
    }
}

fn track_score(mut score: ResMut<Score>, mut deaths: MessageReader<PlayerDied>) {
    for _ in deaths.read() {
        score.0 += 1;
    }
}

fn read_input(input: Res<InputState>, mut query: Query<&mut Velocity, With<Player>>) {
    for mut velocity in &mut query {
        velocity.0.x = if input.is_key_held(KeyCode::Right) {
            100.0
        } else {
            0.0
        };
    }
}

#[derive(Default)]
struct SealTestPlugin;

impl GamePlugin for SealTestPlugin {
    fn name(&self) -> &str {
        "SealTestPlugin"
    }
    fn build(&mut self, app: &mut App) {
        app.insert_resource(Score::default());
        app.add_startup_system((spawn_player, spawn_label));
        app.add_system((move_players, read_input, track_score));
        app.add_system(draw_hud);
    }
}
