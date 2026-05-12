# engine_math

math re-exports and custom utilities

this crate wraps [`glam`] for vector/matrix math and provides engine-specific types
like [`Transform`], [`Color`], and [`Rect`].


## Re-exports
- glam = glam
- Color = types::Color — RGBA color type.
- LocalTransform = types::LocalTransform — local transform: position, rotation, and scale relative to the parent entity.
- Rect = types::Rect — 2D rectangle: position + size.
- Transform = types::Transform — 2D transform component: position, rotation, scale.
- WorldTransform = types::WorldTransform — world transform: absolute position, rotation, and scale in world space.

## Structs

### Color

RGBA color type.

all channels are normalized to the range 0.0 - 1.0.
common colors are provided as associated constants.

# example

```ignore
let red = Color::rgb(1.0, 0.0, 0.0);
let semi_transparent = Color::rgba(1.0, 1.0, 1.0, 0.5);
```

### LocalTransform

local transform: position, rotation, and scale relative to the parent entity.

when an entity has no parent, this is equivalent to world space.
used in entity hierarchies for parent-child transform propagation.

### Rect

2D rectangle: position + size.

represents a bounding box with top-left corner at (x, y)
and dimensions (w, h). useful for collision detection and UI layout.

# example

```ignore
let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
if rect.contains(mouse_pos) {
    // clicked!
}
```

### Transform

2D transform component: position, rotation, scale.

this is the primary way to represent an entity's placement in the world.
it supports translation (x, y), rotation (radians), and scale (x, y).
for depth sorting, use the `Layer` component from `engine_render`.


### WorldTransform

world transform: absolute position, rotation, and scale in world space.

this component is computed automatically from [`LocalTransform`] and
parent hierarchy. do not modify directly — use [`LocalTransform`] instead.

## Type Aliases

### Mat2

2x2 matrix type alias.

backed by [`glam::Mat2`], used for 2D rotations.

### Mat3

3x3 matrix type alias.

backed by [`glam::Mat3`]. re-exported from glam at zero cost; not used by
the engine API.

### Mat4

4x4 matrix type alias.

backed by [`glam::Mat4`]. used internally for shader projection uniforms;
game code rarely needs it directly.

### Vec2

2D vector type alias.

backed by [`glam::Vec2`], provides x, y components with SIMD support.

### Vec3

3D vector type alias.

backed by [`glam::Vec3`]. the engine surface is 2D-only — `Vec3` is
re-exported for game code that needs it (colors, custom math) at zero cost
from glam, but no engine API consumes or returns it.

### Vec4

4D vector type alias.

backed by [`glam::Vec4`], useful for packed colors and shader uniforms.

## Macros

### color

create a `Color` from components.

# example

```ignore
use engine_math::color;

let c = color!(r: 1.0, g: 0.0, b: 0.0);
let d = color!(r: 1.0, g: 0.0, b: 0.0, a: 0.5);
```

### query

convenience wrapper for creating ecs query types.

this macro simplifies common query patterns by wrapping `bevy_ecs` query filters
into a single expression. it is designed to be used in system function signatures.

# example

```ignore
use engine_math::query;
use bevy_ecs::prelude::Query;

// query for entities with Position and Velocity
fn my_system(query: query!(Position, Velocity)) {
    for (pos, vel) in query.iter() {
        // ...
    }
}

// query with filters
fn filtered(query: query!(Position, with: Player, without: Dead, changed: Velocity)) {
    // ...
}
```

### rect

create a `Rect` from components.

# example

```ignore
use engine_math::rect;

let r = rect!(x: 0, y: 0, w: 100, h: 50);
```

### transform

create a `Transform` from components.

# example

```ignore
use engine_math::{transform, Vec2};

let t = transform!(pos: Vec2::new(10.0, 20.0), rot: 0.5, scale: Vec2::ONE);
```
