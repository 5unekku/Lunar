# lunar_lib

public API for game logic

this crate re-exports everything a game project needs from the engine.
game code should depend only on `lunar` and use its re-exports.

# quick start

```ignore
use lunar::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_system(my_system);
    app.run(60);
}

fn my_system(time: Res<Time>) {
    // game logic here
}
```

## Re-exports
- Component = lunar_macros::Component
- Event = lunar_macros::Event
- Message = lunar_macros::Message
- Resource = lunar_macros::Resource
- engine_assets = engine_assets
- engine_core = engine_core
- engine_input = engine_input
- engine_math = engine_math
- engine_render = engine_render
- bootstrap = bootstrap::bootstrap — bootstrap a native lunar game.
- AssetServer = engine_assets::AssetServer
- Font = engine_assets::Font
- Handle = engine_assets::Handle
- Sound = engine_assets::Sound
- Texture = engine_assets::Texture
- App = engine_core::App
- GamePlugin = engine_core::GamePlugin
- Time = engine_core::Time
- WindowSettings = engine_core::WindowSettings
- ActionMap = engine_input::ActionMap
- InputBinding = engine_input::InputBinding
- InputState = engine_input::InputState
- KeyCode = engine_input::KeyCode
- MouseButton = engine_input::MouseButton
- Color = engine_math::Color
- Mat2 = engine_math::Mat2
- Mat3 = engine_math::Mat3
- Mat4 = engine_math::Mat4
- Rect = engine_math::Rect
- Transform = engine_math::Transform
- Vec2 = engine_math::Vec2
- Vec3 = engine_math::Vec3
- Vec4 = engine_math::Vec4
- Camera = engine_render::Camera
- Layer = engine_render::Layer
- RenderConfig = engine_render::RenderConfig
- RenderEngine = engine_render::RenderEngine
- RenderInfo = engine_render::RenderInfo
- RenderQueue = engine_render::RenderQueue
- Sprite = engine_render::Sprite
- Text = engine_render::Text
- layers = engine_render::layers
- prelude = prelude — prelude module — re-exports the most common types for game development.

## Modules

### prelude

prelude module — re-exports the most common types for game development.

Game code should be able to write `use lunar::prelude::*;` and have
everything it needs without further imports.

The prelude is the **public contract**. The underlying ECS backend
(currently bevy_ecs) is an internal implementation detail and may be
swapped without breaking game code that sticks to the prelude.

# example

```ignore
use lunar::prelude::*;

fn setup(mut commands: Commands) {
    commands.spawn(Transform::default());
}

fn move_player(time: Res<Time>, mut query: Query<&mut Transform, With<Player>>) {
    for mut transform in &mut query {
        transform.translation.y += time.delta_seconds();
    }
}
```

### prelude

prelude module — re-exports the most common types for game development.

Game code should be able to write `use lunar::prelude::*;` and have
everything it needs without further imports.

The prelude is the **public contract**. The underlying ECS backend
(currently bevy_ecs) is an internal implementation detail and may be
swapped without breaking game code that sticks to the prelude.

# example

```ignore
use lunar::prelude::*;

fn setup(mut commands: Commands) {
    commands.spawn(Transform::default());
}

fn move_player(time: Res<Time>, mut query: Query<&mut Transform, With<Player>>) {
    for mut transform in &mut query {
        transform.translation.y += time.delta_seconds();
    }
}
```

## Traits

### GameComponent

marker trait for components that can be used in game logic.

any type implementing this trait is guaranteed to be [`Send`], [`Sync`],
and have a `'static` lifetime, making it safe to use across threads
and store in the ECS world indefinitely.

# example

```ignore
use lunar::GameComponent;

struct Player {
    health: u32,
}

impl GameComponent for Player {}
```

### GameResource

marker trait for resources that can be used in game logic.

resources are global state accessible from any system.
like [`GameComponent`], they must be [`Send`], [`Sync`], and `'static`.

# example

```ignore
use lunar::GameResource;

struct ScoreTracker {
    current_score: u32,
}

impl GameResource for ScoreTracker {}
```

## Functions

### bootstrap

bootstrap a native lunar game.

initializes SDL3, creates a window, sets up the wgpu render surface,
adds all built-in plugins including default fullscreen bindings (F11/F),
and runs the game loop. the window title defaults to `"Lunar"`.

game code never touches SDL3, wgpu, or unsafe — read window state
through [`crate::WindowSettings`].

# example

```ignore
use lunar::prelude::*;

struct MyGame;
impl GamePlugin for MyGame {
    fn name(&self) -> &str { "MyGame" }
}

fn main() {
    lunar::bootstrap::<MyGame>(Default::default());
}
```

