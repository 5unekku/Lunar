# 3d lighting

## ambient light

`AmbientLight` is a resource that sets the scene-wide base light level.
without it, unlit areas are completely black.

```rust
fn setup(mut commands: Commands) {
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 0.1,   // 0.0 = pitch black, 1.0 = full white
    });
}
```

for more realistic ambient (sky lighting, bounced light), use `IrradianceSH`
instead — it replaces the flat `AmbientLight` ambient term with a directional
spherical harmonic probe evaluated per-surface-normal. see `lunar::lunar_3d::IrradianceSH`.

## directional light

infinite distance, uniform across the scene. models a sun or moon.
direction comes from the entity's `WorldTransform3d` forward vector — only
rotation matters, position is irrelevant.

```rust
fn setup(mut commands: Commands) {
    commands.spawn(DirectionalLightBundle {
        local: LocalTransform3d::default()
            .with_rotation(Quat::from_euler(
                glam::EulerRot::XYZ,
                -0.8,   // pitch down ~45°
                0.4,    // yaw
                0.0,
            )),
        light: DirectionalLight {
            color: Color::rgba(1.0, 0.95, 0.85, 1.0),  // warm sunlight
            illuminance: 80_000.0,   // lux: 80k ≈ full sun, 1k ≈ overcast
            casts_shadows: true,
        },
        ..Default::default()
    });
}
```

## point light

emits in all directions from the entity's world position. attenuates to zero at `radius`.
keep `radius` as tight as possible — the renderer only shades surfaces within the sphere.

```rust
commands.spawn(PointLightBundle {
    local: LocalTransform3d::from_xyz(2.0, 3.0, 0.0),
    light: PointLight {
        color: Color::rgba(1.0, 0.6, 0.2, 1.0),  // warm orange
        intensity: 800.0,   // candela
        radius: 15.0,       // world units
        casts_shadows: false,
    },
    ..Default::default()
});
```

## spot light

cone of light from the entity's position in its forward direction.
`inner_angle` is the fully-lit cone, `outer_angle` is where it fades to zero.

```rust
commands.spawn(SpotLightBundle {
    local: LocalTransform3d::from_xyz(0.0, 5.0, 0.0)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), // point down
    light: SpotLight {
        color: Color::WHITE,
        intensity: 1200.0,
        radius: 20.0,
        inner_angle: 0.2,   // ~11°, fully lit
        outer_angle: 0.4,   // ~23°, fades to zero at edge
        casts_shadows: true,
    },
    ..Default::default()
});
```

## shadows

shadows are opt-in per light. enable `casts_shadows: true` on the light,
then add `ShadowCaster` to entities that should cast shadows:

```rust
commands.spawn((
    Mesh3dBundle { .. },
    ShadowCaster,
));
```

shadow quality (cascade count, resolution) is controlled via `QualitySettings`
(see `3d/quality.md`). enabling shadows on many point lights is expensive —
prefer one or two shadow-casting lights with the rest shadow-free.

## lightmaps (baked static lighting)

for static geometry that never moves, baked lightmaps eliminate all real-time
lighting cost on those surfaces. attach a `Lightmap` component to the entity:

```rust
fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let lightmap = assets.load_texture("lightmaps/level_01.png");

    commands.spawn((
        Mesh3dBundle { .. },
        Lightmap {
            texture: lightmap,
            intensity: 1.0,
        },
    ));
}
```

the mesh must have a secondary UV channel (`uv_lightmap` on `Vertex3d`) that
addresses into the lightmap texture. use `LightmapBaker` (from `lunar::lunar_lightmap`)
to generate the texture offline from your scene geometry.
