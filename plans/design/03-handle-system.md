# Handle System

## Design Rationale

The engine owns all resources (textures, sounds, fonts, meshes, etc.). Game code references them through typed handles. This ensures:

- No dangling references
- Engine controls lifetime
- Handles are cheap to copy (just an ID)
- Handles can be serialized/deserialized for save states

## Handle Types

```rust
/// Base handle type — a generational index into a resource table
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle<T: Asset> {
    id: u32,
    generation: u16,
    _marker: PhantomData<T>,
}

/// Marker trait for types that can be loaded as assets
pub trait Asset: Send + Sync + 'static {}

/// Concrete handle types
pub type TextureHandle = Handle<Texture>;
pub type SoundHandle = Handle<Sound>;
pub type FontHandle = Handle<Font>;
pub type MeshHandle = Handle<Mesh>;
pub type ShaderHandle = Handle<Shader>;
```

## Handle Operations

```rust
// Loading returns a handle
let tex: TextureHandle = assets.load("textures/player.png");

// Handles can be cloned and compared
let tex2 = tex.clone();
assert_eq!(tex, tex2);

// Check if a handle's data is ready to use
if assets.is_ready(&tex) {
    // safe to use
}

// Get metadata about a loaded resource
if let Some(info) = assets.get_info(&tex) {
    println!("texture: {}x{}", info.width, info.height);
}
```

## Handle Internals

```
AssetStore<T>:
├── entries: Vec<Option<Entry<T>>>
│   └── Entry<T>:
│       ├── data: T                    // the actual resource
│       ├── generation: u16            // incremented on free/reuse
│       ├── ref_count: u32             // handle reference count
│       └── load_state: LoadState      // Loading, Loaded, Failed
└── path_index: HashMap<String, u32>   // path -> entry id
```

When a handle is created:
1. Engine allocates or reuses a slot in the asset store
2. Returns a `Handle<T>` with the slot ID and current generation
3. When the resource is freed, the generation increments
4. Old handles become invalid (generation mismatch)

## Async Loading

Assets load asynchronously. Handles are valid immediately but the data may not be ready:

```rust
let tex = assets.load("textures/player.png");

// Option 1: Check in system
fn render_system(query: Query<&Sprite>, assets: Res<AssetServer>, mut render: ResMut<RenderQueue>) {
    for sprite in query.iter() {
        if assets.is_ready(&sprite.texture) {
            render.draw_sprite(&sprite.texture, ...);
        }
    }
}

// Option 2: Use HandleState component
#[derive(Component)]
struct HandleState<T> {
    handle: Handle<T>,
    state: LoadState,
}

// Option 3: Preload in startup
fn preload_assets(mut commands: Commands, assets: Res<AssetServer>) {
    let paths = [
        "textures/player.png",
        "textures/enemy.png",
        "textures/tileset.png",
    ];
    let handles = assets.load_batch(&paths);
    commands.insert_resource(PreloadedAssets {
        player_texture: handles[0],
        // ...
    });
}
```

---

[← Back to ECS Model](02-ecs-model.md) | [Next: Subsystem APIs →](04-subsystem-apis.md)
