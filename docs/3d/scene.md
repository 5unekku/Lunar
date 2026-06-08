# 3d scene — transforms, meshes, camera

## transforms

3d entities use `LocalTransform3d` and `WorldTransform3d` instead of the 2d `Transform`.
rotation is a quaternion — no gimbal lock.

```rust
use lunar::prelude::*;

// position at (0, 2, -5), looking along +Z
let transform = LocalTransform3d::from_xyz(0.0, 2.0, -5.0)
    .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
    .with_scale(Vec3::splat(2.0));
```

`LocalTransform3d` fields:
- `translation: Vec3` — position in parent space (or world space if no parent)
- `rotation: Quat` — orientation as a quaternion
- `scale: Vec3` — per-axis scale

`WorldTransform3d` is computed by the transform propagation system each tick.
do not write to it directly — write `LocalTransform3d` and the system propagates.

useful `Quat` constructors:
```rust
Quat::IDENTITY                              // no rotation
Quat::from_rotation_y(angle)               // yaw (turn left/right)
Quat::from_rotation_x(angle)               // pitch (look up/down)
Quat::from_rotation_z(angle)               // roll
Quat::from_euler(order, yaw, pitch, roll)  // euler angles → quat
```

## spawning a mesh

meshes are registered in `MeshRegistry` (a resource inserted by `Plugin3d`), not
loaded via `AssetServer`. you get back a `Handle` that goes into a `Mesh3d` component.
same pattern for materials via `MaterialData`.

```rust
use lunar::prelude::*;
use lunar::lunar_3d::primitives::{quad_mesh, sphere_mesh};

fn setup(
    mut commands: Commands,
    mut registry: ResMut<MeshRegistry>,
    mut assets: ResMut<AssetServer>,
) {
    // built-in primitives
    let sphere = registry.add_mesh(sphere_mesh(1.0, 32, 16));
    let quad = registry.add_mesh(quad_mesh(2.0, 2.0));

    // create a material
    let material = registry.add_material(MaterialData {
        base_color: Color::rgba(0.8, 0.3, 0.3, 1.0),
        shading: ShadingModel::Pbr,
        roughness: 0.5,
        metallic: 0.0,
        ..Default::default()
    });

    // spawn the entity
    commands.spawn(Mesh3dBundle {
        local: LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
        mesh: Mesh3d(sphere),
        material: Material3d(material),
        ..Default::default()
    });
}
```

### shadow casting

to make a mesh cast shadows, use `ShadowMesh3dBundle` or add `ShadowCaster` and `Aabb3d`
to a `Mesh3dBundle`:

```rust
// ShadowMesh3dBundle includes Aabb3d + ShadowCaster automatically
commands.spawn(ShadowMesh3dBundle {
    base: Mesh3dBundle {
        local: LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
        mesh: Mesh3d(mesh_handle),
        material: Material3d(mat_handle),
        ..Default::default()
    },
    aabb: Aabb3d { center: Vec3::ZERO, half_extents: Vec3::ONE },
    shadow_caster: ShadowCaster,
});
```

## camera

spawn a `Camera3dBundle` to set the active viewpoint. `Plugin3d` reads the entity
tagged with `ActiveCamera3d` as the main camera.

```rust
fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3dBundle {
            local: LocalTransform3d::from_xyz(0.0, 2.0, 10.0),
            camera: Camera3d {
                projection: Projection::Perspective {
                    fov_y: 1.05,    // ~60 degrees in radians
                    near: 0.1,
                    far: 1000.0,
                },
                ..Default::default()
            },
            ..Default::default()
        },
        ActiveCamera3d,             // marks this as the main camera
    ));
}
```

`Projection` variants:
- `Perspective { fov_y, near, far }` — standard 3d perspective
- `Orthographic { height, near, far }` — isometric/top-down views

moving the camera each tick:

```rust
fn camera_movement(
    input: Res<InputState>,
    time: Res<Time>,
    mut query: Query<&mut LocalTransform3d, With<ActiveCamera3d>>,
) {
    let Ok(mut transform) = query.get_single_mut() else { return };
    let speed = 5.0 * time.delta_seconds();

    if input.is_key_held(KeyCode::W) {
        let forward = transform.rotation * Vec3::NEG_Z;
        transform.translation += forward * speed;
    }
    if input.is_key_held(KeyCode::S) {
        let forward = transform.rotation * Vec3::NEG_Z;
        transform.translation -= forward * speed;
    }

    // mouse look
    let delta = input.mouse_delta();
    let yaw = Quat::from_rotation_y(-delta.x * 0.002);
    let pitch = Quat::from_rotation_x(-delta.y * 0.002);
    transform.rotation = (yaw * transform.rotation * pitch).normalize();
}
```

## visibility and render layers

`Visibility` controls whether an entity is submitted to the renderer:
- `Visibility::Visible` — always render
- `Visibility::Hidden` — never render
- `Visibility::Inherited` — follow parent (default)

`RenderLayers` is a bitmask that pairs entities to cameras. the default layer
(`RenderLayers::DEFAULT`) is layer 0. cameras and meshes on different layers don't see each other.
useful for rendering mirrors, minimap cameras, or UI elements in 3d space.

```rust
// entity only visible on layer 1
commands.spawn((
    Mesh3dBundle { ..Default::default() },
    RenderLayers::from_layers(&[1]),
));

// camera that only sees layer 1
commands.spawn((
    Camera3dBundle { ..Default::default() },
    RenderLayers::from_layers(&[1]),
));
```
