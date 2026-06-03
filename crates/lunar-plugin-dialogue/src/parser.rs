//! RON-based authoring format for dialogue scripts.
//!
//! the format mirrors the runtime structure: blocks are keyed by string IDs
//! which the parser resolves to integer indices, matching what [`ScriptBuilder`]
//! does at runtime. characters are referenced by their u32 index — register them
//! with [`DialogueManager::add_character`] before parsing.
//!
//! # format
//!
//! ```ron
//! Script(
//!     start: "greeting",
//!     blocks: {
//!         "greeting": (character: 1, emotion: 0, text: "hello!", next: Some("farewell")),
//!         "farewell": (
//!             character: 1,
//!             emotion: 0,
//!             text: "what would you like to do?",
//!             choices: [
//!                 (label: "leave",    target: "end"),
//!                 (label: "ask more", target: "more"),
//!             ],
//!         ),
//!         "more": (character: 1, emotion: 0, text: "the world is full of wonders.", next: Some("farewell")),
//!         "end":  (character: 1, emotion: 0, text: "safe travels!"),
//!     },
//! )
//! ```

use rustc_hash::FxHashMap as HashMap;

use serde::Deserialize;

use crate::dialogue::{Block, Choice, Next, Script};

#[derive(Debug, Deserialize)]
#[serde(rename = "Script")]
struct RawScript {
	start: String,
	blocks: HashMap<String, RawBlock>,
}

#[derive(Debug, Deserialize)]
struct RawBlock {
	#[serde(default)]
	character: u32,
	#[serde(default)]
	emotion: u16,
	text: String,
	#[serde(default)]
	next: Option<String>,
	#[serde(default)]
	choices: Vec<RawChoice>,
}

#[derive(Debug, Deserialize)]
struct RawChoice {
	label: String,
	target: String,
}

/// parse a RON script string into a [`Script`].
///
/// validates that the start block exists, all `next` references point to declared
/// blocks, and all choice targets are valid.
///
/// # Errors
/// returns an error string if the RON is malformed or any reference is invalid.
pub fn parse_script(source: &str) -> Result<Script, String> {
	let raw: RawScript = ron::from_str(source).map_err(|e| format!("ron parse error: {e}"))?;

	if !raw.blocks.contains_key(&raw.start) {
		return Err(format!("start block '{}' does not exist", raw.start));
	}

	// assign a stable order for the blocks (sorted by key for determinism)
	let mut ordered: Vec<(&str, &RawBlock)> =
		raw.blocks.iter().map(|(k, v)| (k.as_str(), v)).collect();
	ordered.sort_by_key(|(id, _)| *id);

	let id_to_index: HashMap<&str, u32> = ordered
		.iter()
		.enumerate()
		.map(|(i, (id, _))| (*id, i as u32))
		.collect();

	let start = *id_to_index
		.get(raw.start.as_str())
		.ok_or_else(|| format!("start block '{}' not found after ordering", raw.start))?;

	let blocks: Result<Vec<Block>, String> = ordered
		.iter()
		.map(|(id, raw_block)| {
			if let Some(ref next_id) = raw_block.next
				&& !raw.blocks.contains_key(next_id)
			{
				return Err(format!(
					"block '{id}' references unknown next block '{next_id}'"
				));
			}

			let next = if !raw_block.choices.is_empty() {
				let choices: Result<Vec<Choice>, String> = raw_block
					.choices
					.iter()
					.map(|c| {
						let target = *id_to_index.get(c.target.as_str()).ok_or_else(|| {
							format!("choice in '{id}' targets unknown block '{}'", c.target)
						})?;
						Ok(Choice {
							label: c.label.as_str().into(),
							target,
						})
					})
					.collect();
				Next::Choice(choices?.into_boxed_slice())
			} else if let Some(ref next_id) = raw_block.next {
				let target = *id_to_index
					.get(next_id.as_str())
					.ok_or_else(|| format!("block '{id}' references unknown next '{next_id}'"))?;
				Next::Line(target)
			} else {
				Next::End
			};

			Ok(Block {
				character: raw_block.character,
				emotion: raw_block.emotion,
				text: raw_block.text.as_str().into(),
				next,
			})
		})
		.collect();

	Ok(Script {
		blocks: blocks?.into_boxed_slice(),
		start,
	})
}

/// parse a RON script file from disk.
///
/// # Errors
/// returns an error if the file cannot be read or contains invalid content.
pub fn parse_script_file(path: &str) -> Result<Script, String> {
	let source = std::fs::read_to_string(path)
		.map_err(|e| format!("failed to read script file '{path}': {e}"))?;
	parse_script(&source)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::dialogue::Next;

	#[test]
	fn parse_simple_script() {
		let ron = r##"
Script(
    start: "greeting",
    blocks: {
        "greeting": (character: 1, emotion: 0, text: "hello!", next: Some("end")),
        "end": (character: 0, text: "bye!"),
    },
)
"##;
		let script = parse_script(ron).expect("should parse");
		assert_eq!(script.blocks.len(), 2);
	}

	#[test]
	fn parse_script_with_choices() {
		let ron = r##"
Script(
    start: "question",
    blocks: {
        "question": (
            character: 1,
            emotion: 0,
            text: "what do you want?",
            choices: [
                (label: "yes", target: "yes_path"),
                (label: "no",  target: "no_path"),
            ],
        ),
        "yes_path": (character: 1, emotion: 0, text: "you said yes!"),
        "no_path":  (character: 1, emotion: 0, text: "you said no!"),
    },
)
"##;
		let script = parse_script(ron).expect("should parse");
		let start_block = &script.blocks[script.start as usize];
		assert!(matches!(start_block.next, Next::Choice(_)));
		if let Next::Choice(ref choices) = start_block.next {
			assert_eq!(choices.len(), 2);
			assert_eq!(choices[0].label.as_ref(), "yes");
		}
	}

	#[test]
	fn invalid_next_fails() {
		let ron = r##"
Script(
    start: "a",
    blocks: { "a": (character: 0, text: "hi", next: Some("nonexistent")) },
)
"##;
		assert!(parse_script(ron).is_err());
	}

	#[test]
	fn invalid_choice_target_fails() {
		let ron = r##"
Script(
    start: "a",
    blocks: {
        "a": (character: 0, text: "hi", choices: [(label: "go", target: "nowhere")]),
    },
)
"##;
		assert!(parse_script(ron).is_err());
	}

	#[test]
	fn invalid_start_fails() {
		let ron = r##"
Script(
    start: "nonexistent",
    blocks: { "a": (character: 0, text: "hello") },
)
"##;
		assert!(parse_script(ron).is_err());
	}

	#[test]
	fn narrator_block_character_zero() {
		let ron = r##"
Script(
    start: "narration",
    blocks: {
        "narration": (character: 0, text: "the wind howls...", next: Some("end")),
        "end": (character: 0, text: "the end."),
    },
)
"##;
		let script = parse_script(ron).expect("should parse");
		let start_block = &script.blocks[script.start as usize];
		assert_eq!(start_block.character, 0);
	}
}
