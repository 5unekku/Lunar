# engine_core

engine core: game loop, ECS wiring, engine state

this crate owns the main game loop and coordinates all subsystems.
game logic operates on handles, never direct references.


## Re-exports
- bevy_ecs = bevy_ecs
- engine_math = engine_math
- App = app::App — app builder for configuring the engine
- GamePlugin = app::GamePlugin — trait for game plugins
- Time = app::Time — time resource updated each frame
- Command = command::Command — a command that can be executed by the engine.
- CommandRegistry = command::CommandRegistry — registry of all available commands.
- Engine = engine::Engine — the engine owns the ECS world and schedules.
- EngineError = error::EngineError — engine error enum covering common failure modes.
- EngineResult = error::EngineResult — convenience result type for engine operations
- ErrorEvent = error::ErrorEvent — an error event that can be read by game systems.
- ErrorSource = error::ErrorSource — source of an error event
- GameLoop = game_loop::GameLoop — game loop configuration and state.
- TickRate = game_loop::TickRate — tick rate buckets based on frame cap.
- Children = hierarchy::Children — component that stores the list of child entities.
- HierarchyPlugin = hierarchy::HierarchyPlugin — plugin that registers hierarchy systems.
- Parent = hierarchy::Parent — component that stores the parent entity reference.
- PostUpdate = hierarchy::PostUpdate — built-in stage for transform propagation (runs after Update, before Render).
- propagate_transforms = hierarchy::propagate_transforms — exclusive system that propagates transforms from parents to children.
- sync_children = hierarchy::sync_children — exclusive system that syncs [`Parent`] and [`Children`] components.
- Scene = scene::Scene — scene trait — implement to define a game scene.
- SceneManager = scene::SceneManager — scene manager resource, manages scene switching and overlays.
- EntityDefinition = scene_format::EntityDefinition — authoring-time entity definition.
- SceneData = scene_format::SceneData — component storing the raw custom data from the scene definition.
- SceneDefinition = scene_format::SceneDefinition — authoring-time scene definition (RON format).
- SceneEntity = scene_format::SceneEntity — marker component for entities spawned from a scene.
- SceneInstance = scene_format::SceneInstance — marker component for entities that instance a sub-scene. the sub-scene's root entities are spawned as children of this entity.
- SceneLayer = scene_format::SceneLayer — component for scene-defined render layer.
- SceneLoader = scene_format::SceneLoader — scene loader: spawns entities from a scene definition.
- SceneSprite = scene_format::SceneSprite — component for scene-defined sprites.
- SceneTags = scene_format::SceneTags — component for scene-defined tags.
- SceneText = scene_format::SceneText — component for scene-defined text.
- SpriteDef = scene_format::SpriteDef — sprite definition for runtime use.
- TextDef = scene_format::TextDef — text definition for runtime use.
- TransformDef = scene_format::TransformDef — transform definition for runtime use.
- StageLabelExt = schedule::StageLabelExt — trait for custom stage labels.
- StageOrder = schedule::StageOrder — relative stage ordering for custom stage placement.
- UpdateStage = schedule::UpdateStage — built-in update stages for system ordering.
- EngineState = state::EngineState — engine running state.
- AdvancedSceneLoader = world_manifest::AdvancedSceneLoader — advanced scene loader supporting multiple load modes.
- ChunkEntry = world_manifest::ChunkEntry — a spatial chunk entry in the world manifest.
- CompiledChunkEntry = world_manifest::CompiledChunkEntry — compiled chunk entry with interned string ids.
- CompiledSceneEntry = world_manifest::CompiledSceneEntry — compiled scene entry with interned string ids.
- CompiledWorld = world_manifest::CompiledWorld — compiled world manifest with interned string ids.
- ComponentScene = world_manifest::ComponentScene — scene definition using the new component map format.
- EntityData = world_manifest::EntityData — entity definition using a named component map.
- LoadMode = world_manifest::LoadMode — how a scene should be loaded relative to current state.
- LoadedScenes = world_manifest::LoadedScenes — resource tracking loaded scenes for unload support.
- SceneEntry = world_manifest::SceneEntry — a scene entry in the world manifest.
- StreamingConfig = world_manifest::StreamingConfig — configuration for the streaming scene loader.
- StreamingState = world_manifest::StreamingState — tracks which chunks are currently loaded for streaming.
- StringInterner = world_manifest::StringInterner — interner that maps strings to u32 identifiers.
- WorldManifest = world_manifest::WorldManifest — root world manifest parsed from XML.
- builtin_components = world_manifest::builtin_components — built-in component names recognized by the engine. game code can define additional components freely.
- WindowSettings = window::WindowSettings — read-only window state resource exposed to game code.

## Modules

### builtin_components

