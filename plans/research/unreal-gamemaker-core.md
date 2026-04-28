# Unreal Engine — Actor/Component & Core Architecture Research

## Actor/Component Model

Unreal uses a component-based architecture similar to Unity but with key differences in how components interact with the engine.

### Key Concepts

**Actor & Component Hierarchy**
- `AActor` is the base class for all game objects that can be placed in a level
- `UActorComponent` adds behavior to actors
- `USceneComponent` adds transform (position/rotation/scale) and parent/child relationships
- Components are owned by actors, actors exist in levels
- `RootComponent` is the primary scene component; all other components attach to it

**Tick System**
- Actors and components can opt into ticking via `bCanEverTick`
- Tick groups for ordering: PrePhysics, StartFrame, DuringPhysics, EndFrame, PostPhysics
- Tick intervals can be throttled for performance
- Event-driven alternatives avoid per-frame ticking

**Property System & Reflection**
- `UPROPERTY()` macro enables serialization, replication, editor exposure
- `UFUNCTION()` enables RPC calls, editor buttons, event binding
- `UCLASS()` enables reflection, garbage collection, blueprint exposure
- Properties are introspectable at runtime

## Core Systems (Rendering-Agnostic)

**Gameplay Framework**
- `GameMode` defines rules, scoring, win conditions
- `PlayerController` handles player input and camera
- `Pawn` is the physical representation of a player/AI in the world
- `GameState` replicates game state to all clients
- `PlayerState` holds per-player data (score, name, team)

**Event System**
- `UFUNCTION(BlueprintImplementableEvent)` — blueprint-only events
- `UFUNCTION(BlueprintNativeEvent)` — C++ default + blueprint override
- Delegates for type-safe callbacks (single, multicast, dynamic)
- Event dispatchers on actors for custom events

**Data Assets**
- `UDataAsset` for editor-editable data objects
- `UDataTable` for CSV/JSON import
- `UEnum` for named integer values
- `UStruct` for complex data types

**Subsystem Architecture**
- `UGameInstanceSubsystem` — persists across level loads
- `UWorldSubsystem` — tied to a specific world
- `ULocalPlayerSubsystem` — per-player data
- `UEngineSubsystem` — engine-wide singleton

## Lessons for Lunar

### What to Adopt

1. **Gameplay framework** — Unreal's GameMode/PlayerController/Pawn separation is excellent for multiplayer but useful for single-player too:
   - `GameMode` → game rules, zone transitions, scene management
   - `PlayerController` → input handling, camera control, UI interaction
   - `Pawn` → the entity the player controls (can swap pawns for vehicles, etc.)

2. **Subsystem pattern** — Unreal's subsystems are cleaner than global singletons. Lunar could use ECS resources as subsystems:
   ```rust
   // Instead of global singletons, use typed resources
   pub struct AudioSubsystem { ... }
   pub struct InputSubsystem { ... }
   pub struct UISubsystem { ... }
   ```

3. **Property reflection** — Unreal's property system enables editor tools, serialization, and networking. Lunar's ECS already has component introspection via bevy_ecs, but could add:
   - Component field names and types at runtime
   - Default value serialization
   - Editor-friendly property display names

4. **Event dispatchers** — Unreal's delegates are type-safe and support multiple subscribers. Lunar's bevy_ecs events serve this purpose but could be extended:
   - Named events (string-based for game code convenience)
   - Event priority ordering
   - Event filtering (only fire if data changed)

5. **Data assets** — Unreal's DataAsset system is perfect for game designers. Lunar's YAML-based dialogue and localization systems follow this pattern. Extend to:
   - Item definitions
   - Enemy stats
   - Level configuration
   - UI layout definitions

### What to Avoid

1. **Macro-heavy reflection** — Unreal's UPROPERTY/UFUNCTION macros are powerful but create a parallel type system. Lunar's derive macros are simpler and more idiomatic Rust.

2. **Garbage collection coupling** — Unreal's GC is tightly coupled to the reflection system. Lunar's ECS handles lifetime via handles and generations.

3. **Blueprint complexity** — Blueprints are powerful but create a parallel codebase. Lunar should keep everything in Rust for consistency.

## Decoupled Architecture Suggestion

