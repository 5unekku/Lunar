use engine_dialogue::{DialogueBuilder, DialogueManager, DialoguePlugin};
use lunar::engine_core::UpdateStage;
use lunar::prelude::*;

use crate::rpg_example::components::*;
use crate::rpg_example::resources::*;

#[derive(Default)]
pub struct RpgGame;

impl GamePlugin for RpgGame {
    fn name(&self) -> &'static str {
        "RpgGame"
    }

    fn build(&mut self, app: &mut App) {
        app.add_plugin(DialoguePlugin);
        app.add_startup_system(setup);
        app.add_system(overworld_input);
        app.add_system(dialogue_input);
        app.add_system(camera_follow);
        app.add_system_to_stage(UpdateStage::Render, render);
    }
}

pub fn setup(
    mut commands: Commands,
    mut assets: ResMut<AssetServer>,
    mut dialogues: ResMut<DialogueManager>,
) {
    let player_tex = assets.load_texture("sprites/player.webp");
    let npc_tex1 = assets.load_texture("sprites/npc1.webp");
    let npc_tex2 = assets.load_texture("sprites/npc2.webp");
    let npc_tex3 = assets.load_texture("sprites/npc3.webp");
    let npc_tex3_emotion = assets.load_texture("sprites/npc3_emotion.webp");
    let font = assets.load_font("fonts/Inconsolata.ttf");

    let npc_defs = vec![
        NpcData {
            start_x: 600.0,
            start_y: 500.0,
            label: "old man".into(),
            dialogue_name: "npc1".into(),
            has_icon: true,
            icon_color: Color::rgb(0.75, 0.19, 0.19),
            emotion_tex: None,
        },
        NpcData {
            start_x: 1000.0,
            start_y: 700.0,
            label: "traveler".into(),
            dialogue_name: "npc2".into(),
            has_icon: false,
            icon_color: Color::rgb(0.82, 0.69, 0.24),
            emotion_tex: None,
        },
        NpcData {
            start_x: 400.0,
            start_y: 900.0,
            label: "merchant".into(),
            dialogue_name: "npc3".into(),
            has_icon: true,
            icon_color: Color::rgb(0.19, 0.63, 0.25),
            emotion_tex: Some(npc_tex3_emotion),
        },
    ];

    // npc1 — has a question with two choices; the choice is remembered
    let d1 = DialogueBuilder::new("line1")
        .line(
            "line1",
            Some(&npc_defs[0].label),
            "Halt, traveler. These are dangerous roads.",
            Some("line2"),
        )
        .choice_line(
            "line2",
            Some(&npc_defs[0].label),
            "Are you looking for something?",
            vec![
                ("I seek treasure.", "treasure"),
                ("Just passing through.", "passing"),
            ],
        )
        .line(
            "treasure",
            Some(&npc_defs[0].label),
            "Ha! Then head east, past the old ruins. But beware the shadows.",
            None,
        )
        .line(
            "passing",
            Some(&npc_defs[0].label),
            "Then keep your wits about you. The road ahead is long.",
            None,
        )
        .build();
    dialogues.register("npc1", d1);

    // npc2 — simple linear dialogue, no icon
    let d2 = DialogueBuilder::new("l1")
        .line(
            "l1",
            Some(&npc_defs[1].label),
            "The weather is lovely today, isn't it?",
            Some("l2"),
        )
        .line(
            "l2",
            Some(&npc_defs[1].label),
            "I've been walking for hours. Not a single cloud.",
            None,
        )
        .build();
    dialogues.register("npc2", d2);

    // npc3 — has an icon and emotion sprite; dialogue differs based on npc1's answer.
    // two registered variants; plugin.rs picks which to start at runtime.
    let d3_treasure = DialogueBuilder::new("l1")
        .line(
            "l1",
            Some(&npc_defs[2].label),
            "Ah, a treasure hunter! You'll want sturdy boots for those ruins.",
            Some("l2"),
        )
        .line(
            "l2",
            Some(&npc_defs[2].label),
            "Good luck out there. I have nothing to sell today, sorry!",
            None,
        )
        .build();
    dialogues.register("npc3_treasure", d3_treasure);

    let d3_passing = DialogueBuilder::new("l1")
        .line(
            "l1",
            Some(&npc_defs[2].label),
            "Welcome to my humble stall.",
            Some("l2"),
        )
        .line(
            "l2",
            Some(&npc_defs[2].label),
            "I have nothing to sell today. Sorry!",
            None,
        )
        .build();
    dialogues.register("npc3_passing", d3_passing);

    // walls: border walls + a couple of interior obstacles
    let walls = Walls(vec![
        // border walls (player can't leave the room)
        Wall::new(0.0, 0.0, ROOM_WIDTH, 16.0), // top
        Wall::new(0.0, ROOM_HEIGHT - 16.0, ROOM_WIDTH, 16.0), // bottom
        Wall::new(0.0, 0.0, 16.0, ROOM_HEIGHT), // left
        Wall::new(ROOM_WIDTH - 16.0, 0.0, 16.0, ROOM_HEIGHT), // right
        // interior obstacles
        Wall::new(300.0, 300.0, 200.0, 40.0),
        Wall::new(900.0, 400.0, 40.0, 200.0),
        Wall::new(700.0, 800.0, 160.0, 40.0),
    ]);

    commands.spawn((Player, Facing::Down, Transform::from_xy(800.0, 600.0)));

    let npc_textures = vec![npc_tex1, npc_tex2, npc_tex3];
    for (i, def) in npc_defs.iter().enumerate() {
        commands.spawn((Npc(i), Transform::from_xy(def.start_x, def.start_y)));
    }

    commands.insert_resource(GameAssets {
        player_tex,
        npc_textures,
        font,
    });
    commands.insert_resource(NpcDefinitions(npc_defs));
    commands.insert_resource(GameMode::Overworld);
    commands.insert_resource(PlayerChoiceState::default());
    commands.insert_resource(walls);
    commands.insert_resource(Camera {
        position: Vec2::new(800.0, 600.0),
        zoom: 1.0,
        rotation: 0.0,
        viewport: Some((VIEW_WIDTH as u32, VIEW_HEIGHT as u32)),
        layer_parallax: Default::default(),
    });
}

