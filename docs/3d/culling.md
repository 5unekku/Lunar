# 3d culling — BSP, BVH, portal

the engine has three complementary culling systems. use them together for
best performance — they're additive, not mutually exclusive.

## frustum culling (always on)

`Plugin3d` runs a CPU frustum cull against entity AABBs every frame. add `Aabb3d`
to any mesh entity to participate:

```rust
commands.spawn((
    Mesh3dBundle { .. },
    Aabb3d {
        center: Vec3::ZERO,
        half_extents: Vec3::new(1.0, 2.0, 1.0),
    },
));
```

entities without `Aabb3d` are never culled and always submitted to the renderer.

## BVH culling (`BvhPlugin`)

for open worlds with many entities, `BvhPlugin` replaces the linear frustum scan
with a dynamic AABB tree. complexity drops from O(n) to O(log n) per frame.

```rust
app.add_plugin(BvhPlugin);
```

entities with `Aabb3d` are inserted into the BVH automatically each frame.
no other changes needed. use `BvhPlugin` when you have hundreds or more
culled entities.

## portal culling (`PortalPlugin` + `BspPlugin`)

for **indoor levels** (corridors, rooms), portals eliminate entire wings of
the level when a door or opening is out of the camera frustum.

tag entities with the `Area` they belong to:

```rust
// entity is in room 0
commands.spawn((
    Mesh3dBundle { .. },
    Area(0),
));

// entity is in room 1
commands.spawn((
    Mesh3dBundle { .. },
    Area(1),
));
```

place `Portal` entities at openings between areas:

```rust
use lunar::lunar_bsp::{Portal, PortalPlugin};

commands.spawn((
    LocalTransform3d::from_xyz(0.0, 1.0, 5.0),
    WorldTransform3d::default(),
    Portal {
        area_a: 0,
        area_b: 1,
        half_extents: Vec2::new(1.0, 2.0),  // portal opening size
    },
));
```

register the plugin:

```rust
app.add_plugin(PortalPlugin);
app.add_plugin(BspPlugin);  // includes BVH; BspPlugin depends on BvhPlugin
```

the portal system runs a BFS from the camera's current area, only traversing
portals visible within the camera frustum. entities in unreachable areas are
skipped before the GPU sees them — a closed door eliminates an entire wing
at zero GPU cost.

entities without an `Area` component are **always visible** — portal culling
only prunes tagged entities.

## combining strategies

typical setups:

| scene type | recommended |
|-----------|-------------|
| small/simple | just `Aabb3d` on meshes (frustum cull) |
| open world (forests, terrain) | `BvhPlugin` + `Aabb3d` |
| indoor level (rooms, corridors) | `BspPlugin` + `PortalPlugin` + `Area` on all static geometry |
| mixed (open world with interiors) | all three |
