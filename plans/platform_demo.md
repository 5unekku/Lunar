# platform demo — implementation plan

target: a ~4×4 m grass platform with hard walls, first-person camera, and a solid sky
with a sun disc overhead. no assets, no GLTF, no shadows — procedural geometry only.
the demo exists to exercise the 3D API end-to-end and expose usability problems early.

---

## target scene description

- **floor**: a 4 m × 4 m horizontal quad at y=0, grass color HSV(4π/3, 1, 0.75)
- **walls**: four invisible AABB colliders 4 m wide × 2 m tall along the perimeter;
  the player cannot walk off the platform edge
- **camera**: first-person, starts at (0, 1.7, 0); yaw is free; pitch clamped to
  [0°, 180°] zenith angle (0 = straight up, 90 = horizontal, 180 = straight down)
- **sky**: solid blue HSV(7π/3 mod 2π, 0.5, 1), rendered as a large unlit skydome
  sphere centered on the camera
- **sun**: solid disc/quad HSV(2π/3, 0.8, 1) placed at the top of the skydome (directly
  overhead); a flat circle mesh or a small billboard square, whichever is simpler

---

## gap analysis — what must be built

the render engine (`lunar-render`) is pure 2D. `lunar-3d` has a complete data model
(meshes, materials, lights, transforms, visibility) but nothing actually draws to the
screen. every item below is a hard blocker for the demo.

---

## item 1 — `lunar-render-3d` crate (BLOCKER — nothing renders without this)

new crate alongside `lunar-render`. depends on `lunar-3d`, `lunar-math`, `wgpu`.
the 2D renderer is unmodified.

### 1.1 — wgpu device sharing

the 2D renderer owns the wgpu device/queue/surface. the 3D renderer needs the same
device. two options:

- **A** (preferred): `RenderEngine` exposes `device()` / `queue()` getters so
  `RenderEngine3d` borrows them each frame via `NonSend<RenderEngine>`.
- **B**: extract device/queue into a shared `GpuContext` resource both renderers hold
  a reference to.

use option A first — less restructuring of the existing 2D code.

**changes to `lunar-render`:**
- add `pub fn device(&self) -> &wgpu::Device` and `pub fn queue(&self) -> &wgpu::Queue`
  getters on `RenderEngine` (currently private)
- add `pub fn surface_format(&self) -> wgpu::TextureFormat` getter (needed to create
  the 3D pipeline with a matching color attachment format)

### 1.2 — depth buffer

the 3D render pass needs a depth texture. 2D doesn't. `RenderEngine3d` owns this.

- `wgpu::TextureFormat::Depth32Float`
- recreated on window resize (listen for surface config changes, or check dimensions
  each frame and recreate if mismatched)

### 1.3 — unlit 3D pipeline

minimum shading model needed for the demo (grass quad + skydome both use unlit).

**vertex shader inputs** (matches `Vertex3d` layout):
- location 0: position (vec3)
- location 1: normal (vec3) — unused in unlit but must match the vertex layout
- location 2: tangent (vec4)
- location 3: uv (vec2)
- location 4: uv_lightmap (vec2)
- location 5: color (vec4, unpacked from u8×4)

**uniforms — bind group 0:**
- binding 0: `Globals` (view_proj: mat4x4<f32>)

**uniforms — bind group 1 (per draw):**
- binding 0: `Model` (model: mat4x4<f32>)
- binding 1: `Material` (base_color: vec4<f32>)

**fragment output**: `base_color × vertex_color` (no texture in the demo — pure color)

later a textured variant adds a sampler + texture to bind group 1 and branches on a
`has_texture` flag, but that is not required for the demo.

### 1.4 — mesh GPU upload

```
struct GpuMesh {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    index_format: wgpu::IndexFormat,  // Uint16 or Uint32
}
```

`RenderEngine3d` maintains a `HashMap<HandleId, GpuMesh>` keyed on the mesh asset id.
on first use of a handle, upload to GPU. `MeshUsage::Streaming` re-uploads every frame.
`MeshUsage::Static` uploads once and never again.

vertex buffer layout is `Vertex3d` packed tightly (no padding needed — all fields are
f32 or u8×4 already 4-byte aligned).

### 1.5 — per-material bind group cache

```
struct GpuMaterial {
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,  // base_color f32×4
}
```

same pattern: `HashMap<HandleId, GpuMaterial>`. upload once on first use, update when
material data changes (not needed for the demo since materials are static).

### 1.6 — render system

`fn render_3d(world: &mut World)` — exclusive world system, runs in the Render stage.

