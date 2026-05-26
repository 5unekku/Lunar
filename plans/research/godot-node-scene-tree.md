# Godot — Node/Scene Tree & UI System Research

## Node/Scene Tree Model

Godot's entire engine is built around a tree of nodes. Every object in a Godot project is a Node or inherits from Node.

### Key Concepts

**Tree Hierarchy**
- SceneTree is the root; nodes form parent/child relationships
- Nodes have `_ready()`, `_process()`, `_input()` lifecycle methods
- Parent nodes can access children via `$Path` or `get_node()`
- Signals (Godot's event system) decouple nodes from each other

**Scene System**
- `.tscn` files are text-based scene definitions (human-readable, git-friendly)
- Scenes can be instanced into other scenes
- Inherited scenes allow specialization (base scene → derived scene with overrides)
- PackedScene for runtime scene instantiation

**Signals**
- Type-safe event system: `signal pressed`, `emit_signal("pressed")`
- Can connect any signal to any method: `button.pressed.connect(self._on_pressed)`
- Built-in signals on most nodes (pressed, text_changed, mouse_entered, etc.)
- No coupling — sender doesn't know who receives signals

## UI System (Control Nodes)

**Control Node Hierarchy**
- All UI elements inherit from Control
- Automatic parent/child sizing: children can anchor to parent edges
- Size flags: Fill, Expand, Shrink Center/Begin/End
- Focus system for gamepad/keyboard navigation

**Container Nodes**
- VBoxContainer, HBoxContainer, GridContainer, MarginContainer, TabContainer
- Containers auto-layout children based on rules
- Custom containers via `_get_minimum_size()` and `_notification()`

**Theme System**
- Theme resource defines colors, fonts, fontsizes, icons, styles
- Can be applied globally or per-node
- StyleBox for backgrounds (flat, textured, line)
- Font variations (bold, italic) from single font file

**UI Input**
- InputEvent system handles mouse, keyboard, gamepad, touch
- `_gui_input(event)` for per-node input handling
- `accept_event()` to stop propagation
- Focus management with `grab_focus()`, `release_focus()`

## Lessons for Lunar UI

### What to Adopt

1. **Scene tree as data** — Godot's `.tscn` format is brilliant. Lunar could use YAML scene files:
   ```yaml
   scene: "main_menu"
   root:
     type: Panel
     children:
       - type: VBoxContainer
         children:
           - type: Button
             text: "Start Game"
             on_pressed: start_game
           - type: Button
             text: "Quit"
             on_pressed: quit
   ```

2. **Signal system** — Godot's signals are a clean decoupling mechanism. Lunar's bevy_ecs events serve a similar purpose but could be extended for UI-specific signals:
   - `UIPressed`, `UIHovered`, `UIFocused` events
   - Game code subscribes to UI events without knowing about UI internals

3. **Container-based layout** — Godot's container nodes are simpler than flexbox but more intuitive for game UI. Lunar should implement:
   - `VBoxContainer` — stack children vertically
   - `HBoxContainer` — stack children horizontally
   - `GridContainer` — fixed column count, auto-rows
   - `MarginContainer` — padding around single child
   - `CenterContainer` — center single child

4. **Theme resource** — centralized styling that can be swapped at runtime. Lunar's localization system already uses a similar pattern (per-language string tables). A Theme resource could work the same way:
   ```rust
   pub struct Theme {
       pub colors: HashMap<String, Color>,
       pub fonts: HashMap<String, Handle<Font>>,
       pub font_sizes: HashMap<String, u32>,
       pub style_boxes: HashMap<String, StyleBox>,
   }
   ```

5. **Size flags** — simple enum-based sizing instead of complex CSS-like rules:
   - `Fill` — take available space
   - `Expand` — grow to fill parent
   - `ShrinkBegin` — shrink to content, align to start
   - `ShrinkCenter` — shrink to content, center

### What to Avoid

1. **Node proliferation** — Godot has 100+ node types. Lunar should keep widget types minimal and extensible via the RenderPass trait.

2. **Scene coupling to code** — Godot scenes often reference specific script methods. Lunar should use event-based communication instead.

3. **_process() coupling** — Godot couples update logic to node lifecycle. Lunar's ECS systems are cleaner — UI update should be a system, not a per-widget method.

## Decoupled Architecture Suggestion

```
lunar-ui (crate)
├── scene/           # YAML scene loading, instancing
├── container/       # VBoxContainer, HBoxContainer, etc.
├── widget/          # Button, Label, TextureRect, etc.
├── signal/          # UI event types (pressed, changed, focused)
├── theme/           # Theme resource, style system
└── focus/           # Focus management, gamepad navigation

game code uses lunar-ui through:
- SceneHandle for loaded UI scenes
- UI events via bevy_ecs EventReader
- Theme resource for styling
- No direct coupling to render — UI produces DrawCommands
```

The key insight: Godot's UI system works because containers handle layout automatically, signals decouple behavior, and themes centralize styling. Lunar can adopt these patterns while keeping the ECS architecture.
