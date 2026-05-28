use lunar::prelude::*;

use crate::components::*;
use crate::resources::*;

#[derive(Default)]
pub struct RpgGame;

impl GamePlugin for RpgGame {
    fn name(&self) -> &'static str {
        "RpgGame"
    }

    fn build(&mut self, app: &mut App) {
        app.add_plugin(DialoguePlugin);
        app.add_startup_system(setup);
        app.add_ordered_systems((overworld_input, player_move_animation, dialogue_input));
        app.add_ordered_systems_to_stage(UpdateStage::Render, (camera_follow, render));
    }
}

pub fn setup(
    mut commands: Commands,
    mut assets: ResMut<AssetServer>,
    mut dialogues: ResMut<DialogueManager>,
    mut actions: ResMut<ActionMap>,
) {
    actions.action("move_left")
        .key(KeyCode::Left).key(KeyCode::A)
        .button(GamepadButton::DpadLeft)
        .axis(GamepadAxis::LeftStickX, -0.5);

    actions.action("move_right")
        .key(KeyCode::Right).key(KeyCode::D)
        .button(GamepadButton::DpadRight)
        .axis(GamepadAxis::LeftStickX, 0.5);

    actions.action("move_up")
        .key(KeyCode::Up).key(KeyCode::W)
        .button(GamepadButton::DpadUp)
        .axis(GamepadAxis::LeftStickY, -0.5);

    actions.action("move_down")
        .key(KeyCode::Down).key(KeyCode::S)
        .button(GamepadButton::DpadDown)
        .axis(GamepadAxis::LeftStickY, 0.5);

    actions.action("interact")
        .key(KeyCode::Space).key(KeyCode::Enter)
        .button(GamepadButton::South);

    actions.action("nav_up")
        .key(KeyCode::Up).key(KeyCode::W)
        .button(GamepadButton::DpadUp)
        .axis(GamepadAxis::LeftStickY, -0.5);

    actions.action("nav_down")
        .key(KeyCode::Down).key(KeyCode::S)
        .button(GamepadButton::DpadDown)
        .axis(GamepadAxis::LeftStickY, 0.5);
    let player_tex = assets.load_texture(texture!("sprites/player"));
    let npc_tex1 = assets.load_texture(texture!("sprites/npc1"));
    let npc_tex2 = assets.load_texture(texture!("sprites/npc2"));
    let npc_tex3 = assets.load_texture(texture!("sprites/npc3"));
    let npc_tex3_emotion = assets.load_texture(texture!("sprites/npc3_emotion"));
    let font = assets.load_font("fonts/Inconsolata.ttf");

    let old_man = dialogues.add_character("old man");
    let traveler = dialogues.add_character("traveler");
    let merchant = dialogues.add_character("merchant");

    let npc_defs = vec![
        NpcData {
            start_col: 18,
            start_row: 15,
            label: "old man".into(),
            dialogue_name: "npc1".into(),
            has_icon: true,
            icon_color: Color::rgb(0.75, 0.19, 0.19),
            emotion_tex: None,
        },
        NpcData {
            start_col: 31,
            start_row: 21,
            label: "traveler".into(),
            dialogue_name: "npc2".into(),
            has_icon: false,
            icon_color: Color::rgb(0.82, 0.69, 0.24),
            emotion_tex: None,
        },
        NpcData {
            start_col: 12,
            start_row: 28,
            label: "merchant".into(),
            dialogue_name: "npc3".into(),
            has_icon: true,
            icon_color: Color::rgb(0.19, 0.63, 0.25),
            emotion_tex: Some(npc_tex3_emotion),
        },
    ];

    // npc1 — has a question with two choices; the choice is remembered
    let d1 = ScriptBuilder::new("line1")
        .block("line1", old_man, 0, "Halt, traveler. These are dangerous roads.", Some("line2"))
        .choice("line2", old_man, 0, "Are you looking for something?", vec![
            ("I seek treasure.", "treasure"),
            ("Just passing through.", "passing"),
        ])
        .block("treasure", old_man, 0, "Ha! Then head east, past the old ruins. But beware the shadows.", None)
        .block("passing", old_man, 0, "Then keep your wits about you. The road ahead is long.", None)
        .build()
        .expect("npc1 script");
    dialogues.register("npc1", d1);

    // npc2 — simple linear dialogue, no icon
    let d2 = ScriptBuilder::new("l1")
        .block("l1", traveler, 0, "The weather is lovely today, isn't it?", Some("l2"))
        .block("l2", traveler, 0, "I've been walking for hours. Not a single cloud.", None)
        .build()
        .expect("npc2 script");
    dialogues.register("npc2", d2);

    // npc3 — two variants depending on npc1's answer
    let d3_treasure = ScriptBuilder::new("l1")
        .block("l1", merchant, 0, "Ah, a treasure hunter! You'll want sturdy boots for those ruins.", Some("l2"))
        .block("l2", merchant, 0, "Good luck out there. I have nothing to sell today, sorry!", None)
        .build()
        .expect("npc3_treasure script");
    dialogues.register("npc3_treasure", d3_treasure);

    let d3_passing = ScriptBuilder::new("l1")
        .block("l1", merchant, 0, "Welcome to my humble stall.", Some("l2"))
        .block("l2", merchant, 0, "I have nothing to sell today. Sorry!", None)
        .build()
        .expect("npc3_passing script");
    dialogues.register("npc3_passing", d3_passing);

    // tile grid — border is impassable via out-of-bounds check; interior obstacles here
    let mut tile_grid = TileGrid::new(GRID_COLS, GRID_ROWS);
    tile_grid.set_rect(9, 9, 7, 2);
    tile_grid.set_rect(28, 12, 2, 7);
    tile_grid.set_rect(21, 25, 6, 2);

    let player_start = GridPos { col: 25, row: 18 };
    let player_world = grid_to_world(player_start.col, player_start.row);
    commands.spawn((
        Player,
        Facing::Down,
        player_start,
        Transform::from_xy(player_world.x, player_world.y),
        PlayerMoveAnimation {
            source: player_world,
            target: player_world,
            elapsed: MOVE_ANIM_DURATION,
        },
    ));

    let npc_textures = vec![npc_tex1, npc_tex2, npc_tex3];
    for (i, def) in npc_defs.iter().enumerate() {
        let world = grid_to_world(def.start_col, def.start_row);
        commands.spawn((
            Npc(i),
            GridPos {
                col: def.start_col,
                row: def.start_row,
            },
            Transform::from_xy(world.x, world.y),
        ));
    }

    commands.insert_resource(GameAssets {
        player_tex,
        npc_textures,
        font,
    });
    commands.insert_resource(NpcDefinitions(npc_defs));
    commands.insert_resource(GameMode::Overworld);
    commands.insert_resource(PlayerChoiceState::default());
    commands.insert_resource(tile_grid);
    commands.insert_resource(MoveTimer(MOVE_COOLDOWN));
    commands.insert_resource(Camera {
        position: player_world,
        zoom: 1.0,
        rotation: 0.0,
        viewport: Some((VIEW_WIDTH as u32, VIEW_HEIGHT as u32)),
        layer_parallax: Default::default(),
        target: None,
    });
}

