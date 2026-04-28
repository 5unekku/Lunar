//! dialogue authoring format parser
//!
//! provides a yaml-based format for writing dialogue files that compile
//! into the engine's [`Dialogue`](crate::dialogue::Dialogue) data structures.
//!
//! # format
//!
//! dialogue files are written in yaml with the following structure:
//!
//! ```yaml
//! start: greeting
//! nodes:
//!   greeting:
//!     speaker: "NPC"
//!     text: "hello there, traveler!"
//!     sprite_change: "npc_happy"
//!     next: farewell
//!   farewell:
//!     speaker: "NPC"
//!     text: "what would you like to do?"
//!     choices:
//!       - label: "leave"
//!         target: "end"
//!       - label: "ask more"
//!         target: "more_info"
//!   more_info:
//!     text: "the world is full of wonders..."
//!     next: farewell
//!   end:
//!     text: "safe travels!"
//! ```
//!
//! # example
//!
//! ```ignore
//! use engine_core::dialogue_parser::parse_dialogue;
//!
//! let yaml = r#"
//! start: greeting
//! nodes:
//!   greeting:
//!     speaker: "NPC"
//!     text: "hello!"
//!     next: end
//!   end:
//!     text: "bye!"
//! "#;
//!
//! let dialogue = parse_dialogue(yaml).expect("failed to parse dialogue");
//! ```

use crate::dialogue::{Dialogue, DialogueChoice, DialogueLine, DialogueNode};
use serde::Deserialize;

/// raw yaml representation of a dialogue file.
#[derive(Debug, Deserialize)]
struct RawDialogue {
    /// the entry point node id
    start: String,
    /// all nodes in this dialogue, keyed by id
    nodes: std::collections::HashMap<String, RawNode>,
}

/// raw yaml representation of a single dialogue node.
#[derive(Debug, Deserialize)]
struct RawNode {
    /// optional speaker identifier (none for narrator text)
    speaker: Option<String>,
    /// the text content
    text: String,
    /// optional sprite change trigger
    #[serde(default)]
    sprite_change: Option<String>,
    /// optional next node id for auto-advance
    #[serde(default)]
    next: Option<String>,
    /// optional branching choices
    #[serde(default)]
    choices: Vec<RawChoice>,
}

/// raw yaml representation of a choice.
#[derive(Debug, Deserialize)]
struct RawChoice {
    /// the label shown to the player
    label: String,
    /// the target node id if chosen
    target: String,
}

/// parse a yaml dialogue string into a [`Dialogue`].
///
/// returns an error if the yaml is malformed or references invalid nodes.
pub fn parse_dialogue(source: &str) -> Result<Dialogue, String> {
    let raw: RawDialogue =
        serde_yaml::from_str(source).map_err(|e| format!("yaml parse error: {e}"))?;

    let start = raw.start.clone();

    if !raw.nodes.contains_key(&start) {
        return Err(format!("start node '{start}' does not exist in nodes"));
    }

    let mut nodes = Vec::new();

    for (id, raw_node) in &raw.nodes {
        // validate that next targets exist
        if let Some(ref next) = raw_node.next
            && !raw.nodes.contains_key(next)
        {
            return Err(format!(
                "node '{id}' references non-existent next node '{next}'"
            ));
        }

        // validate that choice targets exist
        for choice in &raw_node.choices {
            if !raw.nodes.contains_key(&choice.target) {
                return Err(format!(
                    "node '{id}' has choice targeting non-existent node '{}'",
                    choice.target
                ));
            }
        }

        let node = DialogueNode {
            id: id.clone(),
            line: DialogueLine {
                speaker: raw_node.speaker.clone(),
                text: raw_node.text.clone(),
                sprite_change: raw_node.sprite_change.clone(),
                choices: raw_node
                    .choices
                    .iter()
                    .map(|c| DialogueChoice {
                        label: c.label.clone(),
                        target: c.target.clone(),
                    })
                    .collect(),
            },
            next: raw_node.next.clone(),
        };
        nodes.push(node);
    }

    Ok(Dialogue { start, nodes })
}

/// parse a dialogue file from disk.
///
/// reads the file at the given path and parses it as yaml.
pub fn parse_dialogue_file(path: &str) -> Result<Dialogue, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read dialogue file '{path}': {e}"))?;
    parse_dialogue(&source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_dialogue() {
        let yaml = r#"
start: greeting
nodes:
  greeting:
    speaker: "NPC"
    text: "hello!"
    next: end
  end:
    text: "bye!"
"#;
        let dialogue = parse_dialogue(yaml).expect("should parse");
        assert_eq!(dialogue.start, "greeting");
        assert_eq!(dialogue.nodes.len(), 2);
    }

    #[test]
    fn parse_dialogue_with_choices() {
        let yaml = r#"
start: question
nodes:
  question:
    speaker: "NPC"
    text: "what do you want?"
    choices:
      - label: "yes"
        target: "yes_path"
      - label: "no"
        target: "no_path"
  yes_path:
    text: "you said yes!"
  no_path:
    text: "you said no!"
"#;
        let dialogue = parse_dialogue(yaml).expect("should parse");
        let question = dialogue
            .get_node("question")
            .expect("should have question node");
        assert_eq!(question.line.choices.len(), 2);
        assert_eq!(question.line.choices[0].label, "yes");
        assert_eq!(question.line.choices[1].target, "no_path");
    }

    #[test]
    fn parse_invalid_next_fails() {
        let yaml = r#"
start: a
nodes:
  a:
    text: "hello"
    next: nonexistent
"#;
        let result = parse_dialogue(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_choice_target_fails() {
        let yaml = r#"
start: a
nodes:
  a:
    text: "hello"
    choices:
      - label: "go"
        target: nowhere
"#;
        let result = parse_dialogue(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn parse_narrator_text() {
        let yaml = r#"
start: narration
nodes:
  narration:
    text: "the wind howls through the trees..."
    next: end
  end:
    text: "the end."
"#;
        let dialogue = parse_dialogue(yaml).expect("should parse");
        let narration = dialogue
            .get_node("narration")
            .expect("should have narration node");
        assert!(narration.line.speaker.is_none());
    }

    #[test]
    fn parse_invalid_start_fails() {
        let yaml = r#"
start: nonexistent
nodes:
  a:
    text: "hello"
"#;
        let result = parse_dialogue(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn parse_sprite_change() {
        let yaml = r#"
start: greet
nodes:
  greet:
    speaker: "NPC"
    text: "hi!"
    sprite_change: "npc_angry"
    next: end
  end:
    text: "bye"
"#;
        let dialogue = parse_dialogue(yaml).expect("should parse");
        let greet = dialogue.get_node("greet").expect("should have greet node");
        assert_eq!(greet.line.sprite_change, Some("npc_angry".to_string()));
    }
}
