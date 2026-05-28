//! top-down shooter example — player, bullets, enemies, AABB collision, score.
//!
//! depends only on `lunar::prelude`. demonstrates:
//! - 2D movement and input via `InputState` and `KeyCode`
//! - AABB collision without a physics engine
//! - score and lives tracking as `Resource`s
//! - `draw_rect` + `draw_text` for gameplay and HUD

use lunar::prelude::*;

// ── constants ─────────────────────────────────────────────────────────────────

const ARENA_W: f32 = 480.0;
const ARENA_H: f32 = 640.0;

const PLAYER_W: f32 = 30.0;
const PLAYER_H: f32 = 30.0;
const PLAYER_SPEED: f32 = 200.0;

const BULLET_W: f32 = 6.0;
const BULLET_H: f32 = 14.0;
const BULLET_SPEED: f32 = 480.0;
const FIRE_RATE: f32 = 0.15;

const ENEMY_W: f32 = 36.0;
const ENEMY_H: f32 = 24.0;
const ENEMY_SPEED: f32 = 80.0;
const ENEMY_SPAWN_RATE: f32 = 1.2;
const MAX_ENEMIES: usize = 20;

// ── components ────────────────────────────────────────────────────────────────

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Bullet;

#[derive(Component)]
struct Enemy;

// ── resources ─────────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
struct Score(u32);

#[derive(Resource)]
struct Lives(u32);

#[derive(Resource)]
struct FireTimer(f32);

#[derive(Resource)]
struct SpawnTimer(f32);

#[derive(Resource)]
struct GameFont(Handle<Font>);

#[derive(Resource, PartialEq, Eq)]
enum GameState {
    Playing,
    GameOver,
}

// ── plugin ────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ShooterGame;

impl GamePlugin for ShooterGame {
    fn name(&self) -> &'static str {
        "ShooterGame"
    }

    fn build(&mut self, app: &mut App) {
        app.add_startup_system(setup);
        app.add_ordered_systems((
            player_input,
            move_bullets,
            move_enemies,
            bullet_enemy_collision,
            player_enemy_collision,
            enemy_spawner,
        ));
        app.add_system_to_stage(UpdateStage::Render, render);
    }
}

// ── setup ─────────────────────────────────────────────────────────────────────

fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    commands.insert_resource(Camera {
        position: Vec2::new(ARENA_W * 0.5, ARENA_H * 0.5),
        zoom: 1.0,
        rotation: 0.0,
        viewport: Some((ARENA_W as u32, ARENA_H as u32)),
        layer_parallax: Default::default(),
        target: None,
    });

    let font = assets.load_font("fonts/Inconsolata.ttf");
    commands.insert_resource(GameFont(font));
    commands.insert_resource(Score::default());
    commands.insert_resource(Lives(3));
    commands.insert_resource(FireTimer(0.0));
    commands.insert_resource(SpawnTimer(0.0));
    commands.insert_resource(GameState::Playing);

    commands.spawn((
        Player,
        Transform::from_xy(ARENA_W * 0.5, ARENA_H - 60.0),
    ));
}

// ── systems ───────────────────────────────────────────────────────────────────

fn player_input(
    mut commands: Commands,
    input: Res<InputState>,
    time: Res<Time>,
    game_state: Res<GameState>,
    mut fire_timer: ResMut<FireTimer>,
    mut query: Query<&mut Transform, With<Player>>,
) {
    if *game_state == GameState::GameOver {
        return;
    }
    let Ok(mut transform) = query.single_mut() else {
        return;
    };
    let delta = time.delta_seconds();

    let mut dx = 0.0f32;
    if input.is_key_held(KeyCode::Left) || input.is_key_held(KeyCode::A) {
        dx -= PLAYER_SPEED * delta;
    }
    if input.is_key_held(KeyCode::Right) || input.is_key_held(KeyCode::D) {
        dx += PLAYER_SPEED * delta;
    }
    transform.translation.x = (transform.translation.x + dx)
        .clamp(PLAYER_W * 0.5, ARENA_W - PLAYER_W * 0.5);

    fire_timer.0 -= delta;
    if fire_timer.0 <= 0.0 && input.is_key_held(KeyCode::Space) {
        fire_timer.0 = FIRE_RATE;
        commands.spawn((
            Bullet,
            Transform::from_xy(transform.translation.x, transform.translation.y - PLAYER_H * 0.5),
        ));
    }
}

fn move_bullets(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Transform), With<Bullet>>,
) {
    let delta = time.delta_seconds();
    for (entity, mut transform) in query.iter_mut() {
        transform.translation.y -= BULLET_SPEED * delta;
        if transform.translation.y < -BULLET_H {
            commands.entity(entity).despawn();
        }
    }
}

fn move_enemies(
    mut commands: Commands,
    time: Res<Time>,
    mut lives: ResMut<Lives>,
    mut game_state: ResMut<GameState>,
    mut query: Query<(Entity, &mut Transform), With<Enemy>>,
) {
    if *game_state == GameState::GameOver {
        return;
    }
    let delta = time.delta_seconds();
    for (entity, mut transform) in query.iter_mut() {
        transform.translation.y += ENEMY_SPEED * delta;
        if transform.translation.y > ARENA_H + ENEMY_H {
            commands.entity(entity).despawn();
            if lives.0 > 0 {
                lives.0 -= 1;
            }
            if lives.0 == 0 {
                *game_state = GameState::GameOver;
            }
        }
    }
}