#[allow(clippy::too_many_arguments)]
pub fn overworld_input(
    input: Res<InputState>,
    actions: Res<ActionMap>,
    time: Res<Time>,
    mut player_query: Query<(&mut GridPos, &mut Facing, &mut PlayerMoveAnimation), With<Player>>,
    npc_query: Query<(&GridPos, &Npc), Without<Player>>,
    npc_defs: Res<NpcDefinitions>,
    tile_grid: Res<TileGrid>,
    mut mode: ResMut<GameMode>,
    mut dialogues: ResMut<DialogueManager>,
    choice_state: Res<PlayerChoiceState>,
    mut move_timer: ResMut<MoveTimer>,
) {
    if !matches!(*mode, GameMode::Overworld) {
        return;
    }
    let Ok((mut grid_pos, mut facing, mut anim)) = player_query.single_mut() else {
        return;
    };

    let direction = if actions.is_action_held(&input, "move_left") {
        Some(Facing::Left)
    } else if actions.is_action_held(&input, "move_right") {
        Some(Facing::Right)
    } else if actions.is_action_held(&input, "move_up") {
        Some(Facing::Up)
    } else if actions.is_action_held(&input, "move_down") {
        Some(Facing::Down)
    } else {
        None
    };

    if let Some(dir) = direction {
        *facing = dir;
        move_timer.0 += time.delta_seconds();
        if move_timer.0 >= MOVE_COOLDOWN {
            let (dcol, drow) = dir.delta();
            let new_col = grid_pos.col + dcol;
            let new_row = grid_pos.row + drow;
            let npc_at_target = npc_query
                .iter()
                .any(|(pos, _)| pos.col == new_col && pos.row == new_row);
            if !tile_grid.is_blocked(new_col, new_row) && !npc_at_target {
                let old_world = grid_to_world(grid_pos.col, grid_pos.row);
                grid_pos.col = new_col;
                grid_pos.row = new_row;
                anim.source = old_world;
                anim.target = grid_to_world(new_col, new_row);
                anim.elapsed = 0.0;
            }
            move_timer.0 = 0.0;
        }
    } else {
        // reset so the next keypress moves immediately without waiting for the cooldown
        move_timer.0 = MOVE_COOLDOWN;
    }

    if actions.is_action_just_pressed(&input, "interact") {
        let (dfacing_col, dfacing_row) = facing.delta();
        let target_col = grid_pos.col + dfacing_col;
        let target_row = grid_pos.row + dfacing_row;
        for (npc_pos, npc) in &npc_query {
            if npc_pos.col != target_col || npc_pos.row != target_row {
                continue;
            }
            let def = &npc_defs.0[npc.0];
            let dialogue_key = if npc.0 == 2 {
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
                just_started: true,
            };
            break;
        }
    }
}

