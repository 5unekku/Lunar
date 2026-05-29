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

// lunar-math types
pub use lunar_math::{
    Color, Mat2, Mat3, Mat4, Quat, Rect, ScreenRect, Transform, Vec2, Vec3, Vec4,
};

// lunar-core types
pub use lunar_core::{App, GamePlugin, Time, UpdateStage, WindowSettings};
pub use lunar_core::pool::Pool;

// lunar-2d types — only available when the 2d feature is enabled
#[cfg(feature = "2d")]
pub use lunar_2d::{
    Collider, Collider2dBundle, ColliderShape, CollisionWorld, Plugin2d, SpriteAnimation,
    propagate_transforms,
};

// lunar-3d types — only available when the 3d feature is enabled
#[cfg(feature = "3d")]
pub use lunar_3d::{
    // core transforms / camera / mesh / material / lights
    ActiveCamera3d, AmbientLight, Camera3d, CullMode, DirectionalLight, IndexBuffer, IrradianceSH,
    LocalTransform3d, Material3d, MaterialData, Mesh3d, MeshData, MeshUsage, Plugin3d, StaticMesh,
    PointLight, Projection, ShadingModel, SkinWeights, SpotLight, Vertex3d, WorldTransform3d,
    propagate_transforms_3d,
    // bundles
    Camera3dBundle, DirectionalLightBundle, Mesh3dBundle, PointLightBundle, ShadowMesh3dBundle,
    SpotLightBundle,
    // animation
    AnimationClip, AnimationPlayer, AnimationTarget, JointTrack, Keyframe, advance_animations,
    // collision + raycasting
    Collider3d, ColliderShape3d, CollisionWorld3d, Ray3d, RayHit3d,
    build_collision_world_3d, raycast_3d,
    // fog
    Fog, FogFalloff,
    // visibility
    Aabb3d, ComputedVisibility, Frustum, RenderLayers, ShadowCaster, ShadowReceiver,
    Visibility, ViewportAspect, propagate_visibility, update_frustum,
    // primitives
    primitives,
    // mesh registry
    MeshRegistry,
};
// lunar-render-3d types — only available when the 3d feature is enabled
#[cfg(feature = "3d")]
pub use lunar_render_3d::{DevRenderProfile, QualityPreset, QualitySettings, RenderConfig3d, RenderEngine3d, RenderInfo3d, RenderPlugin3d, Sky};

// lunar-bsp types — BVH spatial acceleration, portal culling, and BSP runtime (3d feature)
#[cfg(feature = "3d")]
pub use lunar_bsp::{
    Area, BspPlugin, Bvh, BvhPlugin, BvhVisible, BvhNode,
    BspLevel,
    Portal, PortalCulling, PortalPlugin, VisibleAreas,
    portal::{CameraArea, PortalOpen},
};

// lunar-lightmap types — CPU lightmap baker and Lightmap component (3d feature)
#[cfg(feature = "3d")]
pub use lunar_lightmap::{
    Lightmap,
    baker::{BakeDirectional, BakeResult, LightmapBaker},
};

// multiview/split-screen types
#[cfg(feature = "3d")]
pub use lunar_3d::{ActiveViewports, ViewportRect};

// mip streaming types
pub use lunar_assets::{MipStreamingConfig, TextureVramUsage};

// Bundle derive — needed for game code that defines its own bundles
#[cfg(feature = "3d")]
pub use bevy_ecs::bundle::Bundle;

// lunar-render types
pub use lunar_gamedata::{DataRecord, DataTable, DataValue, GameData};

pub use lunar_render::{
    Camera, CameraFollow2d, ColorTint, Layer, PostEffect, PostProcessStack, RenderConfig,
    RenderEngine, RenderInfo, RenderQueue, RenderTargetId, RenderTargetStore, ScreenFlash,
    ScreenShake, Sprite, Text, YSort, layers,
};

// lunar-input types
pub use lunar_input::{ActionMap, GamepadAxis, GamepadButton, InputBinding, InputState, KeyCode, MouseButton};

// lunar-assets types — Texture/Font/Sound are needed as type parameters for
// Handle<T> when game code stores asset handles in its own resources.
pub use lunar_assets::{AssetServer, AudioFormat, Font, Handle, LoadingState, LoadingStats, Sound, Texture, TextureSource};

// texture! macro — embeds and converts image assets at compile time
pub use crate::texture;

// optional engine modules — included in prelude when the feature is enabled
#[cfg(feature = "dialogue")]
pub use lunar_dialogue::{
    Block, Character, Choice, DialogueManager, DialoguePlugin, Next, Script, ScriptBuilder,
};

// lunar marker traits
pub use crate::{GameComponent, GameResource};
