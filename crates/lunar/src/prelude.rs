//! prelude module — re-exports the most common types for game development.
//!
//! Game code should be able to write `use lunar::prelude::*;` and have
//! everything it needs without further imports.
//!
//! The prelude is the **public contract**. The underlying ECS backend
//! (currently bevy_ecs) is an internal implementation detail and may be
//! swapped without breaking game code that sticks to the prelude.
//!
//! # example
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(Transform::default());
//! }
//!
//! fn move_player(time: Res<Time>, mut query: Query<&mut Transform, With<Player>>) {
//!     for mut transform in &mut query {
//!         transform.translation.y += time.delta_seconds();
//!     }
//! }
//! ```

// ECS system parameters
pub use bevy_ecs::system::{
    Commands, In, IntoSystem, Local, NonSend, NonSendMut, Query, Res, ResMut, Single, System,
};

// ECS world / entity types
pub use bevy_ecs::entity::Entity;
pub use bevy_ecs::world::{EntityMut, EntityRef, EntityWorldMut, FromWorld, World};

// ECS markers — these names cover BOTH the trait (used as a bound, e.g. `T: Component`)
// AND the derive macro (used in `#[derive(Component)]`). bevy_ecs re-exports its
// derive macro at `bevy_ecs::component::Component` alongside the trait, which
// would shadow ours and emit `::bevy_ecs::…` paths. So we route deliberately:
// the derive comes from `lunar-macros` (emits `::lunar::__bevy_ecs::…`),
// and the trait stays accessible through the same identifier because Rust
// resolves trait bounds through the type namespace separately from macros.
//
// Net effect: `#[derive(Component)]` uses our wrapper; `T: Component` still
// resolves via the bevy_ecs trait (re-exported from a path that doesn't bring
// the macro into scope — see crate root).
pub use crate::{Component, Event, Message, Resource};

// ECS query filters
pub use bevy_ecs::query::{Added, AnyOf, Changed, Has, Or, With, Without};

// ECS change detection
pub use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut, Mut, Ref};

// ECS messaging (bevy_ecs 0.18 renamed events → messages for buffered streams)
pub use bevy_ecs::message::{MessageReader, MessageWriter, Messages};

// engine-math types
pub use engine_math::{
    Color, Mat2, Mat3, Mat4, Quat, Rect, ScreenRect, Transform, Vec2, Vec3, Vec4,
};

// engine-core types
pub use engine_core::{App, GamePlugin, Time, WindowSettings};

// engine-2d types — only available when the 2d feature is enabled
#[cfg(feature = "2d")]
pub use engine_2d::{Plugin2d, SpriteAnimation, propagate_transforms};

// engine-3d types — only available when the 3d feature is enabled
#[cfg(feature = "3d")]
pub use engine_3d::{
    ActiveCamera3d, AmbientLight, Camera3d, CullMode, DirectionalLight, IndexBuffer,
    LocalTransform3d, Material3d, MaterialData, Mesh3d, MeshData, Plugin3d, PointLight, Projection,
    ShadingModel, SpotLight, Vertex3d, WorldTransform3d, propagate_transforms_3d,
};

// engine-render types
pub use engine_render::{
    Camera, Layer, RenderConfig, RenderEngine, RenderInfo, RenderQueue, Sprite, Text, layers,
};

// engine-input types
pub use engine_input::{ActionMap, InputState, KeyCode, MouseButton};

// engine-assets types — Texture/Font/Sound are needed as type parameters for
// Handle<T> when game code stores asset handles in its own resources.
pub use engine_assets::{AssetServer, AudioFormat, Font, Handle, Sound, Texture};

// lunar marker traits
pub use crate::{GameComponent, GameResource};
