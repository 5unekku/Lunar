# Dialogue and Text System

## Goals

Lunar should provide a native, built-in way to manage text and dialogue without requiring games to implement their own solutions. The system should abstract away the technical details and let developers focus on writing content.

**Key goals:**

- **Multi-stage conversations.** Dialogue should support multiple stages of text in sequence, not just single lines.
- **Speaker identification.** Speakers identified by human-readable strings (e.g., `"npc_merchant"`, `"player"`) that are converted to numeric IDs at compile or load time for runtime efficiency.
- **Narrator text.** Support for text with no speaker (key of 0 or null), used for narration, scene descriptions, or internal monologue.
- **Sprite and emotion changes.** Dialogue entries should be able to trigger sprite changes, emotion swaps, or textbox sprite updates during conversation (similar to Final Fantasy or Suikoden static sprite changes during dialogue).
- **Branching choices.** Support for branching dialogue paths based on player choices or game state.
- **No-response options.** Branches that don't require player input — automatic progression or conditional branching without response prompts.
- **Compiled binary format.** Dialogue data should be compiled into an efficient binary format at build time for fast runtime access, avoiding parsing overhead during gameplay.
- **Developer-friendly authoring.** The authoring format should be easy to write and read — whether that's a custom DSL, structured data files, or another approach is to be determined.

## Scope

This system is intended for:
- NPC conversations in RPGs (Deltarune-style)
- Story sequences and cutscenes
- Item descriptions and lore text
- Combat dialogue and barks
- Menu text and UI strings
- Branching narrative content

## Open Questions

The following details are to be determined in future design iterations:
- Authoring format (custom DSL vs. structured data vs. other)
- Runtime data structure and API surface
- Integration with the ECS (dialogue as a component? resource? system?)
- Localization strategy
- Text rendering integration (font system, textbox layout)

---

[← Back to World and Zone Management](05-world-zones.md) | [Next: Asset Pipeline →](07-asset-pipeline.md)
