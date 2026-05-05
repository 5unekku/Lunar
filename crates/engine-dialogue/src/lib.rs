//! dialogue system for lunar.
//!
//! provides multi-stage conversations, speaker identification, branching
//! choices, and narrator text. extracted from `engine-core` so games that
//! don't need it pay zero compile cost.
//!
//! # quick start
//!
//! ```ignore
//! use engine_dialogue::{DialogueBuilder, DialogueManager, DialoguePlugin};
//! use engine_core::App;
//!
//! let mut app = App::new();
//! app.add_plugin(DialoguePlugin);
//!
//! let dialogue = DialogueBuilder::new("start")
//!     .line("greeting", Some("NPC"), "hello there!", Some("farewell"))
//!     .build();
//! ```

mod dialogue;
mod parser;

pub use dialogue::{
    Dialogue, DialogueBuilder, DialogueChoice, DialogueLine, DialogueManager, DialogueNode,
    DialoguePlugin, DialogueState,
};
pub use parser::{parse_dialogue, parse_dialogue_file};