#[allow(clippy::too_many_arguments)]
pub fn overworld_input(
    input: Res<InputState>,
    time: Res<Time>,
    mut player_query: Query<&mut Transform, With<Player>>,
    npc_query: Query<(&Transform, &Npc), Without<Player>>,
    npc_defs: Res<NpcDefinitions>,
    walls: Res<Walls>,
    mut mode: ResMut<GameMode>,
    mut dialogues: ResMut<DialogueManager>,
    choice_state: Res<PlayerChoiceState>,
) {
    if !matches!(*mode, GameMode::Overworld) {
        return;
    }
    let Ok(mut player) = player_query.single_mut() else {
        return;
    };

    let mut dx: f32 = 0.0;
    let mut dy: f32 = 0.0;
    if input.is_key_held(KeyCode::Left) || input.is_key_held(KeyCode::A) {
        dx -= 1.0;
    }
    if input.is_key_held(KeyCode::Right) || input.is_key_held(KeyCode::D) {
        dx += 1.0;
    }
    if input.is_key_held(KeyCode::Up) || input.is_key_held(KeyCode::W) {
        dy -= 1.0;
    }
    if input.is_key_held(KeyCode::Down) || input.is_key_held(KeyCode::S) {
        dy += 1.0;
    }

    let speed = PLAYER_SPEED * time.delta_seconds();
    if dx != 0.0 || dy != 0.0 {
        let len = dx.hypot(dy);
        let move_x = dx / len * speed;
        let move_y = dy / len * speed;

        // resolve x and y independently so the player slides along walls
        let new_x = player.translation.x + move_x;
        if !player_collides_walls(new_x, player.translation.y, &walls) {
            player.translation.x = new_x;
        }
        let new_y = player.translation.y + move_y;
        if !player_collides_walls(player.translation.x, new_y, &walls) {
            player.translation.y = new_y;
        }
    }

    if input.is_key_just_pressed(KeyCode::Space) || input.is_key_just_pressed(KeyCode::Enter) {
        let px = player.translation.x;
        let py = player.translation.y;
        for (npc_t, npc) in &npc_query {
            let d = Vec2::new(px - npc_t.translation.x, py - npc_t.translation.y);
            if d.length() > INTERACT_RANGE {
                continue;
            }
            let def = &npc_defs.0[npc.0];
            let dialogue_key = if npc.0 == 2 {
                // npc3 reacts to npc1's answer
                match choice_state.npc1_choice {
                    Some(0) => "npc3_treasure",
                    _ => "npc3_passing",
                }
            } else {
                &def.dialogue_name
            };
            dialogues.start(dialogue_key);
            *mode = GameMode::Dialogue {
                npc_index: npc.0,
                text_visible_chars: 0,
                text_timer: 0.0,
                choice_selection: 0,
            };
            break;
        }
    }
}

