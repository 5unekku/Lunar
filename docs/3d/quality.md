# 3d render quality

`QualitySettings` is a resource inserted by `RenderPlugin3d` with defaults
derived from the detected GPU capability tier. game code can read and write it
at any time — changes apply next frame.

## quality presets

```rust
use lunar::lunar_render_3d::QualitySettings;

// set from a preset
fn apply_settings(mut quality: ResMut<QualitySettings>) {
    *quality = QualitySettings::minimum();  // accessibility floor — all post-fx off
    // or:
    *quality = QualitySettings::from_tier(RenderTier::detect(adapter_info));
}
```

preset constructors: `minimum()`, `low()`, `medium()`, `high()`, `ultra()` (via `from_tier_and_preset`).

`QualityPreset` enum: `Minimum`, `Low`, `Medium`, `High`, `Ultra`.

## individual toggles

override specific features without changing the full preset:

```rust
fn setup_quality(mut quality: ResMut<QualitySettings>) {
    quality.bloom = true;
    quality.ssao = false;
    quality.fxaa = true;
    quality.staa = false;
    quality.volumetric_fog = false;
    quality.render_scale = 0.75;   // render at 75% resolution, upscale to native
    quality.upscale_mode = UpscaleMode::Fsr3;
}
```

## `QualitySettings` fields

| field | type | description |
|-------|------|-------------|
| `preset` | `QualityPreset` | the coarse preset in use |
| `shadow_res` | `u32` | shadow cascade map resolution per side (pixels) |
| `point_shadow_res` | `u32` | point light shadow cubemap face size (256/512/1024) |
| `shadow_cascades` | `u32` | number of shadow cascades (1 on Low, 3 on Mid/High) |
| `msaa_samples` | `u32` | MSAA sample count: 1 = off, 4 = 4× |
| `bloom` | `bool` | bloom post-pass |
| `bloom_mips` | `u32` | bloom downsample mip levels |
| `ssao` | `bool` | half-res GTAO ambient occlusion |
| `fxaa` | `bool` | FXAA post-process AA (for low tier, no MSAA) |
| `staa` | `bool` | selective TAA — stabilizes shimmer on edges |
| `ssr` | `bool` | quarter-res screen-space reflections |
| `volumetric_fog` | `bool` | quarter-res ray-marched volumetric fog |
| `vignette` | `bool` | screen-edge vignette |
| `chromatic_aberration` | `bool` | lens chromatic aberration |
| `film_grain` | `bool` | film grain overlay |
| `particle_cap` | `u32` | maximum live particles |
| `render_scale` | `f32` | resolution scale: 1.0 = native, 0.75 = 75% |
| `upscale_mode` | `UpscaleMode` | algorithm used when `render_scale < 1.0` |

`UpscaleMode` variants: `Nearest`, `Linear`, `Lanczos`, `Bicubic`, `Fsr3`.

## gpu tier detection

`RenderTier` is detected automatically from the GPU's reported capabilities.
`RenderPlugin3d` derives `QualitySettings` defaults from it:

| tier | capabilities | typical hardware |
|------|-------------|-----------------|
| `LowGles` | no compute shaders | integrated, mobile, GLES |
| `Mid` | compute but no indirect | mid-range discrete |
| `High` | compute + indirect draw | modern discrete GPU |

the `Minimum` preset runs at acceptable fps on any tier. games should expose
a quality slider and let players choose — don't auto-detect and assume.

## sky and atmosphere

`Sky` and `AtmosphericScattering` are resources from `lunar_render_3d`:

```rust
use lunar::lunar_render_3d::{AtmosphericScattering, Sky};

fn setup(mut commands: Commands) {
    commands.insert_resource(Sky {
        sun_direction: Vec3::new(0.3, 0.8, 0.2).normalize(),
        sun_color: Color::rgba(1.0, 0.95, 0.8, 1.0),
        sun_intensity: 10.0,
        sky_color: Color::rgba(0.4, 0.6, 1.0, 1.0),
    });

    // optional: physically-based Rayleigh/Mie scattering
    commands.insert_resource(AtmosphericScattering::default());
}
```

## fog

`Fog` is a component or resource that applies depth fog to the scene:

```rust
use lunar::prelude::*;

fn setup(mut commands: Commands) {
    commands.insert_resource(Fog {
        color: Color::rgba(0.6, 0.7, 0.8, 1.0),
        falloff: FogFalloff::Linear { start: 50.0, end: 200.0 },
    });
}
```

`FogFalloff` variants:
- `Linear { start, end }` — linear fog between start and end distances
- `Exponential { density }` — exponential fog density
- `ExponentialSquared { density }` — denser exponential falloff