fn bullet_enemy_collision(
    mut commands: Commands,
    mut score: ResMut<Score>,
    bullet_query: Query<(Entity, &Transform), With<Bullet>>,
    enemy_query: Query<(Entity, &Transform), With<Enemy>>,
) {
    for (b_entity, bt) in &bullet_query {
        for (e_entity, et) in &enemy_query {
            if aabb_overlap(bt.translation, Vec2::new(BULLET_W, BULLET_H),
                            et.translation, Vec2::new(ENEMY_W, ENEMY_H))
            {
                commands.entity(b_entity).despawn();
                commands.entity(e_entity).despawn();
                score.0 += 10;
                break;
            }
        }
    }
}

fn player_enemy_collision(
    mut game_state: ResMut<GameState>,
    player_query: Query<&Transform, With<Player>>,
    enemy_query: Query<&Transform, With<Enemy>>,
) {
    if *game_state == GameState::GameOver {
        return;
    }
    let Ok(pt) = player_query.single() else {
        return;
    };
    for et in &enemy_query {
        if aabb_overlap(pt.translation, Vec2::new(PLAYER_W, PLAYER_H),
                        et.translation, Vec2::new(ENEMY_W, ENEMY_H))
        {
            *game_state = GameState::GameOver;
            return;
        }
    }
}

fn enemy_spawner(
    mut commands: Commands,
    time: Res<Time>,
    game_state: Res<GameState>,
    mut spawn_timer: ResMut<SpawnTimer>,
    enemy_query: Query<(), With<Enemy>>,
) {
    if *game_state == GameState::GameOver {
        return;
    }
    spawn_timer.0 -= time.delta_seconds();
    if spawn_timer.0 > 0.0 || enemy_query.iter().count() >= MAX_ENEMIES {
        return;
    }
    spawn_timer.0 = ENEMY_SPAWN_RATE;
    let base_x = pseudo_rand_f32(time.elapsed_seconds()) * (ARENA_W - ENEMY_W * 4.0) + ENEMY_W * 2.0;
    for i in 0..3 {
        let x = base_x + i as f32 * (ENEMY_W + 10.0);
        if x + ENEMY_W * 0.5 > ARENA_W {
            break;
        }
        commands.spawn((
            Enemy,
            Transform::from_xy(x, -ENEMY_H),
        ));
    }
}

fn render(
    game_state: Res<GameState>,
    score: Res<Score>,
    lives: Res<Lives>,
    font: Res<GameFont>,
    player_query: Query<&Transform, With<Player>>,
    bullet_query: Query<&Transform, With<Bullet>>,
    enemy_query: Query<&Transform, With<Enemy>>,
    mut queue: ResMut<RenderQueue>,
) {
    queue.draw_rect_on_layer(
        Vec2::ZERO,
        Vec2::new(ARENA_W, ARENA_H),
        Color::rgb(0.04, 0.04, 0.12),
        layers::BACKGROUND,
    );

    if let Ok(pt) = player_query.single() {
        queue.draw_rect_on_layer(
            pt.translation - Vec2::new(PLAYER_W * 0.5, PLAYER_H * 0.5),
            Vec2::new(PLAYER_W, PLAYER_H),
            Color::rgb(0.2, 0.8, 0.3),
            layers::GAME,
        );
    }

    for bt in &bullet_query {
        queue.draw_rect_on_layer(
            bt.translation - Vec2::new(BULLET_W * 0.5, BULLET_H * 0.5),
            Vec2::new(BULLET_W, BULLET_H),
            Color::rgb(1.0, 0.9, 0.2),
            layers::GAME,
        );
    }

    for et in &enemy_query {
        queue.draw_rect_on_layer(
            et.translation - Vec2::new(ENEMY_W * 0.5, ENEMY_H * 0.5),
            Vec2::new(ENEMY_W, ENEMY_H),
            Color::rgb(0.85, 0.15, 0.15),
            layers::GAME,
        );
    }

    let hud = layers::UI;
    queue.draw_text_on_layer(&font.0, &format!("score: {}", score.0),
        Vec2::new(8.0, 8.0), 18.0, Color::WHITE, hud);
    queue.draw_text_on_layer(&font.0, &format!("lives: {}", lives.0),
        Vec2::new(8.0, 30.0), 18.0, Color::rgb(0.9, 0.4, 0.4), hud);

    if *game_state == GameState::GameOver {
        queue.draw_rect_on_layer(
            Vec2::new(ARENA_W * 0.5 - 120.0, ARENA_H * 0.5 - 40.0),
            Vec2::new(240.0, 80.0),
            Color::rgba(0.0, 0.0, 0.0, 0.75),
            hud,
        );
        queue.draw_text_on_layer(&font.0, "game over",
            Vec2::new(ARENA_W * 0.5 - 60.0, ARENA_H * 0.5 - 15.0),
            28.0, Color::WHITE, hud);
        queue.draw_text_on_layer(&font.0, &format!("final score: {}", score.0),
            Vec2::new(ARENA_W * 0.5 - 70.0, ARENA_H * 0.5 + 15.0),
            18.0, Color::rgb(0.8, 0.8, 0.8), hud);
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn aabb_overlap(center_a: Vec2, size_a: Vec2, center_b: Vec2, size_b: Vec2) -> bool {
    let half_a = size_a * 0.5;
    let half_b = size_b * 0.5;
    (center_a.x - center_b.x).abs() < half_a.x + half_b.x
        && (center_a.y - center_b.y).abs() < half_a.y + half_b.y
}

fn pseudo_rand_f32(seed: f32) -> f32 {
    let x = seed * 127.1 + 311.7;
    let x = x.sin() * 43758.547;
    x - x.floor()
}