/// returns true if a player-sized AABB at (cx, cy) overlaps any wall.
fn player_collides_walls(cx: f32, cy: f32, walls: &Walls) -> bool {
    let half_w = SPRITE_W * 0.5;
    let half_h = SPRITE_H * 0.5;
    let px0 = cx - half_w;
    let px1 = cx + half_w;
    let py0 = cy - half_h;
    let py1 = cy + half_h;
    for wall in &walls.0 {
        let wx1 = wall.x + wall.w;
        let wy1 = wall.y + wall.h;
        if px0 < wx1 && px1 > wall.x && py0 < wy1 && py1 > wall.y {
            return true;
        }
    }
    false
}

pub fn dialogue_input(
    input: Res<InputState>,
    time: Res<Time>,
    mut mode: ResMut<GameMode>,
    mut dialogues: ResMut<DialogueManager>,
    mut choice_state: ResMut<PlayerChoiceState>,
) {
    let transition = match &mut *mode {
        GameMode::Dialogue {
            npc_index,
            text_timer,
            text_visible_chars,
            choice_selection,
        } => {
            *text_timer += time.delta_seconds();
            let total = dialogues.current_line().map(|l| l.text.len()).unwrap_or(0);
            *text_visible_chars = (*text_timer * CPS) as usize;

            if dialogues.has_choices() && *text_visible_chars >= total {
                let count = dialogues.choice_labels().len();
                if input.is_key_just_pressed(KeyCode::Up) || input.is_key_just_pressed(KeyCode::W) {
                    *choice_selection = choice_selection.saturating_sub(1);
                }
                if input.is_key_just_pressed(KeyCode::Down) || input.is_key_just_pressed(KeyCode::S)
                {
                    *choice_selection = choice_selection.saturating_add(1).min(count - 1);
                }
            }

            let press = input.is_key_just_pressed(KeyCode::Space)
                || input.is_key_just_pressed(KeyCode::Enter);
            if !press {
                None
            } else if *text_visible_chars < total && total > 0 {
                *text_visible_chars = total;
                None
            } else if dialogues.has_choices() {
                let chosen = *choice_selection;
                let npc = *npc_index;
                dialogues.choose(chosen);
                // remember npc1's choice
                if npc == 0 {
                    choice_state.npc1_choice = Some(chosen);
                }
                if !dialogues.is_active() {
                    Some(GameMode::Overworld)
                } else {
                    *text_visible_chars = 0;
                    *text_timer = 0.0;
                    *choice_selection = 0;
                    None
                }
            } else {
                dialogues.advance();
                if !dialogues.is_active() {
                    Some(GameMode::Overworld)
                } else {
                    *text_visible_chars = 0;
                    *text_timer = 0.0;
                    None
                }
            }
        }
        _ => return,
    };
    if let Some(new) = transition {
        *mode = new;
    }
}

