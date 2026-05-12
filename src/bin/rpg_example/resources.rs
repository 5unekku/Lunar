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
    /// whether to show the portrait icon in the dialogue box.
    /// when false, the icon column is still reserved so text aligns.
    pub has_icon: bool,
    pub icon_color: Color,
    /// alternate portrait texture shown during certain dialogue lines.
    /// None = no emotion variant.
    pub emotion_tex: Option<Handle<Texture>>,
}

#[derive(Resource)]
pub struct NpcDefinitions(pub Vec<NpcData>);

/// which branch the player chose when talking to npc1.
/// None = hasn't spoken to npc1 yet.
#[derive(Resource, Default)]
pub struct PlayerChoiceState {
    pub npc1_choice: Option<usize>,
}

/// axis-aligned wall rectangle in world space.
pub struct Wall {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Wall {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

#[derive(Resource)]
pub struct Walls(pub Vec<Wall>);

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
/// width of the portrait/icon column in the dialogue box.
pub const ICON_COL_W: f32 = 56.0;
/// portrait icon size.
pub const ICON_SIZE: f32 = 48.0;
