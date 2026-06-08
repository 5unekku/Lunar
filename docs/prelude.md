# prelude reference

`use lunar::prelude::*;` brings everything below into scope. this is the complete list.

## ecs system parameters

| name | type | use |
|------|------|-----|
| `Commands` | system param | deferred spawn/despawn/insert |
| `Res<T>` | system param | read a resource |
| `ResMut<T>` | system param | write a resource |
| `Query<D, F>` | system param | iterate entities matching a component pattern |
| `Single<D, F>` | system param | exactly one entity (panics if 0 or 2+) |
| `Local<T>` | system param | per-system persistent state |
| `In<T>` | system param | piped input from a previous system |

## ecs world types

| name | use |
|------|-----|
| `Entity` | opaque entity id |
| `World` | direct world access (exclusive systems) |
| `EntityRef` | read-only entity view |
| `EntityMut` | mutable entity view |

## ecs derives (use with `#[derive(...)]`)

| derive | what it marks |
|--------|--------------|
| `Component` | component struct/enum |
| `Resource` | singleton resource |
| `Event` | ECS event (use `Message` for buffered streams) |
| `Message` | buffered message stream |

## query filters

| filter | matches |
|--------|---------|
| `With<T>` | entity has T (not accessed) |
| `Without<T>` | entity does not have T |
| `Added<T>` | T was inserted this tick |
| `Changed<T>` | T was mutated this tick |
| `Has<T>` | bool — true if entity has T |
| `Or<(A, B)>` | A or B is true |
| `AnyOf<(A, B)>` | returns any components present |

## change detection wrappers

| type | use |
|------|-----|
| `Mut<T>` | mutable reference with change tracking |
| `Ref<T>` | read reference with change detection |
| `DetectChanges` | trait: `.is_changed()`, `.is_added()` on `Res<T>` |
| `DetectChangesMut` | trait: `.set_if_neq()` on `ResMut<T>` |

## messaging

| type | use |
|------|-----|
| `MessageWriter<T>` | write messages; T must derive `Message` |
| `MessageReader<T>` | read messages with `.read()` iterator |
| `Messages<T>` | direct access to the message buffer |

## math types

| type | description |
|------|-------------|
| `Vec2` | 2d vector — `Vec2::new(x, y)`, `Vec2::ZERO`, `Vec2::ONE` |
| `Vec3` | 3d vector — `Vec3::new(x, y, z)` |
| `Vec4` | 4d vector |
| `Mat2` | 2x2 matrix |
| `Mat3` | 3x3 matrix |
| `Mat4` | 4x4 matrix |
| `Quat` | quaternion (used for 3d rotations) |
| `Transform` | 2d transform — `translation: Vec2`, `rotation: f32`, `scale: Vec2` |
| `Rect` | float rect — `min: Vec2`, `max: Vec2` |
| `ScreenRect` | pixel-space rect — `x1/y1/x2/y2: i16` |
| `Color` | RGBA color — `Color::rgba(r,g,b,a)`, `Color::WHITE`, `Color::BLACK` |

## core types

| type | description |
|------|-------------|
| `App` | builder for registering systems, resources, plugins |
| `GamePlugin` | trait — implement this for your game's root plugin |
| `Time` | resource — delta/elapsed time, time scale |
| `TickRate` | enum — `Hz30` / `Hz60` / `Hz90` / `Hz120` / `Hz144` / `Hz240` |
| `TickRateConfig` | resource — write `.rate` to change tick rate at runtime |
| `LoopConfig` | value passed to `App::run` — `frame_cap`, `tick_rate` |
| `UpdateStage` | enum — `Input`, `Physics`, `Update`, `Render`, `PostUpdate` |
| `WindowSettings` | resource — window size, fullscreen, vsync, cursor lock |
| `Pool<T>` | object pool for reusing allocations |

## rendering (always available)

| type | description |
|------|-------------|
| `Sprite` | 2d sprite component — `Sprite::new(handle)` |
| `Text` | 2d text component — `Text::new("hello", font)` |
| `Camera` | 2d orthographic camera — `Camera::new()`, `Camera::at_position(x, y)` |
| `CameraFollow2d` | component — attach to camera to follow an entity |
| `RenderQueue` | resource — immediate-mode draw calls |
| `RenderEngine` | resource — low-level render access (create render targets, etc.) |
| `RenderConfig` | 2d render configuration |
| `RenderInfo` | resource — frame timing, draw call counts |
| `RenderTargetId` | id for an offscreen render target |
| `RenderTargetStore` | resource — maps `RenderTargetId` → `Handle<Texture>` |
| `ColorTint` | component — override entity tint |
| `YSort` | component — auto-sort entity depth by y position |
| `ScreenShake` | resource — apply screen shake |
| `ScreenFlash` | resource — apply fullscreen flash |
| `PostEffect` | component/resource — post-process effects |
| `PostProcessStack` | resource — ordered list of post effects |
| `layers` | module — `BACKGROUND(0)`, `GAME(100)`, `FOREGROUND(200)`, `UI(300)`, `POST_PROCESS(1000)` |

## 2d (feature `2d`, enabled by default)

| type | description |
|------|-------------|
| `Plugin2d` | plugin — registers transform propagation and 2d collision |
| `Collider` | component — 2d collision shape |
| `ColliderShape` | enum — `Circle(r)`, `Rect(w, h)` |
| `Collider2dBundle` | bundle — `(Collider, Transform)` shorthand |
| `CollisionWorld` | resource — query overlaps and spatial data |
| `SpriteAnimation` | component — frame-based sprite animation |

## assets

| type | description |
|------|-------------|
| `AssetServer` | resource — load textures, fonts, sounds |
| `Handle<T>` | cheap copyable reference to an asset |
| `Texture` | asset type for images |
| `Font` | asset type for truetype fonts |
| `Sound` | asset type for audio clips |
| `AudioFormat` | enum — `OggVorbis`, `OggOpus`, `Wav`, `Flac`, `Unknown` |
| `LoadingState` | resource — current loading progress snapshot |
| `LoadingStats` | snapshot — `total`, `loaded`, `failed`, `.fraction()`, `.is_done()` |
| `TextureSource` | raw pixel data for procedural textures |
| `texture!` | macro — embed and convert image assets at compile time |

## input

| type | description |
|------|-------------|
| `InputState` | resource — keyboard, mouse, gamepad state |
| `ActionMap` | resource — named action → binding map |
| `InputBinding` | enum — `Key(KeyCode)`, `Mouse(MouseButton)`, `GamepadButton(idx, btn)`, `GamepadAxis(idx, axis, threshold)` |
| `KeyCode` | enum — every keyboard key |
| `MouseButton` | enum — `Left`, `Right`, `Middle`, `Extra` |
| `GamepadButton` | enum — `South`, `East`, `West`, `North`, `L1`, `R1`, `L2`, `R2`, `Start`, `Select`, `DPadUp/Down/Left/Right`, ... |
| `GamepadAxis` | enum — `LeftX`, `LeftY`, `RightX`, `RightY`, `LeftTrigger`, `RightTrigger` |

## game data

| type | description |
|------|-------------|
| `GameData` | resource — runtime access to tabular game data |
| `DataTable` | a named table of records |
| `DataRecord` | a row — access fields by name |
| `DataValue` | enum — `Int(i64)`, `Float(f64)`, `Str(Arc<str>)`, `Bool(bool)` |

## marker traits

| trait | use |
|-------|-----|
| `GameComponent` | implement on component types: `Send + Sync + 'static` |
| `GameResource` | implement on resource types: `Send + Sync + 'static` |
