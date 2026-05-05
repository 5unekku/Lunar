use lunar::prelude::*;

#[derive(Resource)]
pub enum GameMode {
    Overworld,
    Dialogue {
        npc_index: usize,
        text_visible_chars: usize,
        text_timer: f32,
        choice_selection: usize,
    },
}

#[derive(Resource)]
pub struct GameAssets {
    pub player_tex: Handle<Texture>,
    pub npc_textures: Vec<Handle<Texture>>,
    pub font: Handle<Font>,
}

pub struct NpcData {
    pub start_x: f32,
    pub start_y: f32,
    pub label: String,
    pub dialogue_name: String,
    pub has_icon: bool,
    pub icon_color: Color,
}

#[derive(Resource)]
pub struct NpcDefinitions(pub Vec<NpcData>);

pub const ROOM_WIDTH: f32 = 1600.0;
pub const ROOM_HEIGHT: f32 = 1200.0;
pub const VIEW_WIDTH: f32 = 640.0;
pub const VIEW_HEIGHT: f32 = 480.0;
pub const SPRITE_W: f32 = 32.0;
pub const SPRITE_H: f32 = 48.0;
pub const PLAYER_SPEED: f32 = 120.0;
pub const INTERACT_RANGE: f32 = 64.0;
pub const CPS: f32 = 10.0;
pub const DIALOGUE_BOX_H: f32 = 128.0;
