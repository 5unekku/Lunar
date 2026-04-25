# Plugin System

## Plugin Trait

```rust
/// Trait for game plugins
pub trait GamePlugin: Send + Sync + 'static {
    /// Build the app — register systems, resources, etc.
    fn build(&self, app: &mut App);

    /// Called after all plugins are built, before the game loop
    fn finish(&self, _app: &mut App) {}
}
```

## App Builder

```rust
/// Application builder — chains plugin registration
pub struct App {
    world: World,
    schedule: Schedule,
    plugins: Vec<Box<dyn GamePlugin>>,
    // ...
}

impl App {
    /// Create a new app
    pub fn new() -> Self;

    /// Add a game plugin
    pub fn add_plugin<P: GamePlugin>(&mut self, plugin: P) -> &mut Self;

    /// Add a system to the default update stage
    pub fn add_system<S: IntoSystem>(&mut self, system: S) -> &mut Self;

    /// Add a system to a specific stage
    pub fn add_system_to_stage<S: IntoSystem>(
        &mut self,
        stage: impl StageLabel,
        system: S,
    ) -> &mut Self;

    /// Add a startup system
    pub fn add_startup_system<S: IntoSystem>(&mut self, system: S) -> &mut Self;

    /// Insert a resource
    pub fn insert_resource<R: Resource>(&mut self, resource: R) -> &mut Self;

    /// Register a scene
    pub fn add_scene<S: Scene>(&mut self, name: &str, scene: S) -> &mut Self;

    /// Register a command
    pub fn register_command(&mut self, name: &str, command: Box<dyn Command>) -> &mut Self;

    /// Get mutable access to the ECS world
    pub fn world_mut(&mut self) -> &mut World;

    /// Run the app (blocks until quit)
    pub fn run(self);
}
```

## Engine Plugins

The engine itself is composed of plugins:

```rust
/// Built-in engine plugins
pub mod engine_plugins {
    /// Input plugin — sets up input subsystem
    pub struct InputPlugin;

    /// Render plugin — sets up rendering
    pub struct RenderPlugin;

    /// Audio plugin — sets up audio
    pub struct AudioPlugin;

    /// Time plugin — sets up time/delta time
    pub struct TimePlugin;

    /// Log plugin — sets up logging
    pub struct LogPlugin;
}
```

## Plugin Execution Order

```
App::run()
├── LogPlugin.build()
├── TimePlugin.build()
├── InputPlugin.build()
├── RenderPlugin.build()
├── AudioPlugin.build()
├── [Game plugins .build() in registration order]
├── [All plugins .finish() in registration order]
├── Startup systems (in order)
└── Game loop:
    ├── Input stage
    ├── Physics stage
    ├── Update stage
    ├── Render stage
    └── Frame cap sleep
```

## Plugin Dependencies

Plugins can declare dependencies:

```rust
impl GamePlugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        // ...
    }
}

impl PluginDependencies for PhysicsPlugin {
    fn depends_on(&self) -> Vec<&str> {
        vec!["TimePlugin", "InputPlugin"]
    }
}
```

The app builder topologically sorts plugins by dependencies before calling `build()`.

---

[← Back to Asset Pipeline](07-asset-pipeline.md) | [Next: Macros →](09-macros.md)