steps per frame:
1. get the surface texture from the 2D renderer (or acquire a new one — see note below)
2. build the command encoder
3. run the 3D render pass:
   a. color attachment = current surface view; load op = `Load` (2D renders first and
      clears the screen; 3D composites on top)
   b. depth attachment = `RenderEngine3d`'s depth texture; load op = `Clear(1.0)`
4. set pipeline, globals bind group (view_proj from active camera)
5. iterate all entities with `Mesh3d + WorldTransform3d + Material3d + ComputedVisibility`
   where `ComputedVisibility.0 == true`; for each: set model bind group, draw indexed
6. submit

**note on frame ordering**: the 2D renderer currently acquires the surface texture, draws
into it, and presents it all inside its render call. to composite 3D underneath 2D, the
3D pass must run before the 2D pass within the same frame — or the 2D renderer must be
restructured to separate acquire/present from draw. for the demo the simplest approach
is: 3D renders to the surface with `Clear` load op, then 2D renders on top with `Load`.
this requires exposing `acquire_surface_texture()` / `present()` separately, OR running
3D before 2D registration order in the same stage.

simplest path for the demo: register `render_3d` before `render_2d` in the Render stage.
the 3D pass clears with the sky color and draws geometry; the 2D pass composites on top
with `Load`. adjust the load op for the 2D clear pass.

### 1.7 — `RenderPlugin3d`

`struct RenderPlugin3d;` implements `GamePlugin`. `build()`:
- inserts `RenderEngine3d` resource (NonSend — wgpu types are not Send)
- adds `render_3d` to UpdateStage::Render (before the 2D render system)

---

## item 2 — sky system

a skydome that wraps the entire scene in a solid color + sun disc.

### 2.1 — `Sky` resource

```rust
pub struct Sky {
    pub sky_color: Color,
    pub sun_color: Color,
    pub sun_radius: f32,     // solid angle in degrees, default 5.0
    pub show_sun: bool,
}
```

insert via `app.insert_resource(Sky::default())`.

### 2.2 — skydome mesh

at setup time (or lazily on first render), `RenderEngine3d` generates:
- a large sphere (radius = far_plane × 0.9, ~900 units) via `primitives::sphere_mesh()`
- face normals flipped inward (all normals negated, or winding order reversed)
  so the inside surface is visible
- uploaded as a `GpuMesh` under a reserved handle id (not in the asset server)

alternatively: large quad that acts as a fullscreen sky plane. the skydome sphere is
cleaner because it works naturally with any camera rotation.

### 2.3 — sun disc mesh

a flat circle as a triangle-fan disc in the XZ plane, positioned at skydome_top:
- 32 triangles, radius = `sin(sun_radius_radians) × skydome_radius`
- centered at `Vec3::new(0, skydome_radius × 0.99, 0)` in world space (directly above)
- alternative: a simple `quad_mesh()` for a square sun if the circle is annoying

the sun mesh is generated once and uploaded as a static `GpuMesh`.

### 2.4 — sky render step

inside `render_3d` (before opaque geometry):
1. draw skydome with sky_color, depth writes OFF (`depth_write_enabled: false`),
   model matrix = camera world position (skydome follows the camera so it never clips)
2. draw sun disc with sun_color on top of the skydome (same depth write disabled)
3. then draw opaque scene geometry with depth writes ON

needs a second pipeline variant: unlit + depth write disabled + front-face culling
flipped (Back → Front or None, since normals are inward on the dome).

---

## item 3 — cursor lock (relative mouse mode)

without this, mouse look stops working when the cursor hits the window edge. the user
can't turn around.

### 3.1 — input API

add to `InputState`:

```rust
pub fn set_cursor_locked(&mut self, locked: bool);
pub fn is_cursor_locked(&self) -> bool;
```

internally calls `sdl3::mouse::set_relative_mouse_mode(locked)` (native) or
`web_sys::HtmlCanvasElement::request_pointer_lock()` (WASM).

on native, relative mouse mode means SDL delivers motion events as deltas regardless
of cursor position — exactly what `mouse_delta()` already accumulates. no other change
needed in the input system; `mouse_delta()` already works.

### 3.2 — prelude export

re-export `InputState` already happens via `lunar_input`. the new method is
automatically available.

---

## item 4 — `MeshData::compute_smooth_normals` (minor, useful for the sphere)

the sphere generated by `primitives::sphere_mesh` uses `Vertex3d::new()` which already
sets correct normals per vertex (spherical normals = normalize(position)). so this is
not strictly needed for the demo. document as follow-up.

---

## item 5 — the demo example itself

**file**: `examples/platform_demo/main.rs`

### scene setup system

