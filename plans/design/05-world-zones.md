# World and Zone Management

> **Crate:** zones live in `engine-zones`, an opt-in domain crate. Add it to
> your `Cargo.toml` only if your game uses zoned area loading. The Scene
> system below stays in `engine-core` because it's a generic game-state
> pattern useful to all games.

## Design Philosophy

Lunar supports two models of level organization, both optional:

**Zone model (default for RPGs):** A persistent world where zones/areas load and unload as the player moves between them. Think Earthbound, Final Fantasy, Deltarune — you walk from the overworld into a building, into a room, all seamless. The world state (player inventory, flags, NPCs) persists across zones.

**Scene model (for menu-driven games):** Hard switches between distinct screens — main menu, game over, levels. Think Hades runs, Hollow Knight bench saves.

Games can use either or both. The engine doesn't force either model.

## Zone Model

A **Zone** is a collection of entities and systems that can be loaded/unloaded independently. The world persists across zone transitions.

```rust
/// Zone trait — implement to define a world zone
pub trait Zone: Send + Sync + 'static {
    /// Called when the zone is being loaded (async asset loading)
    fn on_load(&mut self, app: &mut App);

    /// Called when the zone becomes active
    fn on_enter(&mut self, app: &mut App);

    /// Called when the zone is being unloaded
    fn on_exit(&mut self, app: &mut App);

    /// Optional: define transition points
    fn transitions(&self) -> Vec<ZoneTransition>;
}

/// A transition point — triggers when an entity enters the area
pub struct ZoneTransition {
    pub trigger_area: Rect,
    pub target_zone: String,
    pub spawn_position: Vec2,
    pub fade: Option<FadeConfig>,
}
```

## World Manager

```rust
#[derive(Resource)]
pub struct WorldManager {
    // internal state
}

impl WorldManager {
    /// Register a zone by name
    pub fn register_zone<Z: Zone>(&mut self, name: &str, zone: Z);

    /// Transition to a zone (keeps world state)
    pub fn enter_zone(&mut self, name: &str);

    /// Get the current zone name
    pub fn current_zone(&self) -> Option<&str>;

    /// Get persistent world data (survives zone transitions)
    pub fn world_data(&self) -> &WorldData;
}
```

## Zone Definition Example (RPG-style)

```rust
struct TownZone;

impl Zone for TownZone {
    fn on_load(&mut self, app: &mut App) {
        // load zone-specific assets
        app.insert_resource(TownConfig {
            music: assets.load("audio/town.ogg"),
            encounter_rate: 0.0, // no random encounters in town
        });
    }

    fn on_enter(&mut self, app: &mut App) {
        // spawn zone entities (NPCs, shops, etc.)
        // player position is preserved from the transition point
    }

    fn on_exit(&mut self, app: &mut App) {
        // despawn zone entities
        // player and world data persist
    }

    fn transitions(&self) -> Vec<ZoneTransition> {
        vec![
            ZoneTransition {
                trigger_area: Rect::new(600.0, 0.0, 40.0, 40.0), // doorway
                target_zone: "shop_interior".to_string(),
                spawn_position: Vec2::new(100.0, 200.0),
                fade: Some(FadeConfig {
                    duration: 0.5,
                    color: Color::BLACK,
                }),
            },
        ]
    }
}
```

## Scene Model (Menu-driven)

For games that need hard scene switches:

```rust
/// Scene trait — for distinct game states
pub trait Scene: Send + Sync + 'static {
    fn on_enter(&mut self, app: &mut App);
    fn on_update(&mut self, app: &mut App);
    fn on_exit(&mut self, app: &mut App);
}

#[derive(Resource)]
pub struct SceneManager {
    // internal state
}

impl SceneManager {
    pub fn switch_to(&mut self, name: &str);
    pub fn current_scene(&self) -> Option<&str>;
}
```

## Scene Transitions

Both zones and scenes support optional fade transitions:

```rust
pub struct FadeConfig {
    pub duration: f32,
    pub color: Color,
}
```

```rust
// Main game scene
scene_manager.switch_to("level1");

// Push a HUD scene on top
scene_manager.push_overlay("hud");

// Pop it when done
scene_manager.pop_overlay();
```

---

[← Back to Subsystem APIs](04-subsystem-apis.md) | [Next: Dialogue and Text System →](06-dialogue-system.md)
