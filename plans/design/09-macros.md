# Macros

## Entry Point Macro

```rust
/// Bootstrap the engine with a game plugin
///
/// Expands to:
/// - async main function
/// - window creation
/// - subsystem initialization
/// - plugin registration
/// - app.run()
#[macro_export]
macro_rules! lunar_app {
    ($plugin:ty) => {
        #[tokio::main]
        async fn main() {
            $crate::prelude::App::new()
                .add_plugin($crate::engine_plugins::LogPlugin)
                .add_plugin($crate::engine_plugins::TimePlugin)
                .add_plugin($crate::engine_plugins::InputPlugin)
                .add_plugin($crate::engine_plugins::RenderPlugin)
                .add_plugin($crate::engine_plugins::AudioPlugin)
                .add_plugin(<$plugin>::default())
                .run();
        }
    };

    ($plugin:ty, config: $config:expr) => {
        #[tokio::main]
        async fn main() {
            let config = $config;
            $crate::prelude::App::new()
                .add_plugin($crate::engine_plugins::LogPlugin)
                .add_plugin($crate::engine_plugins::TimePlugin)
                .add_plugin($crate::engine_plugins::InputPlugin::with_config(&config.input))
                .add_plugin($crate::engine_plugins::RenderPlugin::with_config(&config.render))
                .add_plugin($crate::engine_plugins::AudioPlugin::with_config(&config.audio))
                .add_plugin(<$plugin>::default())
                .run();
        }
    };
}
```

## Component/Resource Derive

These re-export bevy_ecs derives:

```rust
/// Derive macro for ECS components
pub use bevy_ecs::component::Component;

/// Derive macro for ECS resources
pub use bevy_ecs::system::Resource;
```

## Asset Handle Derive

```rust
/// Derive macro to mark a type as an asset
#[macro_export]
macro_rules! impl_asset {
    ($ty:ty) => {
        impl $crate::prelude::Asset for $ty {}
    };
}

// Usage in engine:
impl_asset!(Texture);
impl_asset!(Sound);
impl_asset!(Font);
```

## Stage Label Derive

```rust
/// Derive macro for custom stage labels
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StageLabel(&'static str);

// Usage:
const MY_STAGE: StageLabel = StageLabel("my_stage");
```

## Convenience Macros

```rust
/// Create a Transform from position
#[macro_export]
macro_rules! transform {
    ($x:expr, $y:expr) => {
        $crate::prelude::Transform::from_translation(
            $crate::prelude::Vec3::new($x, $y, 0.0)
        )
    };
    ($x:expr, $y:expr, $z:expr) => {
        $crate::prelude::Transform::from_translation(
            $crate::prelude::Vec3::new($x, $y, $z)
        )
    };
}

/// Create a Color from RGB
#[macro_export]
macro_rules! color {
    ($r:expr, $g:expr, $b:expr) => {
        $crate::prelude::Color::rgb($r, $g, $b)
    };
    ($r:expr, $g:expr, $b:expr, $a:expr) => {
        $crate::prelude::Color::rgba($r, $g, $b, $a)
    };
}

/// Create a Rect
#[macro_export]
macro_rules! rect {
    ($x:expr, $y:expr, $w:expr, $h:expr) => {
        $crate::prelude::Rect::new($x, $y, $w, $h)
    };
}
```

## Query Macros (Optional Convenience)

```rust
/// Shorthand for common query patterns
#[macro_export]
macro_rules! query {
    // Single component
    ($comp:ty) => {
        $crate::prelude::Query<&$comp>
    };
    // Multiple components
    ($($comp:ty),+) => {
        $crate::prelude::Query<($(& $comp),+)>
    };
    // With filter
    ($($comp:ty),+ ; without $($without:ty),+) => {
        $crate::prelude::Query<($(& $comp),+), $crate::prelude::Without<($($without),+)>>
    };
}
```

---

[← Back to Plugin System](08-plugin-system.md) | [Next: Error Handling →](10-error-handling.md)
