# Complete Example — Top-Down Shooter

```rust
use lunar::prelude::*;

// --- Components ---

#[derive(Component)]
struct Player {
    speed: f32,
    fire_rate: f32,
    fire_cooldown: f32,
    health: u32,
}

#[derive(Component)]
struct Enemy {
    speed: f32,
    health: u32,
}

#[derive(Component)]
struct Bullet {
    damage: u32,
    lifetime: f32,
}

#[derive(Component)]
struct Velocity(pub Vec2);

#[derive(Component)]
struct Sprite {
    texture: TextureHandle,
    size: Vec2,
}

#[derive(Component)]
struct Collider {
    radius: f32,
}

#[derive(Component)]
struct Dead;

// --- Resources ---

#[derive(Resource)]
struct GameScore(u32);

#[derive(Resource)]
struct SpawnTimer(f32);

// --- Systems ---

fn player_movement_system(
    mut query: Query<(&Player, &mut Velocity)>,
    input: Res<InputState>,
) {
    for (player, mut vel) in query.iter_mut() {
        let mut dir = Vec2::ZERO;
        if input.is_key_held(KeyCode::W) || input.is_key_held(KeyCode::Up) {
            dir.y -= 1.0;
        }
        if input.is_key_held(KeyCode::S) || input.is_key_held(KeyCode::Down) {
            dir.y += 1.0;
        }
        if input.is_key_held(KeyCode::A) || input.is_key_held(KeyCode::Left) {
            dir.x -= 1.0;
        }
        if input.is_key_held(KeyCode::D) || input.is_key_held(KeyCode::Right) {
            dir.x += 1.0;
        }

        if dir.length_squared() > 0.0 {
            dir = dir.normalize();
        }

        vel.0 = dir * player.speed;
    }
}

fn player_shoot_system(
    mut query: Query<(&mut Player, &Transform)>,
    input: Res<InputState>,
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<AssetServer>,
) {
    for (mut player, transform) in query.iter_mut() {
        player.fire_cooldown -= time.delta_seconds();

        if input.is_mouse_button_held(MouseButton::Left) && player.fire_cooldown <= 0.0 {
            player.fire_cooldown = player.fire_rate;
            let bullet_tex = assets.load("textures/bullet.png");

            // spawn bullet
            commands.spawn((
                Bullet {
                    damage: 25,
                    lifetime: 2.0,
                },
                Velocity(Vec2::new(0.0, -500.0)),
                Transform::from_translation(transform.translation),
                Sprite {
                    texture: bullet_tex,
                    size: Vec2::new(4.0, 12.0),
                },
                Collider { radius: 4.0 },
            ));
        }
    }
}

fn enemy_spawn_system(
    mut timer: ResMut<SpawnTimer>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    time: Res<Time>,
) {
    timer.0 -= time.delta_seconds();
    if timer.0 <= 0.0 {
        timer.0 = 2.0; // spawn every 2 seconds

        // deterministic spawn position using sine wave
        let x = time.elapsed_seconds().sin() * 640.0 + 640.0;
        let enemy_tex = assets.load("textures/enemy.png");
        commands.spawn((
            Enemy {
                speed: 80.0,
                health: 50,
            },
            Velocity(Vec2::ZERO),
            Transform::from_translation(Vec3::new(x, -50.0, 0.0)),
            Sprite {
                texture: enemy_tex,
                size: Vec2::new(32.0, 32.0),
            },
            Collider { radius: 16.0 },
        ));
    }
}

fn enemy_movement_system(
    mut query: Query<(&Enemy, &mut Velocity, &Transform)>,
    player_query: Query<&Transform, With<Player>>,
) {
    let player_pos = player_query.iter().next().map(|t| t.translation);
    for (enemy, mut vel, transform) in query.iter_mut() {
        if let Some(pp) = player_pos {
            let dir = (pp - transform.translation).normalize_or_zero();
            vel.0 = dir * enemy.speed;
        }
    }
}

fn physics_movement_system(
    mut query: Query<(&mut Transform, &mut Velocity)>,
    time: Res<Time>,
) {
    let dt = time.delta_seconds();
    for (mut transform, mut vel) in query.iter_mut() {
        transform.translation += vel.0 * dt;
    }
}

fn collision_system(
    mut commands: Commands,
    bullets: Query<(Entity, &Bullet, &Transform, &Collider), Without<Enemy>>,
    enemies: Query<(Entity, &Enemy, &Transform, &Collider), Without<Bullet>>,
    mut score: ResMut<GameScore>,
) {
    for (bullet_ent, bullet_pos, bullet_col) in bullets.iter() {
        for (enemy_ent, enemy, enemy_pos, enemy_col) in enemies.iter() {
            let dist = bullet_pos.translation.distance(enemy_pos.translation);
            if dist < bullet_col.radius + enemy_col.radius {
                // hit!
                commands.entity(bullet_ent).insert(Dead);

                // damage enemy
                let new_health = enemy.health.saturating_sub(bullet.damage);
                if new_health == 0 {
                    commands.entity(enemy_ent).insert(Dead);
                    score.0 += 100;
                }
            }
        }
    }
}

fn cleanup_dead_system(
    mut commands: Commands,
    query: Query<Entity, With<Dead>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn render_system(
    query: Query<(&Transform, &Sprite)>,
    mut render: ResMut<RenderQueue>,
) {
    for (transform, sprite) in query.iter() {
        render.draw_sprite(
            &sprite.texture,
            Vec2::new(transform.translation.x, transform.translation.y),
            sprite.size,
        );
    }
}

fn hud_system(
    score: Res<GameScore>,
    font: Res<DefaultFont>,
    mut render: ResMut<RenderQueue>,
) {
    render.draw_text(
        &font.0,
        &format!("Score: {}", score.0),
        Vec2::new(10.0, 30.0),
        24.0,
        Color::WHITE,
    );
}

// --- Plugin ---

struct ShooterGame;

impl GamePlugin for ShooterGame {
    fn build(&self, app: &mut App) {
        app.insert_resource(GameScore(0))
           .insert_resource(SpawnTimer(2.0))
           .add_startup_system(setup)
           .add_system_to_stage(UpdateStage::Input, player_movement_system)
           .add_system_to_stage(UpdateStage::Input, player_shoot_system)
           .add_system_to_stage(UpdateStage::Update, enemy_spawn_system)
           .add_system_to_stage(UpdateStage::Physics, enemy_movement_system)
           .add_system_to_stage(UpdateStage::Physics, physics_movement_system)
           .add_system_to_stage(UpdateStage::Update, collision_system)
           .add_system_to_stage(UpdateStage::Update, cleanup_dead_system)
           .add_system_to_stage(UpdateStage::Render, render_system)
           .add_system_to_stage(UpdateStage::Render, hud_system);
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        Player {
            speed: 250.0,
            fire_rate: 0.2,
            fire_cooldown: 0.0,
            health: 100,
        },
        Velocity(Vec2::ZERO),
        Transform::from_translation(Vec3::new(640.0, 360.0, 0.0)),
        Sprite {
            texture: assets.load("textures/player.png"),
            size: Vec2::new(32.0, 48.0),
        },
        Collider { radius: 16.0 },
    ));
}

lunar_app!(ShooterGame);
```

---

[← Back to Initialization Order](13-initialization-order.md) | [Next: Web Target Considerations →](appendix-b-web-targets.md)