## Module lunar_lib::prelude

prelude module — re-exports the most common types for game development.

Game code should be able to write `use lunar::prelude::*;` and have
everything it needs without further imports.

The prelude is the **public contract**. The underlying ECS backend
(currently bevy_ecs) is an internal implementation detail and may be
swapped without breaking game code that sticks to the prelude.

# example

```ignore
use lunar::prelude::*;

fn setup(mut commands: Commands) {
    commands.spawn(Transform::default());
}

fn move_player(time: Res<Time>, mut query: Query<&mut Transform, With<Player>>) {
    for mut transform in &mut query {
        transform.translation.y += time.delta_seconds();
    }
}
```

### Re-exports
- Commands = bevy_ecs::system::Commands
- In = bevy_ecs::system::In
- IntoSystem = bevy_ecs::system::IntoSystem
- Local = bevy_ecs::system::Local
- NonSend = bevy_ecs::system::NonSend
- NonSendMut = bevy_ecs::system::NonSendMut
- Query = bevy_ecs::system::Query
- Res = bevy_ecs::system::Res
- ResMut = bevy_ecs::system::ResMut
- Single = bevy_ecs::system::Single
- System = bevy_ecs::system::System
- Entity = bevy_ecs::entity::Entity
- EntityMut = bevy_ecs::world::EntityMut
- EntityRef = bevy_ecs::world::EntityRef
- EntityWorldMut = bevy_ecs::world::EntityWorldMut
- FromWorld = bevy_ecs::world::FromWorld
- FromWorld = bevy_ecs::world::FromWorld
- World = bevy_ecs::world::World
- Component = crate::Component
- Event = crate::Event
- Message = crate::Message
- Resource = crate::Resource
- Added = bevy_ecs::query::Added
- AnyOf = bevy_ecs::query::AnyOf
- Changed = bevy_ecs::query::Changed
- Has = bevy_ecs::query::Has
- Or = bevy_ecs::query::Or
- With = bevy_ecs::query::With
- Without = bevy_ecs::query::Without
- DetectChanges = bevy_ecs::change_detection::DetectChanges
- DetectChangesMut = bevy_ecs::change_detection::DetectChangesMut
- Mut = bevy_ecs::change_detection::Mut
- Ref = bevy_ecs::change_detection::Ref
- MessageReader = bevy_ecs::message::MessageReader
- MessageWriter = bevy_ecs::message::MessageWriter
- Messages = bevy_ecs::message::Messages
- Color = engine_math::Color
- Mat2 = engine_math::Mat2
- Mat3 = engine_math::Mat3
- Mat4 = engine_math::Mat4
- Rect = engine_math::Rect
- Transform = engine_math::Transform
- Vec2 = engine_math::Vec2
- Vec3 = engine_math::Vec3
- Vec4 = engine_math::Vec4
- App = engine_core::App
- GamePlugin = engine_core::GamePlugin
- Time = engine_core::Time
- WindowSettings = engine_core::WindowSettings
- Camera = engine_render::Camera
- Layer = engine_render::Layer
- RenderConfig = engine_render::RenderConfig
- RenderEngine = engine_render::RenderEngine
- RenderInfo = engine_render::RenderInfo
- RenderQueue = engine_render::RenderQueue
- Sprite = engine_render::Sprite
- Text = engine_render::Text
- layers = engine_render::layers
- ActionMap = engine_input::ActionMap
- InputState = engine_input::InputState
- KeyCode = engine_input::KeyCode
- MouseButton = engine_input::MouseButton
- AssetServer = engine_assets::AssetServer
- Font = engine_assets::Font
- Handle = engine_assets::Handle
- Sound = engine_assets::Sound
- Texture = engine_assets::Texture
- GameComponent = crate::GameComponent — marker trait for components that can be used in game logic.
- GameResource = crate::GameResource — marker trait for resources that can be used in game logic.

### Traits

#### GameComponent

marker trait for components that can be used in game logic.

any type implementing this trait is guaranteed to be [`Send`], [`Sync`],
and have a `'static` lifetime, making it safe to use across threads
and store in the ECS world indefinitely.

# example

```ignore
use lunar::GameComponent;

struct Player {
    health: u32,
}

impl GameComponent for Player {}
```

#### GameResource

marker trait for resources that can be used in game logic.

resources are global state accessible from any system.
like [`GameComponent`], they must be [`Send`], [`Sync`], and `'static`.

# example

```ignore
use lunar::GameResource;

struct ScoreTracker {
    current_score: u32,
}

impl GameResource for ScoreTracker {}
```
