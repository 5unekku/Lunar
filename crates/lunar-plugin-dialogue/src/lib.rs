//! dialogue system for lunar.
//!
//! conversations are flat arrays of [`Block`]s linked by integer indices.
//! branching is handled by [`Next::Choice`] — each choice carries a label and
//! a target block index, so selecting option `i` jumps to `choices[i].target`.
//!
//! use [`ScriptBuilder`] to author scripts in code, or [`parse_script`] for
//! RON files. register characters first with [`DialogueManager::add_character`].
//!
//! # quick start
//!
//! ```ignore
//! use lunar_dialogue::{DialogueManager, DialoguePlugin, ScriptBuilder};
//! use lunar_core::App;
//!
//! let mut app = App::new();
//! app.add_plugin(DialoguePlugin);
//!
//! // in a setup system:
//! fn setup(mut dialogues: ResMut<DialogueManager>) {
//!     let npc = dialogues.add_character("old man");
//!
//!     let script = ScriptBuilder::new("greeting")
//!         .block("greeting", npc, 0, "hello there!", Some("farewell"))
//!         .block("farewell", npc, 0, "safe travels.", None)
//!         .build()
//!         .unwrap();
//!
//!     dialogues.register("intro", script);
//! }
//! ```

mod dialogue;
mod parser;

pub use dialogue::{
    Block, Character, Choice, DialogueManager, DialoguePlugin, Next, Script, ScriptBuilder,
};
pub use parser::{parse_script, parse_script_file};

/// common, game-facing dialogue types for `use lunar::prelude::*`.
pub mod prelude {
    pub use crate::{Block, Character, Choice, DialogueManager, DialoguePlugin, Next, Script, ScriptBuilder};
}
