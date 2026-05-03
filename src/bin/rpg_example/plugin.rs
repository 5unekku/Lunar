use bevy_ecs::prelude::*;
use engine_core::GamePlugin;

#[derive(Default)]
pub struct RpgGame;

impl GamePlugin for RpgGame {
    fn name(&self) -> &'static str {
        "RpgGame"
    }

    fn build(&mut self, app: &mut engine_core::App) {
        app.add_plugin(engine_core::DialoguePlugin);
        app.add_startup_system(setup);
        app.add_system(overworld_input);
        app.add_system(dialogue_input);
        app.add_system(camera_follow);
        app.add_system_to_stage(engine_core::UpdateStage::Render, render);
    }
}

use engine_assets::AssetServer;
use engine_core::{DialogueBuilder, DialogueManager, Time, WindowSettings};
use engine_input::{InputState, KeyCode};
use engine_math::{Color, Transform, Vec2, glam::Vec3Swizzles};
use engine_render::{Camera, RenderInfo, RenderQueue, layers};

use crate::rpg_example::components::*;
use crate::rpg_example::resources::*;

pub fn setup(
    mut commands: Commands,
    mut assets: ResMut<AssetServer>,
    mut dialogues: ResMut<DialogueManager>,
) {
    let player_tex = assets.load_texture("sprites/player.webp");
    let npc_tex1 = assets.load_texture("sprites/npc1.webp");
    let npc_tex2 = assets.load_texture("sprites/npc2.webp");
    let npc_tex3 = assets.load_texture("sprites/npc3.webp");
    let font = assets.load_font("fonts/Inconsolata.ttf");

    let npc_defs = vec![
        NpcData {
            start_x: 600.0,
            start_y: 500.0,
            label: "old man".into(),
            dialogue_name: "npc1".into(),
            has_icon: true,
            icon_color: Color::rgb(200.0 / 255.0, 60.0 / 255.0, 60.0 / 255.0),
        },
        NpcData {
            start_x: 1000.0,
            start_y: 700.0,
            label: "traveler".into(),
            dialogue_name: "npc2".into(),
            has_icon: false,
            icon_color: Color::rgb(220.0 / 255.0, 200.0 / 255.0, 60.0 / 255.0),
        },
        NpcData {
            start_x: 400.0,
            start_y: 900.0,
            label: "merchant".into(),
            dialogue_name: "npc3".into(),
            has_icon: true,
            icon_color: Color::rgb(60.0 / 255.0, 180.0 / 255.0, 80.0 / 255.0),
        },
    ];

    commands.spawn((Player, Facing::Down, Transform::from_xy(800.0, 600.0)));

    let npc_textures = vec![npc_tex1, npc_tex2, npc_tex3];
    for (i, def) in npc_defs.iter().enumerate() {
        commands.spawn((Npc(i), Transform::from_xy(def.start_x, def.start_y)));
    }

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
            Some("end"),
        )
        .line(
            "passing",
            Some(&npc_defs[0].label),
            "Then keep your wits about you. The road ahead is long.",
            Some("end"),
        )
        .line("end", None, "...", None)
        .build();
    dialogues.register("npc1", d1);

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

    let d3 = DialogueBuilder::new("l1")
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
    dialogues.register("npc3", d3);

    commands.insert_resource(GameAssets {
        player_tex,
        npc_textures,
        font,
    });
    commands.insert_resource(NpcDefinitions(npc_defs));
    commands.insert_resource(GameMode::Overworld);
    commands.insert_resource(Camera {
        position: Vec2::new(800.0, 600.0),
        zoom: 1.0,
        rotation: 0.0,
        viewport: Some((VIEW_WIDTH as u32, VIEW_HEIGHT as u32)),
        layer_parallax: Default::default(),
    });
}

