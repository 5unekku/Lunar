//! dialogue data structures and runtime manager.
//!
//! conversations are stored as flat arrays of [`Block`]s linked by integer indices,
//! like a graph of nodes. each block carries who is speaking, their emotion, the
//! text, and a [`Next`] that describes what comes after — either nothing, a single
//! block, or a set of labelled choices each pointing to a different block.
//!
//! # structure
//!
//! ```text
//! Script
//!   blocks: Box<[Block]>   ← flat array; blocks reference each other by index
//!   start: u32             ← index of first block
//!
//! Block
//!   character: u32         ← 0 = no speaker; 1..n = character index
//!   emotion:   u16         ← index into that character's clip list
//!   text:      Box<str>
//!   next:      Next
//!
//! Next::End
//! Next::Line(u32)          ← advance to this block
//! Next::Choice(Box<[Choice]>)
//!   Choice { label: Box<str>, target: u32 }
//!     labels[i] displayed, selecting i jumps to links[i].target
//! ```
//!
//! # building scripts
//!
//! use [`ScriptBuilder`] — assign string IDs to blocks during construction,
//! they are resolved to integer indices when [`ScriptBuilder::build`] is called.
//!
//! ```ignore
//! let npc = manager.add_character("old man");
//!
//! let script = ScriptBuilder::new("greeting")
//!     .block("greeting", npc, 0, "hello there!", Some("farewell"))
//!     .block("farewell", npc, 0, "safe travels.", None)
//!     .build()
//!     .unwrap();
//!
//! manager.register("npc1", script);
//! manager.start("npc1");
//! ```

use rustc_hash::FxHashMap as HashMap;

use bevy_ecs::prelude::*;
use lunar_core::App;

/// a named character that can speak in a dialogue.
///
/// registered with [`DialogueManager::add_character`], referenced in blocks by index.
/// index `0` is reserved — use it to indicate no speaker (narrator text).
pub struct Character {
	pub name: Box<str>,
}

/// a single step in a conversation.
pub struct Block {
	/// 0 = no speaker (narrator). 1..n = index from [`DialogueManager::add_character`].
	pub character: u32,
	/// index into the character's emotion/animation clip list.
	pub emotion: u16,
	pub text: Box<str>,
	pub next: Next,
}

/// what follows a [`Block`] after it is read.
pub enum Next {
	/// the conversation ends here.
	End,
	/// advance unconditionally to this block index.
	Line(u32),
	/// present the player with labelled choices; `choices[i].target` is where choice `i` leads.
	Choice(Box<[Choice]>),
}

/// one branch in a [`Next::Choice`].
pub struct Choice {
	/// text shown to the player.
	pub label: Box<str>,
	/// block index to jump to when this choice is selected.
	pub target: u32,
}

/// a complete conversation script with a defined entry point.
///
/// build with [`ScriptBuilder`] or [`parse_script`], then register with [`DialogueManager`].
pub struct Script {
	pub blocks: Box<[Block]>,
	pub start: u32,
}

// ── builder ──────────────────────────────────────────────────────────────────

struct BuildEntry {
	id: String,
	character: u32,
	emotion: u16,
	text: String,
	next: BuildNext,
}

enum BuildNext {
	End,
	Goto(String),
	Choice(Vec<(String, String)>),
}

/// builds a [`Script`] using human-friendly string IDs for blocks.
///
/// string IDs are resolved to integer indices when [`build`](ScriptBuilder::build)
/// is called, so forward references work — you can reference a block before it is
/// declared as long as it exists by the time `build()` runs.
pub struct ScriptBuilder {
	start: String,
	entries: Vec<BuildEntry>,
}

impl ScriptBuilder {
	/// start a new script; `start` is the string ID of the first block to show.
	#[must_use]
	pub fn new(start: &str) -> Self {
		Self {
			start: start.to_string(),
			entries: Vec::new(),
		}
	}

	/// add a block that auto-advances to `next`, or ends if `next` is `None`.
	#[must_use]
	pub fn block(
		mut self,
		id: &str,
		character: u32,
		emotion: u16,
		text: &str,
		next: Option<&str>,
	) -> Self {
		self.entries.push(BuildEntry {
			id: id.to_string(),
			character,
			emotion,
			text: text.to_string(),
			next: next.map_or(BuildNext::End, |n| BuildNext::Goto(n.to_string())),
		});
		self
	}

	/// add a block that presents choices; `choices` is a list of `(label, target_id)` pairs.
	#[must_use]
	pub fn choice(
		mut self,
		id: &str,
		character: u32,
		emotion: u16,
		text: &str,
		choices: Vec<(&str, &str)>,
	) -> Self {
		self.entries.push(BuildEntry {
			id: id.to_string(),
			character,
			emotion,
			text: text.to_string(),
			next: BuildNext::Choice(
				choices
					.into_iter()
					.map(|(label, target)| (label.to_string(), target.to_string()))
					.collect(),
			),
		});
		self
	}