built-in component names recognized by the engine.
game code can define additional components freely.

### prelude

full prelude for game development
prelude for lunar-core — re-exports bevy_ecs essentials and lunar-core's
own types (app/plugin, scenes, hierarchy, world manifest, etc.).

domain crates (`lunar-dialogue`, `lunar-localization`, `lunar-zones`)
and subsystem crates (render, input, assets) must be imported separately.

# example

```ignore
use engine_core::prelude::*;

fn setup(mut commands: Commands) {
    commands.spawn((Transform::default(), Player));
}
```

## Structs

### AdvancedSceneLoader

advanced scene loader supporting multiple load modes.

### App

app builder for configuring the engine

use the app to register systems, resources, and plugins before calling `run()`.

### Children

component that stores the list of child entities.

this is automatically maintained when [`Parent`] components are added/removed.
use the [`Children`] component to iterate over an entity's children.

### ChunkEntry

a spatial chunk entry in the world manifest.

### CommandRegistry

registry of all available commands.

stores commands by name and provides execution and listing interfaces.
the registry is initialized with built-in commands like `help` and `version`.

### CompiledChunkEntry

compiled chunk entry with interned string ids.

### CompiledSceneEntry

compiled scene entry with interned string ids.

### CompiledWorld

compiled world manifest with interned string ids.

### ComponentScene

scene definition using the new component map format.

### Engine

the engine owns the ECS world and schedules.

this is the low-level wrapper around `bevy_ecs`.
most game code should interact with the engine through [`crate::app::App`] instead.

# example

```ignore
use engine_core::Engine;
use engine_core::app::App;

let mut app = App::new();
app.add_plugin(MyGamePlugin);
// Engine wraps the bevy_ecs World and Schedule
// most code interacts through App instead
```

### EntityData

entity definition using a named component map.

instead of hardcoded fields, entities carry a map of component names
to their data. the engine recognizes its own built-in components
and passes unknown components through to game code.

### EntityDefinition

authoring-time entity definition.

### ErrorEvent

an error event that can be read by game systems.

emitted when a recoverable error occurs. game code can
listen for these events and respond accordingly.

### GameLoop

game loop configuration and state.

manages the fixed timestep accumulator and frame rate limiting.
call [`GameLoop::tick`] each frame to get the number of ECS ticks to run.

### HierarchyPlugin

plugin that registers hierarchy systems.

### LoadedScenes

resource tracking loaded scenes for unload support.

### Parent

component that stores the parent entity reference.

an entity can only have one parent. adding a [`Parent`] component
automatically updates the parent's [`Children`] component.

### PostUpdate

built-in stage for transform propagation (runs after Update, before Render).

### SceneData

component storing the raw custom data from the scene definition.

### SceneDefinition

authoring-time scene definition (RON format).

use [`SceneDefinition::from_ron`] to parse from a RON string.

### SceneEntity

marker component for entities spawned from a scene.

### SceneEntry

a scene entry in the world manifest.

### SceneInstance

marker component for entities that instance a sub-scene.
the sub-scene's root entities are spawned as children of this entity.

### SceneLayer

component for scene-defined render layer.

### SceneLoader

scene loader: spawns entities from a scene definition.

use [`SceneLoader::spawn_scene`] to load a scene into the world.

### SceneManager

scene manager resource, manages scene switching and overlays.

switch between scenes with [`SceneManager::switch_to`] or stack
overlay scenes with [`SceneManager::push_overlay`].

### SceneSprite

component for scene-defined sprites.

### SceneTags

component for scene-defined tags.

### SceneText

component for scene-defined text.

### SpriteDef

sprite definition for runtime use.

### StreamingConfig

configuration for the streaming scene loader.

### StreamingState

tracks which chunks are currently loaded for streaming.

### StringInterner

interner that maps strings to u32 identifiers.

at build time, all unique strings from authoring files are collected
and assigned sequential u32 ids. the compiled output references
these ids instead of raw strings.

### TextDef

text definition for runtime use.

### Time

time resource updated each frame

provides delta time for framerate-independent movement and elapsed time.

### TransformDef

transform definition for runtime use.

### WindowSettings

read-only window state resource exposed to game code.

the engine handles all window lifecycle internally (SDL3, wgpu surface).
game code reads this resource to get the current window dimensions and
fullscreen state. to toggle fullscreen, write `is_fullscreen = true`
(or use the default F11/F key binding via ActionMap).

# example

```ignore
fn my_system(settings: Res<WindowSettings>) {
    if settings.is_fullscreen {
        // fullscreen mode
    }
    let aspect = settings.width as f32 / settings.height as f32;
}
```

### WorldManifest

root world manifest parsed from XML.

## Enums

### EngineError

engine error enum covering common failure modes.

### EngineState

