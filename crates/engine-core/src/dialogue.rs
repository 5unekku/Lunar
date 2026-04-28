//! dialogue and text system
//!
//! provides multi-stage conversations, speaker identification, branching choices,
//! and narrator text. designed for RPG-style dialogue without requiring games
//! to implement their own solutions.
//!
//! # architecture
//!
//! dialogue is structured as a graph of [`DialogueNode`]s:
//! - each node contains a [`DialogueLine`] with optional speaker and text
//! - nodes can auto-advance to a [`next`](DialogueNode::next) node
//! - nodes can present [`choices`](DialogueLine::choices) for branching
//!
//! use [`DialogueBuilder`] to construct dialogues fluently, then
//! register them with the [`DialogueManager`] resource.
//!
//! # example
//!
//! ```ignore
//! use engine_core::dialogue::{DialogueBuilder, DialogueManager};
//!
//! let dialogue = DialogueBuilder::new("start")
//!     .line("greeting", Some("NPC"), "hello there!", Some("farewell"))
//!     .choice_line("farewell", None, "what do you do?", &[
//!         ("leave", "end"),
//!         ("ask more", "more_info"),
//!     ])
//!     .build();
//!
//! manager.register("intro", dialogue);
//! manager.start("intro");
//! ```

use bevy_ecs::prelude::*;

use crate::app::App;

/// a dialogue line with optional speaker and choices.
///
/// represents a single line of text in a conversation.
/// if [`speaker`](DialogueLine::speaker) is `None`, the text is treated as narrator text.
#[derive(Debug, Clone)]
pub struct DialogueLine {
    /// speaker identifier, none for narrator text
    pub speaker: Option<String>,
    /// the text content
    pub text: String,
    /// optional sprite change for the speaker
    pub sprite_change: Option<String>,
    /// optional choices for branching
    pub choices: Vec<DialogueChoice>,
}

/// a branching choice in a dialogue.
///
/// presents the player with an option that leads to a different dialogue node.
#[derive(Debug, Clone)]
pub struct DialogueChoice {
    /// the text shown to the player
    pub label: String,
    /// the next dialogue node id if chosen
    pub target: String,
}

/// a named dialogue node within a conversation.
///
/// each node represents a single step in the dialogue graph.
/// if [`next`](DialogueNode::next) is set and there are no choices,
/// the dialogue auto-advances to that node.
#[derive(Debug, Clone)]
pub struct DialogueNode {
    /// unique identifier within the conversation
    pub id: String,
    /// the line of dialogue
    pub line: DialogueLine,
    /// next node id if no choices (auto-advance)
    pub next: Option<String>,
}

/// a complete conversation or dialogue tree.
///
/// contains all nodes and a [`start`](Dialogue::start) entry point.
/// use [`DialogueBuilder`] to construct this fluently.
#[derive(Debug, Clone)]
pub struct Dialogue {
    /// the entry point node id
    pub start: String,
    /// all nodes in this dialogue
    pub nodes: Vec<DialogueNode>,
}

impl Dialogue {
    /// create a new dialogue with a start node
    #[must_use]
    pub fn new(start: &str) -> Self {
        Self {
            start: start.to_string(),
            nodes: Vec::new(),
        }
    }

    /// add a node to this dialogue
    pub fn add_node(&mut self, node: DialogueNode) {
        self.nodes.push(node);
    }

    /// get a node by id
    #[must_use]
    pub fn get_node(&self, id: &str) -> Option<&DialogueNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// a builder for constructing dialogue trees.
///
/// provides a fluent interface for adding lines and choices.
/// call [`build`](DialogueBuilder::build) when finished to get a [`Dialogue`].
pub struct DialogueBuilder {
    dialogue: Dialogue,
}

impl DialogueBuilder {
    /// create a new dialogue builder with the given start node id
    #[must_use]
    pub fn new(start: &str) -> Self {
        Self {
            dialogue: Dialogue::new(start),
        }
    }

    /// add a simple line with auto-advance
    #[must_use]
    pub fn line(mut self, id: &str, speaker: Option<&str>, text: &str, next: Option<&str>) -> Self {
        self.dialogue.nodes.push(DialogueNode {
            id: id.to_string(),
            line: DialogueLine {
                speaker: speaker.map(String::from),
                text: text.to_string(),
                sprite_change: None,
                choices: Vec::new(),
            },
            next: next.map(String::from),
        });
        self
    }

    /// add a line with choices
    #[must_use]
    pub fn choice_line(
        mut self,
        id: &str,
        speaker: Option<&str>,
        text: &str,
        choices: Vec<(&str, &str)>,
    ) -> Self {
        self.dialogue.nodes.push(DialogueNode {
            id: id.to_string(),
            line: DialogueLine {
                speaker: speaker.map(String::from),
                text: text.to_string(),
                sprite_change: None,
                choices: choices
                    .into_iter()
                    .map(|(label, target)| DialogueChoice {
                        label: label.to_string(),
                        target: target.to_string(),
                    })
                    .collect(),
            },
            next: None,
        });
        self
    }

