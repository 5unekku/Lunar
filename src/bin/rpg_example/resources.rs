use lunar::prelude::*;

#[derive(Resource)]
pub enum GameMode {
    /// `just_returned` is true for one frame after returning from dialogue,
    /// preventing the same keypress from immediately re-opening a conversation.
    Overworld {
        just_returned: bool,
        /// whether Space/Enter was held on the previous tick — rising edge triggers interaction
        interact_was_held: bool,
    },
    Dialogue {
        npc_index: usize,
        text_visible_chars: usize,
        text_timer: f32,
        choice_selection: usize,
        /// true on the frame the dialogue opens — suppresses input so the
        /// opening keypress doesn't immediately advance the first line
        just_started: bool,
        /// whether Space/Enter was held on the previous tick — used to compute rising edge
        space_was_held: bool,
        /// held state for choice navigation — rising edge moves selection one step
        up_was_held: bool,
        down_was_held: bool,
    },
}

#[derive(Resource)]
pub struct GameAssets {
    pub player_tex: Handle<Texture>,
    pub npc_textures: Vec<Handle<Texture>>,
    pub font: Handle<Font>,
}

pub struct NpcData {
    pub start_col: i32,
    pub start_row: i32,
    pub label: String,
    pub dialogue_name: String,
    pub has_icon: bool,
    pub icon_color: Color,
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

/// flat row-major grid of passable/blocked tiles.
/// out-of-bounds queries return true (impassable).
#[derive(Resource)]
pub struct TileGrid {
    cols: usize,
    rows: usize,
    blocked: Vec<bool>,
}

impl TileGrid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            blocked: vec![false; cols * rows],
        }
    }

    pub fn set_rect(&mut self, col: usize, row: usize, width: usize, height: usize) {
        for r in row..row + height {
            for c in col..col + width {
                if c < self.cols && r < self.rows {
                    self.blocked[r * self.cols + c] = true;
                }
            }
        }
    }

    pub fn is_blocked(&self, col: i32, row: i32) -> bool {
        if col < 0 || row < 0 {
            return true;
        }
        let col = col as usize;
        let row = row as usize;
        if col >= self.cols || row >= self.rows {
            return true;
        }
        self.blocked[row * self.cols + col]
    }

    pub fn iter_blocked(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..self.rows).flat_map(move |row| {
            (0..self.cols).filter_map(move |col| {
                if self.blocked[row * self.cols + col] {
                    Some((col, row))
                } else {
                    None
                }
            })
        })
    }
}

/// per-step movement cooldown in seconds.
#[derive(Resource)]
pub struct MoveTimer(pub f32);

pub fn grid_to_world(col: i32, row: i32) -> Vec2 {
    Vec2::new(
        col as f32 * TILE_SIZE + TILE_SIZE * 0.5,
        row as f32 * TILE_SIZE + TILE_SIZE * 0.5,
    )
}

pub const TILE_SIZE: f32 = 32.0;
pub const GRID_COLS: usize = 50;
pub const GRID_ROWS: usize = 38;
pub const ROOM_WIDTH: f32 = TILE_SIZE * GRID_COLS as f32;
pub const ROOM_HEIGHT: f32 = TILE_SIZE * GRID_ROWS as f32;
pub const VIEW_WIDTH: f32 = 640.0;
pub const VIEW_HEIGHT: f32 = 480.0;
pub const SPRITE_W: f32 = 32.0;
pub const SPRITE_H: f32 = 48.0;
pub const MOVE_COOLDOWN: f32 = 0.15;
pub const CPS: f32 = 10.0;
pub const DIALOGUE_BOX_H: f32 = 128.0;
pub const ICON_COL_W: f32 = 56.0;
pub const ICON_SIZE: f32 = 48.0;