```rust
fn setup(mut commands: Commands, assets: ResMut<AssetServer>) {
    // floor — 4m × 4m grass quad at y=0
    let floor_mesh   = assets.add_mesh(quad_mesh(2.0, 2.0));  // half_x, half_z
    let floor_mat    = assets.add_material(MaterialData {
        shading: ShadingModel::Unlit,
        base_color: GRASS_COLOR,
        ..default()
    });
    commands.spawn(Mesh3dBundle {
        local: LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
        mesh: Mesh3d(floor_mesh),
        material: Material3d(floor_mat),
        ..default()
    });

    // boundary colliders — four invisible AABB walls
    let wall_h = ColliderShape3d::Aabb { half_extents: Vec3::new(0.1, 1.0, 2.0) };
    // north/south/east/west walls at ±2.0 on each axis

    // camera — starts at eye height
    commands.spawn(Camera3dBundle {
        local: LocalTransform3d::from_xyz(0.0, 1.7, 0.0),
        ..default()
    });
}
```

### FPS controller system

```rust
fn fps_controller(
    input: Res<InputState>,
    time: Res<Time>,
    mut camera: Query<&mut LocalTransform3d, With<Camera3d>>,
    mut yaw: Local<f32>,
    mut pitch: Local<f32>,  // zenith angle: 0=up, π/2=horizontal, π=down
) {
    let (dx, dy) = input.mouse_delta();
    let sensitivity = 0.002;

    *yaw   -= dx * sensitivity;
    *pitch  = (*pitch + dy * sensitivity).clamp(0.001, std::f32::consts::PI - 0.001);

    // build orientation from yaw + pitch
    // forward in camera space at this pitch/yaw:
    let dir = Vec3::new(
        pitch.sin() * yaw.sin(),
        pitch.cos(),
        pitch.sin() * yaw.cos(),
    );

    // movement — WASD in the horizontal plane only (no flying)
    let forward = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
    let right   = Vec3::new(forward.z, 0.0, -forward.x);
    let speed   = 4.0 * time.delta_seconds();
    let mut pos = camera.single().translation;

    if input.is_key_held(KeyCode::W) { pos += forward * speed; }
    if input.is_key_held(KeyCode::S) { pos -= forward * speed; }
    if input.is_key_held(KeyCode::A) { pos -= right   * speed; }
    if input.is_key_held(KeyCode::D) { pos += right   * speed; }

    // clamp to platform — 2 m from center on each axis
    pos.x = pos.x.clamp(-1.9, 1.9);
    pos.z = pos.z.clamp(-1.9, 1.9);
    pos.y = 1.7; // fixed eye height

    let mut transform = camera.single_mut();
    transform.translation = pos;
    transform.rotation = Quat::from_euler_yxz(*yaw, 0.0, 0.0)
        * Quat::from_axis_angle(Vec3::X, *pitch - std::f32::consts::FRAC_PI_2);
}
```

### plugin wiring

```rust
struct PlatformDemo;
impl GamePlugin for PlatformDemo {
    fn build(&mut self, app: &mut App) {
        app.add_plugin(Plugin3d);
        app.add_plugin(RenderPlugin3d);
        app.insert_resource(Sky {
            sky_color: SKY_COLOR,
            sun_color: SUN_COLOR,
            show_sun: true,
            ..default()
        });
        app.add_system(setup.run_once());
        app.add_system(fps_controller);
    }
}
fn main() {
    lunar::bootstrap::<PlatformDemo>(RenderConfig::default());
}
```

---

## implementation order (dependency chain)

```
1 → 1.1 (device sharing) → 1.2 (depth buffer) → 1.3 (unlit pipeline)
                                                 → 1.4 (mesh upload)
                                                 → 1.5 (material bind groups)
                                                 → 1.6 (render system)
                                                 → 1.7 (RenderPlugin3d)
                         → 2 (sky, depends on 1.3 pipeline + 1.6 render system)
3 (cursor lock — independent, can do anytime)
5 (demo — depends on 1, 2, 3 all done)
```

estimated effort: item 1 is the bulk (~400–600 lines of wgpu boilerplate + WGSL shaders).
items 2 and 3 are each small (~100 lines). item 5 is ~120 lines of game code.

---

## what is explicitly NOT needed for this demo

- GLTF loading (all geometry is procedural)
- shadow maps (unlit shading, no shadow pass)
- per-pixel lighting / phong / PBR (unlit is sufficient)
- raycasting (wall enforcement is positional clamping)
- LOD, particles, decals, terrain, navmesh
- the `lunar-animation` crate or skeletal animation
- audio
