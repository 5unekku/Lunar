# 3d materials

meshes and materials are registered in `MeshRegistry` (a resource inserted by `Plugin3d`)
and referenced by handle. they are not assets loaded from disk — they live in the registry.

## creating a material

```rust
use lunar::prelude::*;

fn setup(mut registry: ResMut<MeshRegistry>, mut assets: ResMut<AssetServer>) {
    // simple colored material
    let flat = registry.add_material(MaterialData {
        base_color: Color::rgba(0.8, 0.3, 0.1, 1.0),
        shading: ShadingModel::Phong,
        ..Default::default()
    });

    // PBR material with textures
    let diffuse = assets.load_texture("textures/wall_albedo.png");
    let normal = assets.load_texture("textures/wall_normal.png");
    let specular = assets.load_texture("textures/wall_roughness_metallic.png");

    let pbr = registry.add_material(MaterialData {
        shading: ShadingModel::Pbr,
        base_color: Color::WHITE,
        diffuse: Some(diffuse),
        normal_map: Some(normal),
        specular: Some(specular),  // R = roughness, G = metallic
        roughness: 0.7,
        metallic: 0.0,
        ..Default::default()
    });
}
```

## `MaterialData` fields

| field | type | description |
|-------|------|-------------|
| `shading` | `ShadingModel` | `Unlit`, `Phong`, or `Pbr` |
| `cull` | `CullMode` | `Back` (default), `Front`, `None` |
| `base_color` | `Color` | RGBA multiplied with diffuse texture |
| `diffuse` | `Option<Handle<Texture>>` | albedo texture |
| `normal_map` | `Option<Handle<Texture>>` | tangent-space normal map (XY only, Z reconstructed) |
| `specular` | `Option<Handle<Texture>>` | phong: intensity map; pbr: R=roughness, G=metallic |
| `specular_intensity` | `f32` | phong shininess (8–128); pbr metallic factor |
| `metallic` | `f32` | pbr: 0.0 = dielectric, 1.0 = full metal |
| `roughness` | `f32` | pbr: 0.04 = mirror-smooth, 1.0 = fully diffuse |
| `alpha` | `f32` | < 1.0 enables alpha blending |
| `depth_write` | `bool` | false for decals, transparent surfaces, particles |

`ShadingModel` variants:
- `Unlit` — no lighting, full-bright (HUD elements, debug)
- `Phong` — classic diffuse + specular (Quake 3 / Doom 3 baseline, default)
- `Pbr` — metallic-roughness physically-based rendering

## normal map convention

normal maps use the **Doom 3 / id Tech 4 convention**: store only XY tangent-space
components in R and G. do not store Z — the shader reconstructs it as `sqrt(1 - x² - y²)`.
this allows packing other data in the B channel.

## transparent materials

set `alpha < 1.0` and `depth_write: false`:

```rust
let glass = registry.add_material(MaterialData {
    shading: ShadingModel::Pbr,
    base_color: Color::rgba(0.8, 0.9, 1.0, 0.3),
    alpha: 0.3,
    depth_write: false,
    roughness: 0.05,
    ..Default::default()
});
```

## creating a mesh

use built-in primitives or supply your own `MeshData`:

```rust
use lunar::lunar_3d::primitives::{quad_mesh, sphere_mesh, cube_mesh};

let sphere = registry.add_mesh(sphere_mesh(1.0, 32, 16));  // radius, longitude, latitude segments
let quad = registry.add_mesh(quad_mesh(2.0, 3.0));          // width, height
let cube = registry.add_mesh(cube_mesh(1.0));               // side length
```

for custom geometry, fill a `MeshData` and register it:

```rust
use lunar::lunar_3d::{MeshData, Vertex3d};

let mesh_data = MeshData {
    vertices: vec![
        Vertex3d {
            position: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            uv_lightmap: [0.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
        },
        // ...
    ],
    indices: vec![0, 1, 2],
};

let handle = registry.add_mesh(mesh_data);
```

## assigning to an entity

```rust
commands.spawn(Mesh3dBundle {
    local: LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
    mesh: Mesh3d(mesh_handle),
    material: Material3d(material_handle),
    ..Default::default()
});
```

to change material at runtime, query and overwrite:

```rust
fn swap_material(
    mut query: Query<&mut Material3d, With<Player>>,
    registry: Res<MeshRegistry>,
) {
    for mut material in &mut query {
        material.0 = new_material_handle;
    }
}
```
