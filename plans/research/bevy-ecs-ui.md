# Bevy — ECS, UI & Plugin Architecture Research

## ECS Architecture

Bevy is the closest in spirit to Lunar — Rust-based, ECS-first, data-driven.

### Key Concepts

**World & Systems**
- `World` owns all entities, components, resources
- `System` functions run in parallel by default
- `SystemSet` for grouping and ordering
- `Stage` for execution phases (now replaced by `Schedule`)

**Component Design**
- Components are plain data structs (derive `Component`)
- No behavior on components — systems provide behavior
- Bundle pattern for grouping related components
- `With<T>` / `Without<T>` query filters

**Resource Design**
- Resources are singleton data (derive `Resource`)
- Accessed via `Res<T>` / `ResMut<T>` in systems
- `Local<T>` for system-local state
- `NonSend<T>` for non-Send types

## UI System (bevy_ui)

**Widget Components**
- `Node` component for layout (size, position, margins, padding)
- `Style` component for styling (flex direction, justify, align)
- `UiImage`, `UiText` for content
- `BackgroundColor` for backgrounds

**Layout Engine**
- Uses `taffy` (formerly `stretch`) — a flexbox/grid implementation
- Layout computed during `UiSystem::Update` stage
- Automatic size calculation based on content
- Z-ordering via `ZIndex` component

**UI Interaction**
- `Interaction` component tracks hover/press/focus state
- `Button` component adds click behavior
- `FocusPolicy` controls event propagation
- `RelativeTouchForce` for pressure-sensitive input

## Plugin Architecture

**Plugin Trait**
- `Plugin` trait with `build(&mut self, app: &mut App)`
- `PluginGroup` for bundling related plugins
- `DefaultPlugins` includes core, input, render, audio, ui
- Plugin dependencies via `depends_on()`

**App Builder**
- `App` struct with `add_plugin()`, `add_system()`, `init_resource()`
- `Startup` systems run once before main loop
- `Update` systems run every frame
- Custom schedules for custom timing

## Lessons for Lunar UI

### What to Adopt

1. **Component-based UI** — Bevy's UI is just entities with specific components. This fits Lunar's ECS perfectly:
   ```rust
   // UI entity creation
   commands.spawn((
       Node {
           style: Style {
               flex_direction: FlexDirection::Column,
               ..default()
           },
           ..default()
       },
       BackgroundColor(Color::BLACK),
   ));
   ```

2. **Taffy layout engine** — Bevy uses `taffy` for flexbox/grid layout. Lunar could integrate taffy directly:
   - Already pure Rust, no C dependencies
   - WASM compatible
   - Well-maintained, used by multiple projects

3. **Interaction component** — Simple enum-based state tracking:
   ```rust
   pub enum Interaction {
       Pressed,
       Hovered,
       None,
   }
   ```
   Game code queries `Query<&Interaction, Changed<Interaction>>` to react.

4. **Plugin groups** — Bevy's `DefaultPlugins` pattern is clean. Lunar could have:
   ```rust
   app.add_plugin(DefaultPlugins)
      .add_plugin(MyGamePlugin);
   ```

5. **Bundle pattern** — Group related components for common patterns:
   ```rust
   #[derive(Bundle)]
   struct ButtonBundle {
       button: Button,
       node: Node,
       interaction: Interaction,
       background: BackgroundColor,
   }
   ```

### What to Avoid

1. **Bevy's UI rebuild overhead** — Bevy recomputes layout every frame for changed nodes. Lunar should only recompute when style/size changes.

2. **Complex query syntax** — Bevy's query system is powerful but verbose. Lunar's `query!` macro is a good start but should be kept simple.

3. **Plugin coupling** — Bevy plugins can depend on each other in complex ways. Lunar's simpler `GamePlugin` trait is better for keeping things decoupled.

## Decoupled Architecture Suggestion

```
lunar-ui (crate)
├── node/            # Node component, Style component
├── layout/          # taffy integration, layout computation
├── interaction/     # Interaction component, hover/press tracking
├── widget/          # Button, Label, Image bundles
├── focus/           # Focus management, tab navigation
└── render/          # UI → DrawCommand conversion

game code uses lunar-ui through:
- Entity spawning with UI bundles
- Query<&Interaction, Changed<Interaction>> for events
- No direct coupling to render — UI entities produce DrawCommands
```

The key insight: Bevy proves that UI can be pure ECS — no special UI system needed, just components and systems. Lunar should follow this pattern.