pub fn overworld_input(
    input: Res<InputState>,
    time: Res<Time>,
    mut query: Query<&mut Transform, With<Player>>,
    npc_query: Query<(Entity, &Transform, &Npc)>,
    npc_defs: Res<NpcDefinitions>,
    mut mode: ResMut<GameMode>,
    mut dialogues: ResMut<DialogueManager>,
) {
    if !matches!(*mode, GameMode::Overworld) {
        return;
    }
    let Ok(mut player) = query.single_mut() else {
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
        let mut new_x = player.translation.x + dx / len * speed;
        let mut new_y = player.translation.y + dy / len * speed;
        new_x = new_x.clamp(SPRITE_W * 0.5, ROOM_WIDTH - SPRITE_W * 0.5);
        new_y = new_y.clamp(SPRITE_H * 0.5, ROOM_HEIGHT - SPRITE_H * 0.5);
        player.translation.x = new_x;
        player.translation.y = new_y;
    }

    if input.is_key_just_pressed(KeyCode::Space) || input.is_key_just_pressed(KeyCode::Enter) {
        let px = player.translation.x;
        let py = player.translation.y;
        for (_, npc_t, npc) in &npc_query {
            let nx = npc_t.translation.x;
            let ny = npc_t.translation.y;
            let d = Vec2::new(px - nx, py - ny);
            if d.length() > INTERACT_RANGE {
                continue;
            }
            let def = &npc_defs.0[npc.0];
            dialogues.start(&def.dialogue_name);
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

pub fn dialogue_input(
    input: Res<InputState>,
    time: Res<Time>,
    mut mode: ResMut<GameMode>,
    mut dialogues: ResMut<DialogueManager>,
) {
    let transition = match &mut *mode {
        GameMode::Dialogue {
            text_timer,
            text_visible_chars,
            choice_selection,
            ..
        } => {
            *text_timer += time.delta_seconds();
            let total = dialogues.current_line().map(|l| l.text.len()).unwrap_or(0);
            *text_visible_chars = (*text_timer * CPS) as usize;

            // choice navigation
            if dialogues.has_choices() && *text_visible_chars >= total {
                let cc = dialogues.choice_labels().len();
                if input.is_key_just_pressed(KeyCode::Up) || input.is_key_just_pressed(KeyCode::W) {
                    *choice_selection = choice_selection.saturating_sub(1);
                }
                if input.is_key_just_pressed(KeyCode::Down) || input.is_key_just_pressed(KeyCode::S)
                {
                    *choice_selection = choice_selection.saturating_add(1).min(cc - 1);
                }
            }

            // advance
            let press = input.is_key_just_pressed(KeyCode::Space)
                || input.is_key_just_pressed(KeyCode::Enter);
            if !press {
                None
            } else if *text_visible_chars < total && total > 0 {
                *text_visible_chars = total;
                None
            } else if dialogues.has_choices() {
                dialogues.choose(*choice_selection);
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

pub fn camera_follow(player_query: Query<&Transform, With<Player>>, mut camera: ResMut<Camera>) {
    let Ok(player) = player_query.single() else {
        return;
    };
    let px = player.translation.x;
    let py = player.translation.y;
    camera.position.x = px.clamp(VIEW_WIDTH * 0.5, ROOM_WIDTH - VIEW_WIDTH * 0.5);
    camera.position.y = py.clamp(VIEW_HEIGHT * 0.5, ROOM_HEIGHT - VIEW_HEIGHT * 0.5);
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    mode: Res<GameMode>,
    assets: Res<GameAssets>,
    npc_defs: Res<NpcDefinitions>,
    window: Res<WindowSettings>,
    player_query: Query<&Transform, With<Player>>,
    npc_query: Query<(&Transform, &Npc)>,
    camera: Res<Camera>,
    _info: Res<RenderInfo>,
    dialogues: Res<DialogueManager>,
    mut queue: ResMut<RenderQueue>,
) {
    queue.draw_rect_on_layer(
        Vec2::ZERO,
        Vec2::new(ROOM_WIDTH, ROOM_HEIGHT),
        Color::rgb(0.2, 0.6, 0.1),
        layers::BACKGROUND,
    );

    if let Ok(player) = player_query.single() {
        queue.draw_sprite_on_layer(
            &assets.player_tex,
            player.translation.xy(),
            Vec2::new(SPRITE_W, SPRITE_H),
            layers::GAME,
        );
    }
    for (npc_t, npc) in &npc_query {
        queue.draw_sprite_on_layer(
            &assets.npc_textures[npc.0],
            npc_t.translation.xy(),
            Vec2::new(SPRITE_W, SPRITE_H),
            layers::GAME,
        );
    }

    if let GameMode::Dialogue {
        npc_index,
        text_visible_chars,
        choice_selection,
        ..
    } = &*mode
    {
        let def = &npc_defs.0[*npc_index];
        let text = dialogues
            .current_line()
            .map(|l| l.text.as_str())
            .unwrap_or("");
        let speaker = dialogues.current_line().and_then(|l| l.speaker.as_deref());

        let box_origin = camera.screen_to_world(
            Vec2::new(0.0, VIEW_HEIGHT - DIALOGUE_BOX_H),
            window.width,
            window.height,
        );
        let box_size = Vec2::new(VIEW_WIDTH, DIALOGUE_BOX_H);

        queue.draw_rect_on_layer(
            box_origin,
            box_size,
            Color::rgba(0.0, 0.0, 0.0, 0.75),
            layers::UI,
        );

        let mut text_x = box_origin.x + 8.0;
        let text_y = box_origin.y + 28.0;

        if def.has_icon {
            queue.draw_rect_on_layer(
                Vec2::new(box_origin.x + 8.0, box_origin.y + 8.0),
                Vec2::new(16.0, 16.0),
                def.icon_color,
                layers::UI,
            );
            text_x += 24.0;
        }
        if let Some(name) = speaker {
            queue.draw_text_on_layer(
                &assets.font,
                name,
                Vec2::new(text_x, box_origin.y + 4.0),
                14.0,
                Color::rgb(0.8, 0.8, 0.3),
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

        if *text_visible_chars >= text.len() && dialogues.has_choices() {
            let labels = dialogues.choice_labels();
            let mut cy = text_y + 24.0;
            for (i, label) in labels.iter().enumerate() {
                let color = if *choice_selection == i {
                    Color::rgb(1.0, 1.0, 0.5)
                } else {
                    Color::rgb(0.7, 0.7, 0.7)
                };
                if *choice_selection == i {
                    queue.draw_rect_on_layer(
                        Vec2::new(text_x - 4.0, cy - 2.0),
                        Vec2::new(VIEW_WIDTH - 16.0, 20.0),
                        Color::rgba(0.3, 0.3, 0.5, 0.5),
                        layers::UI,
                    );
                }
                queue.draw_text_on_layer(
                    &assets.font,
                    label,
                    Vec2::new(text_x + 8.0, cy),
                    15.0,
                    color,
                    layers::UI,
                );
                cy += 22.0;
            }
        }

        if *text_visible_chars >= text.len() && !dialogues.has_choices() && dialogues.is_active() {
            let ind = camera.screen_to_world(
                Vec2::new(VIEW_WIDTH - 100.0, VIEW_HEIGHT - 20.0),
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