	/// resolve all string IDs to integer indices and produce a [`Script`].
	///
	/// # Errors
	/// returns an error if the start ID or any `next`/choice target does not match a declared block,
	/// or if two blocks share the same id.
	pub fn build(self) -> Result<Script, String> {
		let mut id_to_index: HashMap<&str, u32> = HashMap::default();
		for (i, entry) in self.entries.iter().enumerate() {
			if id_to_index.insert(entry.id.as_str(), i as u32).is_some() {
				return Err(format!("duplicate block id '{}'", entry.id));
			}
		}

		let start = *id_to_index
			.get(self.start.as_str())
			.ok_or_else(|| format!("start block '{}' not found", self.start))?;

		let blocks: Result<Vec<Block>, String> = self
			.entries
			.iter()
			.map(|entry| {
				let next = match &entry.next {
					BuildNext::End => Next::End,
					BuildNext::Goto(target) => {
						let idx = id_to_index.get(target.as_str()).ok_or_else(|| {
							format!("block '{}' references unknown target '{target}'", entry.id)
						})?;
						Next::Line(*idx)
					}
					BuildNext::Choice(choices) => {
						let resolved: Result<Vec<Choice>, String> = choices
							.iter()
							.map(|(label, target)| {
								let idx = id_to_index.get(target.as_str()).ok_or_else(|| {
									format!(
										"choice in '{}' targets unknown block '{target}'",
										entry.id
									)
								})?;
								Ok(Choice {
									label: label.as_str().into(),
									target: *idx,
								})
							})
							.collect();
						Next::Choice(resolved?.into_boxed_slice())
					}
				};
				Ok(Block {
					character: entry.character,
					emotion: entry.emotion,
					text: entry.text.as_str().into(),
					next,
				})
			})
			.collect();

		Ok(Script {
			blocks: blocks?.into_boxed_slice(),
			start,
		})
	}
}

// ── runtime ───────────────────────────────────────────────────────────────────

struct ActiveDialogue {
	key: String,
	current: u32,
}

/// dialogue manager resource.
///
/// holds the character registry, all registered scripts, and the active
/// conversation state. access via [`ResMut<DialogueManager>`] from systems.
#[derive(Resource)]
pub struct DialogueManager {
	/// index 0 is reserved — `character(0)` always returns `None`.
	characters: Vec<Character>,
	scripts: HashMap<String, Script>,
	active: Option<ActiveDialogue>,
}

impl DialogueManager {
	#[must_use]
	pub fn new() -> Self {
		Self {
			characters: vec![Character { name: "".into() }],
			scripts: HashMap::default(),
			active: None,
		}
	}

	/// register a named character, returns the index to use in [`ScriptBuilder::block`].
	pub fn add_character(&mut self, name: &str) -> u32 {
		let index = self.characters.len() as u32;
		self.characters.push(Character { name: name.into() });
		index
	}

	/// look up a character by index. returns `None` for index 0 (no speaker).
	#[must_use]
	pub fn character(&self, index: u32) -> Option<&Character> {
		if index == 0 {
			return None;
		}
		self.characters.get(index as usize)
	}

	/// register a script under a name for later use with [`start`](Self::start).
	pub fn register(&mut self, name: &str, script: Script) {
		self.scripts.insert(name.to_string(), script);
		log::info!("DialogueManager: registered script '{name}'");
	}

	/// begin a conversation by name.
	pub fn start(&mut self, name: &str) {
		if let Some(script) = self.scripts.get(name) {
			self.active = Some(ActiveDialogue {
				key: name.to_string(),
				current: script.start,
			});
			log::info!("DialogueManager: started '{name}'");
		} else {
			log::warn!("DialogueManager: script '{name}' not found");
		}
	}

	/// the block currently being shown, or `None` if no dialogue is active.
	#[must_use]
	pub fn current_block(&self) -> Option<&Block> {
		let active = self.active.as_ref()?;
		self.scripts
			.get(&active.key)?
			.blocks
			.get(active.current as usize)
	}

	/// advance past a non-choice block. ends the conversation if the block has no next.
	pub fn advance(&mut self) {
		let next = self
			.active
			.as_ref()
			.and_then(|a| self.scripts.get(&a.key))
			.and_then(|s| s.blocks.get(self.active.as_ref().unwrap().current as usize))
			.map(|b| match &b.next {
				Next::Line(idx) => Some(*idx),
				_ => None,
			});

		match next {
			Some(Some(idx)) => self.active.as_mut().unwrap().current = idx,
			_ => self.active = None,
		}
	}

	/// select choice `index` and jump to its target block.
	pub fn choose(&mut self, index: usize) {
		let target = self
			.active
			.as_ref()
			.and_then(|a| self.scripts.get(&a.key))
			.and_then(|s| s.blocks.get(self.active.as_ref().unwrap().current as usize))
			.and_then(|b| match &b.next {
				Next::Choice(choices) => choices.get(index).map(|c| c.target),
				_ => None,
			});

		if let Some(target) = target {
			self.active.as_mut().unwrap().current = target;
		}
	}

	/// whether the current block is a choice block.
	#[must_use]
	pub fn has_choices(&self) -> bool {
		matches!(self.current_block().map(|b| &b.next), Some(Next::Choice(_)))
	}

	/// the labels for the current choices, in order.
	#[must_use]
	pub fn choice_labels(&self) -> Vec<&str> {
		match self.current_block().map(|b| &b.next) {
			Some(Next::Choice(choices)) => choices.iter().map(|c| c.label.as_ref()).collect(),
			_ => Vec::new(),
		}
	}

	/// whether a dialogue is currently active.
	#[must_use]
	pub fn is_active(&self) -> bool {
		self.active.is_some()
	}

	/// close the active dialogue immediately.
	pub fn close(&mut self) {
		self.active = None;
	}
}

impl Default for DialogueManager {
	fn default() -> Self {
		Self::new()
	}
}

/// dialogue plugin, registers [`DialogueManager`] as an ECS resource.
pub struct DialoguePlugin;

impl lunar_core::GamePlugin for DialoguePlugin {
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
