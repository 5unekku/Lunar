# engine_render

rendering subsystem via wgpu

decoupled from game logic. handles 2D rendering with wgpu.
2D-only by design; 3D, if it ever exists, will be a sister engine.


## Modules

### atlas

texture atlas integration for the render system.

provides [`TextureAtlas`] resource that wraps a packed atlas texture
with named region lookup. sprites can reference atlas regions via
`DrawKind::Sprite` with an optional `atlas_region` field.

# example

```ignore
use engine_render::atlas::TextureAtlas;
use engine_atlas::{AtlasManifest, AtlasRegion};

// load atlas texture via asset server
let atlas_texture = asset_server.load_texture("sprites_atlas.mi");
let manifest = AtlasManifest::from_bytes(&manifest_bytes)?;

let texture_atlas = TextureAtlas::new(atlas_texture, manifest);
let region = texture_atlas.region("player_idle");
```

### layers

built-in layer constants for common rendering needs.
lower values are drawn first (behind), higher values are drawn last (in front).

### textbox

textbox component for rendering text with typewriter animation

provides a [`Textbox`] component that can be attached to entities
for displaying text with optional typewriter-style animation.

# example

```ignore
use engine_render::textbox::{Textbox, TypewriterState};
use engine_math::Vec2;

let mut textbox = Textbox::new("hello, world!", Vec2::new(100.0, 100.0), Vec2::new(400.0, 100.0));
textbox.set_font(0, 24.0);
textbox.start_typewriter(0.05); // 50ms per character
```

## Structs

### Camera

camera resource, affects how the render queue is projected.

when no camera resource exists, rendering uses world-space anchored at origin.
when present, the orthographic projection is offset and scaled accordingly.

# example

```ignore
use engine_render::Camera;
use engine_math::Vec2;

// camera centered at (400, 300), letterboxed to an 800x600 viewport
let cam = Camera {
    position: Vec2::new(400.0, 300.0),
    zoom: 1.0,
    rotation: 0.0,
    viewport: Some((800, 600)),
    layer_parallax: Default::default(),
};

// use cam.projection_matrix(window_w, window_h) for the render projection
```

### DebugOverlay

debug overlay for displaying runtime stats.

when enabled, draws FPS, frame time, sprite count, and entity count
in the top-left corner using immediate mode rendering.

### DrawContext

drawing context for immediate mode rendering.

provides convenience methods for debug drawing without managing
draw commands manually. obtained via [`RenderQueue::draw_immediate`].

### Layer

ECS component that assigns an entity to a render layer.

entities with a higher layer value are drawn on top of lower layers.
use the [`layers`] constants for common layer assignments.

### RenderConfig

rendering configuration.

controls window size, vsync, and frame rate limiting.
used when initializing the [`RenderEngine`] and [`engine_core::WindowSettings`].

### RenderEngine

render engine resource, owns all wgpu rendering state.

manages the GPU device, queue, surface, and render pipelines.
the [`Resource`] derive is only applied on native targets — on WASM,
WebGPU types are `!Send`, so the engine is stored in a static instead.

### RenderInfo

render info resource, tracks rendering statistics.

updated each frame by the render system. game code can read
this to display debug info or make performance decisions.

### RenderPlugin

render plugin, registers render systems and resources.

add this plugin to your [`App`] to enable rendering.
it registers the [`RenderQueue`] and [`RenderInfo`] as ECS resources.

### RenderQueue

render queue resource, collects draw commands each frame.

game logic pushes draw commands into the queue during the update phase.
the render engine consumes the queue during the render phase.


### Sprite

renderable 2D sprite component.

any entity carrying a [`Transform`] and a `Sprite`
is drawn automatically each frame. game code spawns the entity and the
engine's render system enqueues the draw — no manual `RenderQueue` calls.

# example

```ignore
use lunar::prelude::*;

fn spawn_player(mut commands: Commands, assets: Res<AssetServer>) {
    let texture = assets.get_texture_handle("player.png");
    commands.spawn((
        Transform::from_xy(100.0, 100.0),
        Sprite::new(texture).with_size(Vec2::new(32.0, 32.0)),
    ));
}
```

fields can be set directly or via the builder methods. when `size` is
`None`, the sprite renders at the texture's native pixel size if the
texture is loaded; otherwise a 32×32 placeholder is used.

### SpriteParams

parameters for drawing a transformed sprite.
used with [`RenderQueue::draw_sprite_transformed_on_layer`] to avoid
too many function arguments.

### Text

renderable text component.

any entity carrying a [`Transform`] and a `Text`
is drawn automatically each frame. position comes from `Transform.translation`.

# example

```ignore
use lunar::prelude::*;

fn spawn_label(mut commands: Commands, assets: Res<AssetServer>) {
    let font = assets.get_font_handle("ui.ttf");
    commands.spawn((
        Transform::from_xy(10.0, 10.0),
        Text::new("Score: 0", font).with_size(20.0),
    ));
}
```

## Traits

### RenderPass

trait for custom render passes that can be executed by the render engine.

implement this trait to add custom rendering (e.g. post-processing, 3D passes).
passes are executed in registration order after the default 2D pass.

## Module engine_render::atlas

texture atlas integration for the render system.

provides [`TextureAtlas`] resource that wraps a packed atlas texture
with named region lookup. sprites can reference atlas regions via
`DrawKind::Sprite` with an optional `atlas_region` field.

# example

```ignore
use engine_render::atlas::TextureAtlas;
use engine_atlas::{AtlasManifest, AtlasRegion};

// load atlas texture via asset server
let atlas_texture = asset_server.load_texture("sprites_atlas.mi");
let manifest = AtlasManifest::from_bytes(&manifest_bytes)?;

let texture_atlas = TextureAtlas::new(atlas_texture, manifest);
let region = texture_atlas.region("player_idle");
```

### Structs

#### TextureAtlas

a loaded texture atlas with GPU texture handle and region lookup.

## Module engine_render::textbox

textbox component for rendering text with typewriter animation

provides a [`Textbox`] component that can be attached to entities
for displaying text with optional typewriter-style animation.

# example

```ignore
use engine_render::textbox::{Textbox, TypewriterState};
use engine_math::Vec2;

let mut textbox = Textbox::new("hello, world!", Vec2::new(100.0, 100.0), Vec2::new(400.0, 100.0));
textbox.set_font(0, 24.0);
textbox.start_typewriter(0.05); // 50ms per character
```

### Structs

#### Textbox

a textbox component for rendering text on screen.

contains the text content, position, size, font settings,
and optional typewriter animation state.

#### TypewriterState

state for typewriter animation.

tracks how many characters are currently visible
and the timing for revealing the next one.

## Module engine_render::layers

built-in layer constants for common rendering needs.
lower values are drawn first (behind), higher values are drawn last (in front).

### Constants

#### BACKGROUND

background layer — static backgrounds, parallax layers

#### FOREGROUND

foreground layer — effects, overlays, weather

#### GAME

game layer — game objects, characters, projectiles

#### UI

UI layer — HUD, menus, dialogue boxes
