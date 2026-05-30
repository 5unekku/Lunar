# engine_dialogue

dialogue system for lunar.

provides multi-stage conversations, speaker identification, branching
choices, and narrator text. extracted from `lunar-core` so games that
don't need it pay zero compile cost.

# quick start

```ignore
use engine_dialogue::{DialogueBuilder, DialogueManager, DialoguePlugin};
use engine_core::App;

let mut app = App::new();
app.add_plugin(DialoguePlugin);

let dialogue = DialogueBuilder::new("start")
    .line("greeting", Some("NPC"), "hello there!", Some("farewell"))
    .build();
```

## Re-exports
- Dialogue = dialogue::Dialogue — a complete conversation or dialogue tree.
- DialogueBuilder = dialogue::DialogueBuilder — a builder for constructing dialogue trees.
- DialogueChoice = dialogue::DialogueChoice — a branching choice in a dialogue.
- DialogueLine = dialogue::DialogueLine — a dialogue line with optional speaker and choices.
- DialogueManager = dialogue::DialogueManager — dialogue manager resource.
- DialogueNode = dialogue::DialogueNode — a named dialogue node within a conversation.
- DialoguePlugin = dialogue::DialoguePlugin — dialogue plugin, registers the dialogue manager resource.
- DialogueState = dialogue::DialogueState — dialogue state for an active conversation.
- parse_dialogue = parser::parse_dialogue — parse a RON dialogue string into a [`Dialogue`](crate::dialogue::Dialogue).
- parse_dialogue_file = parser::parse_dialogue_file — parse a dialogue file from disk.

## Structs

### Dialogue

a complete conversation or dialogue tree.

contains all nodes and a [`start`](Dialogue::start) entry point.
use [`DialogueBuilder`] to construct this fluently.

### DialogueBuilder

a builder for constructing dialogue trees.

provides a fluent interface for adding lines and choices.
call [`build`](DialogueBuilder::build) when finished to get a [`Dialogue`].

### DialogueChoice

a branching choice in a dialogue.

presents the player with an option that leads to a different dialogue node.

### DialogueLine

a dialogue line with optional speaker and choices.

represents a single line of text in a conversation.
if [`speaker`](DialogueLine::speaker) is `None`, the text is treated as narrator text.

### DialogueManager

dialogue manager resource.

stores registered dialogues and manages the active conversation state.
access this resource from systems to start, advance, and close dialogues.

### DialogueNode

a named dialogue node within a conversation.

each node represents a single step in the dialogue graph.
if [`next`](DialogueNode::next) is set and there are no choices,
the dialogue auto-advances to that node.

### DialoguePlugin

dialogue plugin, registers the dialogue manager resource.

add this plugin to your [`App`] to enable the dialogue system.

### DialogueState

dialogue state for an active conversation.

tracks the current node and whether the dialogue is still active.
managed internally by the [`DialogueManager`].

## Functions

### parse_dialogue

parse a RON dialogue string into a [`Dialogue`](crate::dialogue::Dialogue).

validates that the start node exists, all `next` references are valid,
and all choice targets point to existing nodes.
returns an error if the yaml is malformed or references invalid nodes.

# Errors
returns an error if the yaml is invalid or contains references to non-existent nodes.

### parse_dialogue_file

parse a dialogue file from disk.

reads the file at the given path and parses it as yaml.
returns an error if the file can't be read or contains invalid content.

# Errors
returns an error if the file cannot be read or if its contents are invalid.