pub fn player_move_animation(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut PlayerMoveAnimation), With<Player>>,
) {
    let Ok((mut transform, mut anim)) = query.single_mut() else { return; };
    if anim.elapsed >= MOVE_ANIM_DURATION {
        return;
    }
    anim.elapsed += time.delta_seconds();
    let t = (anim.elapsed / MOVE_ANIM_DURATION).min(1.0);
    transform.translation.x = anim.source.x + (anim.target.x - anim.source.x) * t;
    transform.translation.y = anim.source.y + (anim.target.y - anim.source.y) * t;
}

pub fn dialogue_input(
    input: Res<InputState>,
    actions: Res<ActionMap>,
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
            just_started,
        } => {
            if *just_started {
                *just_started = false;
                return;
            }
            *text_timer += time.delta_seconds();
            let total = dialogues
                .current_block()
                .map(|b| b.text.chars().count())
                .unwrap_or(0);
            *text_visible_chars = (*text_timer * CPS) as usize;

            if dialogues.has_choices() && *text_visible_chars >= total {
                let count = dialogues.choice_labels().len();
                if actions.is_action_just_pressed(&input, "nav_up") {
                    *choice_selection = choice_selection.saturating_sub(1);
                }
                if actions.is_action_just_pressed(&input, "nav_down") {
                    *choice_selection = choice_selection.saturating_add(1).min(count - 1);
                }
            }

            let press = actions.is_action_just_pressed(&input, "interact");
            if !press {
                None
            } else if *text_visible_chars < total && total > 0 {
                *text_visible_chars = total;
                *text_timer = total as f32 / CPS;
                None
            } else if dialogues.has_choices() {
                let chosen = *choice_selection;
                let npc = *npc_index;
                dialogues.choose(chosen);
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
    tile_grid: Res<TileGrid>,
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
    for (col, row) in tile_grid.iter_blocked() {
        queue.draw_rect_on_layer(
            Vec2::new(col as f32 * TILE_SIZE, row as f32 * TILE_SIZE),
            Vec2::new(TILE_SIZE, TILE_SIZE),
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
        let block = dialogues.current_block();
        let text = block.map(|b| b.text.as_ref()).unwrap_or("");
        let speaker = block.and_then(|b| dialogues.character(b.character)).map(|c| c.name.as_ref());

        let box_y = VIEW_HEIGHT - DIALOGUE_BOX_H;
        let box_origin = camera.screen_to_world(Vec2::new(0.0, box_y), window.width, window.height);
        let box_size = Vec2::new(VIEW_WIDTH, DIALOGUE_BOX_H);

        queue.draw_rect_on_layer(
            box_origin,
            box_size,
            Color::rgba(0.0, 0.0, 0.0, 0.78),
            layers::UI,
        );

        let icon_x = box_origin.x + 4.0;
        let icon_y = box_origin.y + (DIALOGUE_BOX_H - ICON_SIZE) * 0.5;

        if def.has_icon {
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

        let char_count = text.chars().count();
        let show = (*text_visible_chars).min(char_count);
        if show > 0 {
            let show_bytes = text.char_indices().nth(show).map_or(text.len(), |(i, _)| i);
            let text_max_w = VIEW_WIDTH - ICON_COL_W - 16.0;
            queue.draw_text_wrapped(
                &assets.font,
                &text[..show_bytes],
                Vec2::new(text_x, text_y),
                16.0,
                Color::WHITE,
                text_max_w,
                0.0,
                layers::UI,
            );
        }

        if *text_visible_chars >= char_count && dialogues.has_choices() {
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

        if *text_visible_chars >= char_count && !dialogues.has_choices() && dialogues.is_active() {
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