engine running state.

this resource is checked each frame to determine if the game loop
should continue running. set to [`Stopping`](EngineState::Stopping)
to trigger a graceful shutdown.

### ErrorSource

source of an error event

### LoadMode

how a scene should be loaded relative to current state.

### StageOrder

relative stage ordering for custom stage placement.

allows inserting custom stages before, after, or between built-in stages.

### TickRate

tick rate buckets based on frame cap.

determines how often the ECS schedule runs, independent of render framerate.

### UpdateStage

built-in update stages for system ordering.

use these to group systems into logical phases of the frame.

## Traits

### Command

a command that can be executed by the engine.

implement this trait to create a custom command.
commands must be [`Send`] and [`Sync`] since they may be
executed from any thread.

### GamePlugin

trait for game plugins

plugins configure the app by adding systems, resources, and other plugins.

### Scene

scene trait — implement to define a game scene.

scenes represent distinct game states like menus, gameplay, or cutscenes.
unlike zones, scenes can be stacked as overlays.

### StageLabelExt

trait for custom stage labels.

implement this to define custom stages that can be ordered
relative to the built-in [`UpdateStage`] variants.

## Functions

### propagate_transforms

exclusive system that propagates transforms from parents to children.

runs as an exclusive world system so `WorldTransform` is written immediately
(no command deferral) — entities have correct world transforms in the same frame
they are spawned.

uses a topological sort (depth-first from roots) so each entity is processed
exactly once, giving O(N) propagation regardless of hierarchy depth.

entities without a parent get their `WorldTransform` directly from `LocalTransform`.

### sync_children

exclusive system that syncs [`Parent`] and [`Children`] components.

runs as an exclusive world system so `Children` is updated immediately
(no command deferral) — children are visible to other systems in the same frame
a `Parent` component is added.

## Type Aliases

### EngineResult

convenience result type for engine operations

## Module engine_core::prelude

full prelude for game development
prelude for lunar-core — re-exports bevy_ecs essentials and lunar-core's
own types (app/plugin, scenes, hierarchy, world manifest, etc.).

domain crates (`lunar-dialogue`, `lunar-localization`, `lunar-zones`)
and subsystem crates (render, input, assets) must be imported separately.

# example

```ignore
use engine_core::prelude::*;

fn setup(mut commands: Commands) {
    commands.spawn((Transform::default(), Player));
}
```

### Re-exports
- Event = bevy_ecs::event::Event
- Event = bevy_ecs::event::Event
- MessageReader = bevy_ecs::message::MessageReader
- MessageWriter = bevy_ecs::message::MessageWriter
- Messages = bevy_ecs::message::Messages
- With = bevy_ecs::query::With
- Without = bevy_ecs::query::Without
- Commands = bevy_ecs::system::Commands
- Color = engine_math::Color
- Mat2 = engine_math::Mat2
- Mat3 = engine_math::Mat3
- Mat4 = engine_math::Mat4
- Rect = engine_math::Rect
- Transform = engine_math::Transform
- Vec2 = engine_math::Vec2
- Vec3 = engine_math::Vec3
- Vec4 = engine_math::Vec4
- App = crate::app::App — app builder for configuring the engine
- GamePlugin = crate::app::GamePlugin — trait for game plugins
- Time = crate::app::Time — time resource updated each frame
- Engine = crate::engine::Engine — the engine owns the ECS world and schedules.
- EngineError = crate::error::EngineError — engine error enum covering common failure modes.
- EngineResult = crate::error::EngineResult — convenience result type for engine operations
- ErrorEvent = crate::error::ErrorEvent — an error event that can be read by game systems.
- ErrorSource = crate::error::ErrorSource — source of an error event
- GameLoop = crate::game_loop::GameLoop — game loop configuration and state.
- TickRate = crate::game_loop::TickRate — tick rate buckets based on frame cap.
- Children = crate::hierarchy::Children — component that stores the list of child entities.
- Parent = crate::hierarchy::Parent — component that stores the parent entity reference.
- Scene = crate::scene::Scene — scene trait — implement to define a game scene.
- SceneManager = crate::scene::SceneManager — scene manager resource, manages scene switching and overlays.
- EntityDefinition = crate::scene_format::EntityDefinition — authoring-time entity definition.
- SceneEntity = crate::scene_format::SceneEntity — marker component for entities spawned from a scene.
- SceneInstance = crate::scene_format::SceneInstance — marker component for entities that instance a sub-scene. the sub-scene's root entities are spawned as children of this entity.
- SceneLayer = crate::scene_format::SceneLayer — component for scene-defined render layer.
- SceneLoader = crate::scene_format::SceneLoader — scene loader: spawns entities from a scene definition.
- SceneSprite = crate::scene_format::SceneSprite — component for scene-defined sprites.
- SceneTags = crate::scene_format::SceneTags — component for scene-defined tags.
- SceneText = crate::scene_format::SceneText — component for scene-defined text.
- SpriteDef = crate::scene_format::SpriteDef — sprite definition for runtime use.
- TextDef = crate::scene_format::TextDef — text definition for runtime use.
- TransformDef = crate::scene_format::TransformDef — transform definition for runtime use.
- StageLabelExt = crate::schedule::StageLabelExt — trait for custom stage labels.
- StageOrder = crate::schedule::StageOrder — relative stage ordering for custom stage placement.
- UpdateStage = crate::schedule::UpdateStage — built-in update stages for system ordering.
- EngineState = crate::state::EngineState — engine running state.
- AdvancedSceneLoader = crate::world_manifest::AdvancedSceneLoader — advanced scene loader supporting multiple load modes.
- LoadMode = crate::world_manifest::LoadMode — how a scene should be loaded relative to current state.
- LoadedScenes = crate::world_manifest::LoadedScenes — resource tracking loaded scenes for unload support.
- WorldManifest = crate::world_manifest::WorldManifest — root world manifest parsed from XML.
- prelude = bevy_ecs::prelude

