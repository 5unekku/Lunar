# Unity — UI Architecture Research

## Component Model & Inspector Workflow

Unity's core strength is its component-based architecture where every game object is a container for components. The inspector provides immediate visual feedback during development.

### Key Concepts

**Component Composition**
- GameObjects are empty containers; behavior comes from attached components
- Transform component is implicit — every GameObject has position/rotation/scale
- Components communicate via `GetComponent<T>()` or serialized references
- Prefabs allow reusable component hierarchies with override support

**Inspector-Driven Workflow**
- All component properties are editable in real-time during edit mode
- Custom inspectors via `[CustomEditor]` attribute
- Property drawers for custom type visualization
- Undo/redo system integrated at the serialization level

**Serialization**
- `[SerializeField]` exposes private fields to inspector
- ScriptableObjects for data-only assets (config, stats, dialogue)
- Prefab system with nested prefab support and override tracking

## UI System (uGUI / UI Toolkit)

**uGUI (Canvas-based)**
- Canvas acts as a rendering boundary — all UI elements must be children
- RectTransform replaces Transform for 2D layout (anchors, pivots, margins)
- Layout components (HorizontalLayoutGroup, VerticalLayoutGroup, GridLayoutGroup) auto-size children
- EventSystem handles input routing to UI elements
- Raycaster determines which UI element receives events

**UI Toolkit (newer, IMGUI-inspired)**
- UXML for declarative UI structure (like HTML)
- USS for styling (like CSS)
- VisualElement tree with flexbox-like layout system
- Immediate mode for editor tools, retained mode for runtime UI
- Event system with bubbling/capturing phases

## Lessons for Lunar UI

### What to Adopt

1. **Component composition over inheritance** — already using ECS, but consider entity hierarchies for UI (parent/child transforms)

2. **Serialized data assets** — ScriptableObject equivalent for game data (dialogue, item stats, localization tables) that can be edited independently of code

3. **Layout system** — flexbox-like layout for UI panels that auto-size and reflow. Lunar's textbox component is a start but needs:
   - Anchors and margins relative to parent
   - Auto-sizing based on content
   - Layout groups (horizontal, vertical, grid)
   - Content size fitter (grow to fit content or clamp)

4. **Event routing** — Unity's EventSystem with raycasters is a clean way to handle UI input without coupling to specific input methods. Lunar's ActionMap could feed into a UI event system.

5. **Declarative UI definition** — UXML/USS approach lets designers build UI without touching code. For Lunar, a YAML or JSON UI definition format could work:
   ```yaml
   panel:
     anchor: top-left
     margin: [10, 10]
     children:
       - text: { id: "dialogue_text", font: "default", size: 16 }
       - button: { id: "choice_1", text: "Continue" }
   ```

### What to Avoid

1. **Canvas overhead** — Unity's Canvas rebuilds entirely when any child changes. Lunar should batch UI rendering but invalidate only changed regions.

2. **GameObject.Find() anti-pattern** — string-based lookups are fragile. Lunar's handle system is better; UI elements should be referenced by typed handles or IDs.

3. **MonoBehaviour lifecycle coupling** — Unity couples component lifecycle to GameObject lifecycle. Lunar's ECS approach is cleaner — UI state should be separate from render state.

## Decoupled Architecture Suggestion

```
engine-ui (crate)
├── layout/          # flexbox-like layout engine
├── widgets/         # built-in widget types (panel, text, button, image)
├── events/          # UI event system (click, hover, focus)
├── theme/           # styling system (colors, fonts, borders)
└── loader/          # load UI from YAML/JSON definitions

game code uses engine-ui through:
- UIHandle<T> for typed widget references
- UI events through bevy_ecs events
- No direct coupling to render crate — UI produces draw commands
```

The UI crate would produce `DrawCommand`s just like game code does, keeping render completely decoupled.