```
engine-gameplay (crate)
├── mode/            # GameMode resource (rules, scoring, transitions)
├── controller/      # PlayerController resource (input, camera, UI)
├── pawn/            # Pawn component (physical representation)
├── state/           # GameState/PlayerState resources
├── subsystem/       # Subsystem trait, registration
└── event/           # Named event system, dispatchers

game code uses engine-gameplay through:
- GameMode resource for rules
- PlayerController for input/camera
- Subsystems for engine services
- Events for decoupled communication
```

---

## GameMaker — Room/Object & Rapid Prototyping Research

## Room/Object Model

GameMaker prioritizes rapid prototyping and 2D game development with a simple but effective architecture.

### Key Concepts

**Rooms**
- Rooms are levels/scenes with layered object placement
- Layers: Background, Instance, Tile, Asset
- Room settings: size, speed, views (camera), physics
- Room persistence (objects retain state when leaving/returning)

**Objects & Events**
- Objects have event-driven scripts: Create, Step, Draw, Destroy, Collision, Key, Mouse
- Events are ordered: Create → Step (Begin, Normal, End) → Draw → Destroy
- Parent objects pass events to children
- Object inheritance (single parent)

**Sprites & Tiles**
- Sprites are image sequences with origin points
- Tile sets for efficient background/level rendering
- Sprite animation via image_index and image_speed
- Sprite collision masks (precise, bounding box, diamond)

**Variables & Scoping**
- Instance variables (per-object state)
- Global variables (game-wide state)
- Local variables (script-scoped)
- `with()` statement to execute code in another object's context

**Script Functions**
- Built-in functions for movement, collision, drawing, audio
- `move_towards_point()`, `place_meeting()`, `instance_create()`
- `draw_sprite()`, `draw_text()`, `draw_rectangle()`
- Scripts can be user-defined functions

## Lessons for Lunar

### What to Adopt

1. **Event-driven architecture** — GameMaker's event system is intuitive and maps well to ECS:
   - Create → `OnSpawn` event
   - Step → `OnUpdate` system
   - Draw → `OnRender` system
   - Destroy → `OnDespawn` event
   - Collision → collision detection system

2. **Room layering** — GameMaker's layer system is clean:
   - Background layer (static, parallax)
   - Instance layer (game objects)
   - Tile layer (level geometry)
   - UI layer (overlay, always on top)
   Lunar could implement layer-based rendering with z-ordering.

3. **Parent/child object inheritance** — Single inheritance for objects is simpler than ECS for some cases. Lunar could support entity hierarchies for UI and nested objects.

4. **Rapid prototyping** — GameMaker's strength is getting something on screen quickly. Lunar should offer:
   - Simple entity spawning: `spawn!(entity, (Sprite::new("player.png"), Transform::at(100, 100)))`
   - Built-in collision detection
   - Quick room/scene editor (YAML-based)

5. **`with()` pattern** — Execute code in another entity's context:
   ```rust
   // Instead of GameMaker's with(obj) { ... }
   for entity in query.iter() {
       // operate on each entity
   }
   ```
   Lunar's ECS query system already does this.

### What to Avoid

1. **Global variable proliferation** — GameMaker encourages global variables. Lunar's ECS resources are cleaner.

2. **Object inheritance limits** — Single inheritance is limiting. Lunar's ECS component composition is more flexible.

3. **Tight coupling to GM runtime** — GameMaker games are tightly coupled to the GM runtime. Lunar should keep game code decoupled from engine internals.

## Cross-Engine Summary (Unreal + GameMaker)

### Patterns Worth Adopting

| Pattern | Source | Lunar Application |
|---------|---------------------|
| Gameplay framework | Unreal | GameMode, PlayerController, Pawn resources |
| Subsystem pattern | Unreal | Typed resources for engine services |
| Event dispatchers | Unreal | Named events with priority ordering |
| Data assets | Unreal | YAML-based game data definitions |
| Room layering | GameMaker | Z-ordered rendering layers |
| Event-driven objects | GameMaker | OnSpawn/OnUpdate/OnDespawn events |
| Rapid prototyping | GameMaker | Simple spawn macros, built-in collision |
| Parent/child hierarchies | Both | Entity hierarchies for UI and nested objects |

### Lunar-Specific Recommendations

1. **Add Gameplay Framework crate** — `engine-gameplay` with GameMode, PlayerController, Pawn patterns
2. **Add Subsystem trait** — Clean interface for engine services (audio, input, UI)
3. **Add Layer system** — Z-ordered rendering layers (background, game, UI)
4. **Add Entity hierarchies** — Parent/child relationships for UI and nested objects
5. **Keep ECS at the core** — All patterns should map to ECS, not replace it