### Structs

#### AdvancedSceneLoader

advanced scene loader supporting multiple load modes.

#### App

app builder for configuring the engine

use the app to register systems, resources, and plugins before calling `run()`.

#### Children

component that stores the list of child entities.

this is automatically maintained when [`Parent`] components are added/removed.
use the [`Children`] component to iterate over an entity's children.

#### Engine

the engine owns the ECS world and schedules.

this is the low-level wrapper around `bevy_ecs`.
most game code should interact with the engine through [`crate::app::App`] instead.

# example

```ignore
use engine_core::Engine;
use engine_core::app::App;

let mut app = App::new();
app.add_plugin(MyGamePlugin);
// Engine wraps the bevy_ecs World and Schedule
// most code interacts through App instead
```

#### EntityDefinition

authoring-time entity definition.

#### ErrorEvent

an error event that can be read by game systems.

emitted when a recoverable error occurs. game code can
listen for these events and respond accordingly.

#### GameLoop

game loop configuration and state.

manages the fixed timestep accumulator and frame rate limiting.
call [`GameLoop::tick`] each frame to get the number of ECS ticks to run.

#### LoadedScenes

resource tracking loaded scenes for unload support.

#### Parent

component that stores the parent entity reference.

an entity can only have one parent. adding a [`Parent`] component
automatically updates the parent's [`Children`] component.

#### SceneEntity

marker component for entities spawned from a scene.

#### SceneInstance

marker component for entities that instance a sub-scene.
the sub-scene's root entities are spawned as children of this entity.

#### SceneLayer

component for scene-defined render layer.

#### SceneLoader

scene loader: spawns entities from a scene definition.

use [`SceneLoader::spawn_scene`] to load a scene into the world.

#### SceneManager

scene manager resource, manages scene switching and overlays.

switch between scenes with [`SceneManager::switch_to`] or stack
overlay scenes with [`SceneManager::push_overlay`].

#### SceneSprite

component for scene-defined sprites.

#### SceneTags

component for scene-defined tags.

#### SceneText

component for scene-defined text.

#### SpriteDef

sprite definition for runtime use.

#### TextDef

text definition for runtime use.

#### Time

time resource updated each frame

provides delta time for framerate-independent movement and elapsed time.

#### TransformDef

transform definition for runtime use.

#### WorldManifest

root world manifest parsed from XML.

### Enums

#### EngineError

engine error enum covering common failure modes.

#### EngineState

engine running state.

this resource is checked each frame to determine if the game loop
should continue running. set to [`Stopping`](EngineState::Stopping)
to trigger a graceful shutdown.

#### ErrorSource

source of an error event

#### LoadMode

how a scene should be loaded relative to current state.

#### StageOrder

relative stage ordering for custom stage placement.

allows inserting custom stages before, after, or between built-in stages.

#### TickRate

tick rate buckets based on frame cap.

determines how often the ECS schedule runs, independent of render framerate.

#### UpdateStage

built-in update stages for system ordering.

use these to group systems into logical phases of the frame.

### Traits

#### GamePlugin

trait for game plugins

plugins configure the app by adding systems, resources, and other plugins.

#### Scene

scene trait — implement to define a game scene.

scenes represent distinct game states like menus, gameplay, or cutscenes.
unlike zones, scenes can be stacked as overlays.

#### StageLabelExt

trait for custom stage labels.

implement this to define custom stages that can be ordered
relative to the built-in [`UpdateStage`] variants.

### Type Aliases

#### EngineResult

convenience result type for engine operations
