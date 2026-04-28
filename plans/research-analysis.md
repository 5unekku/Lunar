# Research Analysis — Engine Recommendations

> Synthesized from 5 research documents covering Bevy, Godot, Unity, Unreal, GameMaker, libGDX, Pygame, and LÖVE2D.
> Date: 2026-04-28
> Focus: Performance-first recommendations for Lunar engine
> Status: Design phase — concepts need careful adaptation to Rust/ECS constraints

---

## Executive Summary

The research reveals **convergent patterns** across all engines studied. The recommendations below are organized by priority, with performance as the primary filter. Each recommendation notes what to **adopt**, what to **avoid**, and the **performance implications**.

**Important:** We are in a design phase. These concepts cannot be ripped directly from source engines — they must be carefully adapted to Lunar's constraints (Rust, ECS, WASM target, performance-first, no user-side complexity).

---

## 1. Texture Atlas System (HIGH PRIORITY)

**Sources:** libGDX (TextureAtlas), Bevy (glyph atlas), Lunar TODO (#110)

### Current State
- Glyph atlas exists in `engine-render/src/text.rs`
- No general-purpose texture atlas for game sprites
- Each texture load creates a separate GPU texture → texture switch per draw call

### Recommendation
Add a `TextureAtlas` resource that packs multiple sprites into a single GPU texture.

```rust
pub struct TextureAtlas {
    handle: Handle<Texture>,      // the packed GPU texture
    regions: HashMap<String, Rect>, // named sub-rectangles
}
```

### Performance Impact
- **Draw call reduction:** 1 draw call per atlas instead of 1 per sprite
- **GPU memory:** Fewer texture objects = less driver overhead
- **Batching:** Current batching groups by texture ID; atlas makes more sprites batchable

### Implementation Notes
- Use a bin-packing algorithm (shelf packing or maxrects)
- Atlas builder runs offline or at load time
- `Sprite` component gains an optional `atlas_region: Option<Rect>` field
- Existing single-texture path still works for non-atlas sprites

---

## 2. UI System — ECS-Based with Taffy Layout (HIGH PRIORITY)

**Sources:** Bevy (bevy_ui + taffy), Godot (Control nodes), Unity (uGUI)

### Current State
- Textbox component exists (`engine-render/src/textbox.rs`)
- No general UI layout system
- No button, panel, or container widgets
- No focus management or input routing for UI

### Recommendation
Create `engine-ui` crate with:

```
engine-ui/
├── node/          # Node + Style components (ECS)
├── layout/        # Taffy integration
├── widget/        # Button, Label, Panel bundles
├── interaction/   # Hover/press/focus tracking
└── events/        # UI event types (pressed, changed)
```

### Performance Considerations
- **Lazy layout recomputation:** Only recompute when style/content changes, NOT every frame (avoid Bevy's per-frame layout rebuild)
- **Dirty region tracking:** Mark only changed nodes for re-layout
- **Taffy is pure Rust:** No C deps, WASM compatible, well-maintained
- **UI produces DrawCommands:** Decoupled from render crate — UI entities generate draw commands just like game code

### What to Avoid
- Bevy's per-frame layout recomputation for unchanged nodes
- Complex query syntax — keep the `query!` macro simple
- Plugin coupling — UI should be a clean crate, not entangled with engine-core

---

## 3. Rendering Layer System (MEDIUM PRIORITY)

**Sources:** GameMaker (layers), Unity (Canvas z-ordering), Unreal (render passes)

### Current State
- Single render pass with command queue
- Custom render passes exist (`RenderPass` trait) but no built-in layering
- No z-ordering for game objects

### Recommendation
Add a `Layer` component and z-ordered rendering:

```rust
#[derive(Component)]
pub struct Layer {
    pub order: i32,  // render order (lower = drawn first)
}

// Built-in layer constants
pub mod layers {
    pub const BACKGROUND: i32 = 0;
    pub const GAME: i32 = 100;
    pub const FOREGROUND: i32 = 200;
    pub const UI: i32 = 300;
}
```

### Performance Impact
- **Sort once per frame:** Sort draw commands by layer before batching
- **Minimal overhead:** Integer comparison, stable sort
- **Enables parallax:** Different layers can have different camera offsets

### Implementation Notes
- `RenderQueue` sorts commands by layer before submission
- Default layer = 0 for backward compatibility
- Camera can have per-layer offset for parallax

---

## 4. Entity Hierarchies / Parent-Child Transforms (MEDIUM PRIORITY — MAYBE)

**Sources:** Godot (node tree), Unity (Transform hierarchy), Unreal (SceneComponent attachment)

### Current State
- ECS is flat — no parent/child relationships
- `Transform` component exists but no hierarchy
- UI would benefit greatly from parent-relative positioning

### Recommendation
Add parent-child transform hierarchy via **composition, NOT inheritance**:

```rust
#[derive(Component)]
pub struct Parent(pub Entity);

#[derive(Component)]
pub struct Children(pub SmallVec<[Entity; 4]>);

// System computes world transforms from local transforms
fn compute_world_transforms(
    mut query: Query<(&Parent, &mut Transform, &LocalTransform)>,
) {
    // propagate parent transforms to children
}
```

**This is NOT an inheritance model.** It's purely transform propagation — entities still compose their behavior from components. Parent/child is an opt-in relationship, not a type hierarchy.

### Performance Impact
- **One system per frame:** O(n) pass over entities with Parent component
- **Cache-friendly:** Sequential iteration over entities
- **UI benefit:** Child widgets auto-position relative to parent panels

### What to Avoid
- Deep hierarchies for game objects — ECS composition is still preferred
- Godot's `_process()` coupling — transform propagation should be a system, not a per-entity method
- Any inheritance semantics — this is purely spatial nesting

---

## 5. Scene Definition Format (MEDIUM PRIORITY — FORMAT TBD)

**Sources:** Godot (.tscn), Unity (prefabs), research docs

### Current State
- No scene file format
- Scenes are defined programmatically via `Scene` trait
- Zones exist but are code-defined

### Recommendation
Add scene definitions. **Format preference: JSON5 > TOML > JSON for authoring.** For runtime distribution, use a **compact binary format** (no parsing overhead at load time).

**Authoring format (JSON5 example):**
```json5
{
  "scene": "main_menu",
  "entities": [
    {
      "id": "background",
      "components": {
        "sprite": { "texture": "bg.png", "layer": 0 },
        "transform": { "x": 0, "y": 0 }
      }
    }
  ]
}
```

**Runtime format:** Binary serialization (e.g., bincode, rkyv, or custom). Zero parsing overhead — memory-map directly into ECS entities.

### Performance Impact
- **Load-time only:** Parsing happens once during asset loading (or compile-time for shipped builds)
- **No runtime overhead:** Scene data becomes regular ECS entities
- **Binary runtime:** No text parsing, direct deserialization

### Implementation Notes
- Scene loader produces entities via `Commands`
- SceneHandle for runtime reference
- Can instance scenes into other scenes (like Godot's packed scenes)
- Compile-time scene bundling: convert JSON5 → binary during asset pipeline

---

## 6. Gameplay Framework Crate (LOW-MEDIUM PRIORITY)

**Sources:** Unreal (GameMode/PlayerController/Pawn), research docs

### Current State
- No gameplay framework
- Game code handles everything manually
- Input, camera, and player entity are not abstracted

### Recommendation
Create `engine-gameplay` crate:

```
engine-gameplay/
├── mode/          # GameMode resource (rules, transitions)
├── controller/    # PlayerController (input routing, camera)
├── pawn/          # Pawn component (physical representation)
└── state/         # GameState/PlayerState resources
```

### Performance Impact
- **Negligible:** These are resources and components, not per-frame systems
- **Clean separation:** GameMode handles zone transitions, PlayerController handles input → camera → UI routing

### Implementation Notes
- Maps cleanly to ECS resources
- PlayerController routes input to focused UI or controlled pawn
- GameMode manages scene/zone transitions and game rules

---

## 7. Rect Utility Extensions (LOW PRIORITY)

**Sources:** Pygame (Rect helpers), libGDX

### Current State
- `Rect` type exists with `contains()`, `intersects()`
- Missing: `inflate()`, `clamp()`, `collidepoint()`, `colliderect()`

### Recommendation
Add utility methods to `Rect`:

```rust
impl Rect {
    pub fn inflate(&self, dx: f32, dy: f32) -> Self;
    pub fn clamp(&self, within: &Rect) -> Self;
    pub fn collide_point(&self, x: f32, y: f32) -> bool;
    pub fn collide_rect(&self, other: &Rect) -> bool;
    pub fn center(&self) -> Vec2;
    pub fn union(&self, other: &Rect) -> Self;
}
```

### Performance Impact
- **None:** Pure utility methods, no allocation

---

## 8. Immediate Mode Render API (LOW PRIORITY)

**Sources:** Pygame (blit), LÖVE2D (love.graphics.draw), libGDX (SpriteBatch)

### Current State
- Command-based render queue (retained mode)
- No immediate mode option

### Recommendation
Offer an optional immediate mode API for simple games or debug drawing:

```rust
render.draw_immediate(|draw| {
    draw.sprite(&texture, pos);
    draw.rect(&rect, color);
    draw.text(&font, "Score: 100", text_pos);
});
```

### Performance Impact
- **Slightly worse than batched:** No inter-frame batching
- **Good for debug:** Debug overlays, simple games, prototyping
- **Optional:** Doesn't affect the main render path

---

## 9. Theme Resource System (LOW-MEDIUM PRIORITY)

**Sources:** Godot (Theme resource), Unity (UI Toolkit USS)

### Current State
- No centralized styling system
- Colors and fonts are set per-widget

### Recommendation
Add a `Theme` resource:

```rust
pub struct Theme {
    pub colors: HashMap<String, Color>,
    pub fonts: HashMap<String, Handle<Font>>,
    pub font_sizes: HashMap<String, u32>,
    pub style_boxes: HashMap<String, StyleBox>,
}
```

### Performance Impact
- **Load-time only:** Theme is a resource, not per-frame
- **Runtime swap:** Can swap themes at runtime for skinning/accessibility
- **Memory efficient:** Shared styling instead of per-widget duplication

---

## 10. Signal/Event System Extension (LOW PRIORITY)

**Sources:** Godot (signals), Unreal (delegates), Unity (events)

### Current State
- Bevy ECS events exist
- No named event system for game code convenience

### Recommendation
Extend the event system with named events:

```rust
pub struct EventBus {
    events: HashMap<String, Box<dyn Any>>,
}

// Game code uses:
events.dispatch("player_died", PlayerDiedEvent { score: 100 });
events.on("player_died", |event: &PlayerDiedEvent| { ... });
```

### Performance Impact
- **Slight overhead:** HashMap lookup + type erasure
- **Convenience over raw ECS events:** Trade some performance for game-dev ergonomics
- **Optional:** Raw ECS events still available for performance-critical paths

---

## Cross-Engine Pattern Summary

| Pattern | Consensus | Lunar Status | Priority |
|-----------|-----------------|----------|
| Texture atlases | Universal | Partial (glyph only) | HIGH |
| Layer-based rendering | GameMaker, Unity | None | HIGH |
| Parent-child transforms | Godot, Unity, Unreal | None | MEDIUM (maybe) |
| Scene files (JSON5 + binary runtime) | Godot, Unity | None | MEDIUM |
| Gameplay framework | Unreal | None | LOW-MEDIUM (maybe) |
| Theme system | Godot, Unity | None | LOW-MEDIUM |
| Rect utilities | Pygame | Partial | LOW |
| Immediate mode | Pygame, LÖVE | None | LOW |
| Named events | Godot, Unreal | Partial (ECS events) | LOW |
| **UI system (Part 2)** | All engines | Partial (textbox) | DEFERRED |

---

## What to AVOID (from research)

1. **Per-frame UI layout rebuild** (Bevy's bevy_ui) — only recompute on change
2. **String-based entity lookup** (Unity's `GameObject.Find()`) — use typed handles
3. **MonoBehaviour lifecycle coupling** (Unity) — keep ECS systems decoupled
4. **Macro-heavy reflection** (Unreal's UPROPERTY) — keep derive macros simple
5. **Node type proliferation** (Godot's 100+ nodes) — keep widget types minimal
6. **Global variable patterns** (GameMaker) — use ECS resources
7. **Canvas full-rebuild on change** (Unity's uGUI) — dirty-region invalidation only
8. **Blueprint parallel codebase** (Unreal) — keep everything in Rust

---

## Recommended Implementation Order (Part 1 — Engine Core)

1. **Texture Atlas** — biggest performance win, foundational for everything else
2. **Layer System** — z-ordered rendering, enables parallax
3. **Entity Hierarchies** — parent-child transforms (composition, NOT inheritance)
4. **Scene Format** — JSON5 authoring + binary runtime
5. **Gameplay Framework** — GameMode/PlayerController/Pawn (when game complexity demands it)
6. **Rect Utilities** — quick win, low effort
7. **Immediate Mode API** — debug/prototyping convenience

## Future Implementation Order (Part 2 — Post-Engine, UI Phase)

> These items depend on the core engine being stable. They are tracked here for planning but are NOT in the current implementation scope.

1. **UI System (engine-ui)** — taffy integration, ECS-based, widget bundles
2. **Theme System** — centralized styling
3. **Named Events** — game-dev ergonomics (optional)