/// clamp camera to world bounds, per-axis.
/// if the world is smaller than the viewport on an axis, center it.
pub fn camera_follow(player_query: Query<&Transform, With<Player>>, mut camera: ResMut<Camera>) {
    let Ok(player) = player_query.single() else {
        return;
    };

    let half_vw = VIEW_WIDTH * 0.5;
    let half_vh = VIEW_HEIGHT * 0.5;

    camera.position.x = if ROOM_WIDTH <= VIEW_WIDTH {
        ROOM_WIDTH * 0.5
    } else {
        player.translation.x.clamp(half_vw, ROOM_WIDTH - half_vw)
    };

    camera.position.y = if ROOM_HEIGHT <= VIEW_HEIGHT {
        ROOM_HEIGHT * 0.5
    } else {
        player.translation.y.clamp(half_vh, ROOM_HEIGHT - half_vh)
    };
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    mode: Res<GameMode>,
    assets: Res<GameAssets>,
    npc_defs: Res<NpcDefinitions>,
    walls: Res<Walls>,
    window: Res<WindowSettings>,
    player_query: Query<&Transform, With<Player>>,
    npc_query: Query<(&Transform, &Npc)>,
    camera: Res<Camera>,
    dialogues: Res<DialogueManager>,
    mut queue: ResMut<RenderQueue>,
) {
    // ground
    queue.draw_rect_on_layer(
        Vec2::ZERO,
        Vec2::new(ROOM_WIDTH, ROOM_HEIGHT),
        Color::rgb(0.2, 0.55, 0.15),
        layers::BACKGROUND,
    );

    // walls
    for wall in &walls.0 {
        queue.draw_rect_on_layer(
            Vec2::new(wall.x, wall.y),
            Vec2::new(wall.w, wall.h),
            Color::rgb(0.35, 0.25, 0.15),
            layers::GAME,
        );
    }

    // player
    if let Ok(player) = player_query.single() {
        queue.draw_sprite_on_layer(
            &assets.player_tex,
            player.translation,
            Vec2::new(SPRITE_W, SPRITE_H),
            layers::GAME,
        );
    }

    // npcs
    for (npc_t, npc) in &npc_query {
        queue.draw_sprite_on_layer(
            &assets.npc_textures[npc.0],
            npc_t.translation,
            Vec2::new(SPRITE_W, SPRITE_H),
            layers::GAME,
        );
    }

    // dialogue box
    if let GameMode::Dialogue {
        npc_index,
        text_visible_chars,
        choice_selection,
        ..
    } = &*mode
    {
        let def = &npc_defs.0[*npc_index];
        let line = dialogues.current_line();
        let text = line.map(|l| l.text.as_str()).unwrap_or("");
        let speaker = line.and_then(|l| l.speaker.as_deref());

        let box_y = VIEW_HEIGHT - DIALOGUE_BOX_H;
        let box_origin = camera.screen_to_world(Vec2::new(0.0, box_y), window.width, window.height);
        let box_size = Vec2::new(VIEW_WIDTH, DIALOGUE_BOX_H);

        // box background
        queue.draw_rect_on_layer(
            box_origin,
            box_size,
            Color::rgba(0.0, 0.0, 0.0, 0.78),
            layers::UI,
        );

        // portrait column — always reserved, icon drawn only if has_icon
        let icon_x = box_origin.x + 4.0;
        let icon_y = box_origin.y + (DIALOGUE_BOX_H - ICON_SIZE) * 0.5;

        if def.has_icon {
            // use emotion sprite if available, otherwise the base icon color rect
            if let Some(emotion) = &def.emotion_tex {
                queue.draw_sprite_on_layer(
                    emotion,
                    Vec2::new(icon_x + ICON_SIZE * 0.5, icon_y + ICON_SIZE * 0.5),
                    Vec2::new(ICON_SIZE, ICON_SIZE),
                    layers::UI,
                );
            } else {
                queue.draw_rect_on_layer(
                    Vec2::new(icon_x, icon_y),
                    Vec2::new(ICON_SIZE, ICON_SIZE),
                    def.icon_color,
                    layers::UI,
                );
            }
        }

        // text area starts after the icon column regardless of whether the icon is shown
        let text_x = box_origin.x + ICON_COL_W;
        let name_y = box_origin.y + 6.0;
        let text_y = box_origin.y + 26.0;

        if let Some(name) = speaker {
            queue.draw_text_on_layer(
                &assets.font,
                name,
                Vec2::new(text_x, name_y),
                14.0,
                Color::rgb(0.85, 0.85, 0.35),
                layers::UI,
            );
        }

        let show = (*text_visible_chars).min(text.len());
        if show > 0 {
            queue.draw_text_on_layer(
                &assets.font,
                &text[..show],
                Vec2::new(text_x, text_y),
                16.0,
                Color::WHITE,
                layers::UI,
            );
        }

        // choices
        if *text_visible_chars >= text.len() && dialogues.has_choices() {
            let labels = dialogues.choice_labels();
            let mut cy = text_y + 28.0;
            for (i, label) in labels.iter().enumerate() {
                if *choice_selection == i {
                    queue.draw_rect_on_layer(
                        Vec2::new(text_x - 2.0, cy - 2.0),
                        Vec2::new(VIEW_WIDTH - ICON_COL_W - 8.0, 20.0),
                        Color::rgba(0.3, 0.3, 0.55, 0.55),
                        layers::UI,
                    );
                }
                let color = if *choice_selection == i {
                    Color::rgb(1.0, 1.0, 0.5)
                } else {
                    Color::rgb(0.72, 0.72, 0.72)
                };
                queue.draw_text_on_layer(
                    &assets.font,
                    label,
                    Vec2::new(text_x + 6.0, cy),
                    15.0,
                    color,
                    layers::UI,
                );
                cy += 22.0;
            }
        }

        // advance indicator
        if *text_visible_chars >= text.len() && !dialogues.has_choices() && dialogues.is_active() {
            let ind = camera.screen_to_world(
                Vec2::new(VIEW_WIDTH - 80.0, VIEW_HEIGHT - 18.0),
                window.width,
                window.height,
            );
            queue.draw_text_on_layer(
                &assets.font,
                "[space]",
                ind,
                12.0,
                Color::rgba(0.6, 0.6, 0.6, 0.8),
                layers::UI,
            );
        }
    }
}