    /// finish building
    #[must_use]
    pub fn build(self) -> Dialogue {
        self.dialogue
    }
}

/// dialogue state for an active conversation.
///
/// tracks the current node and whether the dialogue is still active.
/// managed internally by the [`DialogueManager`].
#[derive(Debug, Clone)]
pub struct DialogueState {
    /// the current dialogue definition
    pub dialogue: Dialogue,
    /// the current node id
    pub current_node: String,
    /// whether the dialogue is active
    pub active: bool,
}

impl DialogueState {
    /// create a new dialogue state from a dialogue
    #[must_use]
    pub fn new(dialogue: Dialogue) -> Self {
        let start = dialogue.start.clone();
        Self {
            dialogue,
            current_node: start,
            active: true,
        }
    }

    /// get the current line
    #[must_use]
    pub fn current_line(&self) -> Option<&DialogueLine> {
        self.dialogue.get_node(&self.current_node).map(|n| &n.line)
    }

    /// advance to the next node (auto-advance, no choice)
    pub fn advance(&mut self) {
        if let Some(node) = self.dialogue.get_node(&self.current_node) {
            if let Some(ref next) = node.next {
                self.current_node = next.clone();
            } else if node.line.choices.is_empty() {
                self.active = false;
            }
        }
    }

    /// choose a branch
    pub fn choose(&mut self, index: usize) {
        if let Some(node) = self.dialogue.get_node(&self.current_node)
            && let Some(choice) = node.line.choices.get(index)
        {
            self.current_node = choice.target.clone();
        }
    }

    /// check if there are choices available
    #[must_use]
    pub fn has_choices(&self) -> bool {
        self.current_line()
            .is_some_and(|line| !line.choices.is_empty())
    }
}

/// dialogue manager resource.
///
/// stores registered dialogues and manages the active conversation state.
/// access this resource from systems to start, advance, and close dialogues.
#[derive(Resource)]
pub struct DialogueManager {
    /// registered dialogues by name
    dialogues: std::collections::HashMap<String, Dialogue>,
    /// the currently active dialogue state
    active_dialogue: Option<DialogueState>,
}

impl DialogueManager {
    /// create a new dialogue manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            dialogues: std::collections::HashMap::new(),
            active_dialogue: None,
        }
    }

    /// register a dialogue by name
    pub fn register(&mut self, name: &str, dialogue: Dialogue) {
        self.dialogues.insert(name.to_string(), dialogue);
        log::info!("DialogueManager: registered dialogue '{name}'");
    }

    /// start a dialogue by name
    pub fn start(&mut self, name: &str) {
        if let Some(dialogue) = self.dialogues.get(name).cloned() {
            self.active_dialogue = Some(DialogueState::new(dialogue));
            log::info!("DialogueManager: started dialogue '{name}'");
        } else {
            log::warn!("DialogueManager: dialogue '{name}' not found");
        }
    }

    /// get the current line if a dialogue is active
    #[must_use]
    pub fn current_line(&self) -> Option<&DialogueLine> {
        self.active_dialogue.as_ref().and_then(|s| s.current_line())
    }

    /// advance the current dialogue
    pub fn advance(&mut self) {
        if let Some(state) = &mut self.active_dialogue {
            state.advance();
            if !state.active {
                self.active_dialogue = None;
            }
        }
    }

    /// choose a branch in the current dialogue
    pub fn choose(&mut self, index: usize) {
        if let Some(state) = &mut self.active_dialogue {
            state.choose(index);
        }
    }

    /// check if a dialogue is active
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active_dialogue.as_ref().is_some_and(|s| s.active)
    }

    /// check if the current line has choices
    #[must_use]
    pub fn has_choices(&self) -> bool {
        self.active_dialogue
            .as_ref()
            .is_some_and(DialogueState::has_choices)
    }

    /// get the choice labels for the current line
    #[must_use]
    pub fn choice_labels(&self) -> Vec<&str> {
        self.current_line()
            .map(|line| line.choices.iter().map(|c| c.label.as_str()).collect())
            .unwrap_or_default()
    }

    /// close the active dialogue
    pub fn close(&mut self) {
        self.active_dialogue = None;
    }
}

impl Default for DialogueManager {
    fn default() -> Self {
        Self::new()
    }
}

/// dialogue plugin, registers the dialogue manager resource.
///
/// add this plugin to your [`App`] to enable the dialogue system.
pub struct DialoguePlugin;

impl crate::GamePlugin for DialoguePlugin {
    fn name(&self) -> &'static str {
        "DialoguePlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &[]
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(DialogueManager::new());
        log::info!("DialoguePlugin: dialogue manager resource registered");
    }
}
