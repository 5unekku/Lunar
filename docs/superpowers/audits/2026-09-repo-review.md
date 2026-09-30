# repo-wide review — performance, code quality, capabilities

date: 2026-09-29
scope: whole workspace, three lenses: performance, code quality / organization /
cleanliness, and capabilities (what the engine can and cannot do today).

method: read the six 2026-07 audits and the harness runtime-bug doc first, then
reviewed the code for issues those documents do **not** already cover. every finding
below was checked against the source at the cited lines. `cargo clippy --workspace
--all-targets` was run on the current nightly (results in "tooling signals"). the
2d upload finding was measured with render-bench on lavapipe (see rev-01), and
rev-06 was reproduced with a headless pixel test.

this doc does not repeat audit findings. where a new finding shares a root cause
with an existing id, the id is cited.

**status of the 2026-07 program:** phase 0 (harness) and phase 1 (six audits) are
done; the phase-2 synthesis backlog
(`docs/superpowers/specs/2026-07-<date>-improvement-backlog-design.md`) was never
written, `docs/bench/` holds no baseline, and of ~110 audit findings only rt-01 is
fixed. the single highest-leverage next step is still the backlog + baseline: every
perf item in the program is gated on "before/after harness numbers", and without a
committed baseline nothing in wave 2 can land under the program's own rules.

## fix-loop status (2026-09-29)

every finding below was fixed test-first on this branch unless marked open. new
findings from the fix loop are rev-13..rev-17.

| id | finding | status |
|---|---|---|
| rev-01 | 2d per-quad `write_buffer` | fixed `55dc7f8` (sprite-storm ~2x on lavapipe, golden identical) |
| rev-02 | pvs bake unaccelerated | fixed `54c1e84` (triangle bvh, bit-identical, 4901 → 171 ms; rng bug rev-15) |
| rev-03 | 2d collision O(n²) | fixed `63f7ef8` (52.1 → 1.1 ms for 5k colliders, brute-force equivalence test) |
| rev-04 | mips on main thread / srgb | threading fixed `1cabefe` (+ bc textures no longer box-filtered); **srgb-space averaging open**: color space is decided per use at upload, so a correct fix needs a color-space hint on the asset or gpu mip generation |
| rev-05 | bevy_reflect pulled in | fixed `8309a05` |
| rev-06 | 2d layer projections collapse | fixed `3f5ec2b` |
| rev-07 | text overruns vertex buffer | fixed `0a5a6fb` |
| rev-08 | self-despawn skips on_destroy | fixed `46c9996` |
| rev-09 | bind groups built in several places | water bg0 consolidated in `d06909b`; composite / ssr / fog / decal / atmos / fxaa still duplicated (open, fold into arch-02) |
| rev-10 | auto-trait overflow | fixed `f307f4b`; **cause corrected below** |
| rev-11 | out-of-workspace tools | bake-pvs fixed `ddce8ea`; gen-lods disconnect open (capability) |
| rev-12 | small items | docs fixed `2172328`; dead letterbox code removed in `3f5ec2b`; clippy clean `f307f4b` |
| rt-02 / rt-03 | detail sprites, water abort | fixed `d06909b`; both back in feature-reel `d116de8` |
| rev-13 | `Fog` ignored, volumetric fog hardcoded | fixed `df87264` |
| rev-14 | terrain ignores the sun | fixed `7907982` |
| rev-15 | pvs rng samples half of each leaf | fixed `54c1e84` |
| rev-16 | ci never runs (actions policy) | ci.yml fixed `a7b5cad`; docs.yml needs a settings change |
| rev-17 | gtao + ssr darken every max-quality frame | open (design; needs sign-off) |
| rev-18 | directional shadows never held a caster | fixed `ac83446` (cascade ortho near/far sign; per-frame cascade rendering) |
| rev-19 | z-prepass / colour-pass depth mismatch drops pixel rows | fixed `3ab32dd` |
| rev-20 | point shadows ignore moving casters | fixed `af53dab`; off-screen casters still cast nothing (open) |
| rev-21 | water / particles / detail sprites discard scene depth | fixed `ba70282` |
| rev-22 | water / decal params last-write-wins across entities | fixed `ba70282` |
| rev-23 | terrain lacks pbr exposure (darker than meshes) | open (visual; needs sign-off) |
| rev-24 | gpu indirect cull appends draws with `atomicAdd`: draw order follows workgroup scheduling, so exactly-tied depths (building bases) flip frame to frame; static-city golden was capture-unstable | fixed `46c9eb2` (slot-stable draws, `atomicMax` count; stable in and across processes, perf neutral 353.4 → 345.9 ms) |
| rev-26 | the sky's sun disc is always drawn straight above the camera (`frame.rs` `sun_model`), ignoring the `DirectionalLight` direction, so the disc and the lighting disagree whenever the sun isn't at noon | open (visual; needs sign-off) |
| rev-27 | docs promised APIs that don't exist: `mouse_scroll_delta` (now implemented), `RenderTier::detect`, `RenderLayers::from_layers`, `get_single_mut`, and stale fields on `SpriteAnimation`, `CameraFollow2d` (a resource, shown as a component), `AmbientLight`, `Sky`, `RenderConfig3d` | fixed `6876ae9`, `8b91336` and the follow-up doc commit |
| rev-28 | the gpu lod-select path was dead: its pipeline was never created, yet high-tier frames sized its buffers every frame | removed `56591d5` (-261 lines) |
| rev-29 | the wasm cross-compile test (`tests/cross_compile.rs`) failed on the native-only render-bench | fixed `987466b` |
| rev-30 | 2d glyph atlas: once full, every glyph that didn't fit was cached as "no glyph" forever, so new characters/sizes never rendered again | fixed `01bdc63` (flush between frames) |
| rev-31 | 3d lightmap atlas: a lightmap with no atlas slot sampled the whole atlas as its lighting | fixed `1ea12a4` |
| rev-32 | shadow casters outside the camera frustum cast nothing, for cascades as well as point lights: every shadow list is built from the camera-visible `draw_scratch`, so a building behind the camera never shadows the view and shadows pop as the camera turns (rev-20's "off-screen casters" is one case of this) | open (L: shadow-only instances need their own slot range after the surface slots, threaded through the static bundle, gpu-driven and hzb paths) |
| rev-25 | ci build-wasm used the dev profile, which pins cranelift (no wasm32 backend) | fixed `7261cce` (llvm override, as build-cross does) |

### 2026-07 audit findings fixed on this branch

corr-01..08, corr-10..14, corr-16..40, corr-43, corr-44 are fixed test-first, one
commit each (`git log --grep corr-`). corr-15 (scene-loader parent cycles) is
covered by the cycle-safe hierarchy walk from corr-01. corr-09 (spot lights never
rendered) is fixed in `97f7689`: spots ride the point-light path with a cone term.
every correctness finding is now addressed.

security: all of sec-01..17 are addressed. sec-02 was covered by corr-03; sec-01
and sec-15 are contract/doc fixes (component pointers die at structural changes;
RegisteredSystem Send/Sync argument); sec-14 makes the rust dylib loader `unsafe`
with a stated contract (the C-ABI shim is still the real fix); sec-05 is checked
by a unit test plus a clean windows-gnu clippy, not on a windows host.

performance (before -> after):

| id | fix | numbers |
|---|---|---|
| perf-03 | 3d collision: left prune bound + entity index (the rev-03 shape again) + quiet-tick gate | 5000 colliders, overlapping() for all 22.4 -> 5.2 ms; quiet rebuild 0.219 -> 0.021 ms |
| perf-04 | cached early-out probe queries | quiet frame, 10k entities 15.95 -> 12.17 us |
| perf-05 | build_cull_soa change gate | quiet frame, 10k boxes 390 -> 19.6 us |
| perf-06 | gpu late cull tested *local* aabbs against the world frustum: meshes vanished when the origin was off screen (correctness; the perf half is open) | goldens unchanged, timing within noise |
| perf-07 | point-shadow slots cleared once on used -> unused, re-rendered when reused (a returning light used to keep a cleared slot) | feature-reel 322/319 -> 309 ms/frame |

| perf-01 | terrain: per-ring params slots + one pass per terrain. every ring had drawn with the last ring's params (the rev-22 bug, still live for terrain) | feature-reel 308 -> 265 ms/frame, golden pixel-identical |

water/decals still open one pass per entity; feature-reel has one of each, so
there is no bench to show a gain yet. perf-02 (L) and perf-08..13 (low, below
lavapipe noise) are open.

architecture:

| id | fix | evidence |
|---|---|---|
| arch-01 | a ShadowProvider hook no longer drops the z-prepass (opaque geometry vanished on mid/high with any hook) | headless test with a no-op hook |
| arch-05 | avx2 cull kernel moved to `lunar_math::simd_cull`; lunar-3d builds with cranelift in dev | lunar-3d incremental test build 1.24 -> 1.02 s |
| arch-08 | render_graph docs no longer claim it drives pass order | doc only; kept as the arch-02 target |
| arch-12 | wgpu `spirv` feature native-only | naga gone from the wasm32 dependency graph |

arch-02/07/09 (L), arch-04/06/10/13 (M) and arch-11 (rename, churn) are open.
arch-03 (make the facade's `2d` feature gate lunar-render) is ready to do but is a
breaking change for 3d-only users of 2D types, so it waits on sign-off.

other fixes this round: rev-24 above; wasm32 `clippy -D warnings` is now clean
for the engine crates (it never was; ci only builds wasm); unused deps removed
(guillotiere, sdl3 in lunar-render, web-sys; lunar-atlas's lunar-assets).

---

## performance

### rev-01 — 2d renderer issues one `queue.write_buffer` per quad

- **location:** crates/lunar-render/src/lib.rs:1865-1868 (sprite), :1895 (rect),
  :1935 (line), :1972 (glyph); called from the draw loop at :1649-1757
- **impact:** high (the 2d hot path)
- **effort:** S

every sprite, rect, line and glyph writes its 120-byte, 6-vertex block with its own
`queue.write_buffer` call. each call is a separate validated staging write inside
wgpu. sprite-storm (20k sprites + 200 labels) makes ~21k calls a frame.

fix: append into a persistent `Vec<u8>` on the engine and issue one
`write_buffer(vertex_bufs[frame_index], 0, &cpu_verts)` before `queue.submit`. queue
writes are applied before the command buffer runs, so ordering is unchanged. the
double-buffered `vertex_bufs` also become unnecessary once there is one staged write
per frame.

measured (render-bench `--scene sprite-storm`, lavapipe/llvmpipe, 1280x720,
3 runs x 500 frames, lto off / 16 cgu for both builds):

| build | mean (ms) | p50 (ms) | p99 (ms) |
|---|---|---|---|
| current (per-quad writes) | 87.3 / 93.1 | 82.2 | 145.4 / 166.5 |
| batched (one write per frame) | 43.8 / 46.1 | — | 56.1 / 64.6 |

(two runs each, interleaved; p50 from the first run's table.) roughly **2× lower
frame time and ~2.5× lower p99**. the golden-frame check passed: a frame captured
with the current build matches the batched build pixel for pixel within tolerance.
lavapipe exaggerates cpu-side costs compared with a discrete gpu, so rerun on the
reference machine before claiming a number, but the direction is not in doubt. the
patch is about 15 lines (replace the four `write_buffer` calls with
`cpu_verts.extend_from_slice`, flush once before submit).

follow-up once batched: 6 vertices x 20 bytes per quad could become one 32–48 byte
instance with a 4-vertex strip, cutting upload bandwidth ~3x.

### rev-02 — offline PVS bake is O(leaves² × samples × triangles), unaccelerated

- **location:** crates/lunar-bsp-build/src/pvs.rs:45-88 (pair loop), :121-142
  (`leaves_see_each_other`), :152-161 (`ray_hits_any`)
- **impact:** medium (offline, but it is the tool the "front-load work offline" pitch
  depends on)
- **effort:** M

every sample ray is tested against every level triangle with no spatial structure.
1k leaves × 64 samples × 50k triangles is on the order of 10^15 ray/triangle tests.
the BSP being built in the same crate, or a BVH over the triangles, would bring each
ray to ~log T.

there is also a correctness gap: the module doc says the result is conservative
("false positives are safe; false negatives would cause pop-in"), but random ray
sampling produces exactly false negatives: a thin gap that no sample hits marks two
mutually visible leaves as hidden. `skip_distance_sq` also marks long sightlines as
hidden by design. either document that pop-in is possible, or switch to a
conservative portal-based PVS (bake-pvs's area flood is conservative but coarse).

### rev-03 — 2d collision queries are O(N) per call, O(N²) per frame in practice

- **location:** crates/lunar-2d/src/collision.rs:202-206 (`x_candidates`),
  :211-225 (`overlapping`), :228-232 (`query_point`)
- **impact:** medium
- **effort:** S

`x_candidates` ignores `_qmin_x` and returns every entry from index 0 up to the right
bound, so the "sweep-and-prune" only prunes one side. `overlapping(entity)` also
starts with a linear `find` for the entity. the common pattern of "every bullet asks
what it overlaps" is therefore O(N²). `query_point` has no pruning at all.

fix: keep an `Entity → index` map (built alongside the sort), and bound the left side
with `partition_point(|e| e.min_x < qmin_x - max_half_width)` using the widest
collider tracked during build. a uniform grid is the next step if colliders vary
widely in size.

### rev-04 — texture mip generation runs on the main thread, in sRGB space

- **location:** crates/lunar-assets/src/lib.rs:1525-1536 (`AssetServer::update`),
  :1671-1760 (`TextureData::generate_mipmaps`)
- **impact:** medium (frame hitches on load; visible darkening)
- **effort:** S

`generate_mipmaps` is on by default (`MipStreamingConfig::generate_mipmaps: true`)
and runs inside `AssetServer::update` on the game thread, not in the IO worker that
already decoded the image. rayon parallelizes it, but the main thread still blocks
until the chain is done, and every row allocates its own `Vec`. a 4k texture load
stalls a frame.

the filter also averages raw bytes, but 2d textures are uploaded as
`Rgba8UnormSrgb` (lunar-render/src/lib.rs:1271) and 3d color textures as sRGB
(lunar-render-3d/src/frame.rs:23). averaging sRGB-encoded values darkens every mip
below 0, most visibly on high-contrast detail. linearize, average, re-encode (a
256-entry LUT is enough), and move the call into the IO worker thread.

### rev-05 — `bevy_ecs` default features pull in unused reflection

- **location:** Cargo.toml `[workspace.dependencies] bevy_ecs`
- **impact:** low–medium (compile time, binary size)
- **effort:** S

`bevy_ecs = { version = "0.18", features = ["multi_threaded"] }` keeps default
features, which enable `bevy_reflect` (+ its derive macros), `async_executor` and
`backtrace`. nothing in the workspace uses reflection. `default-features = false`
plus the features actually needed (`std`, `multi_threaded`) trims a proc-macro crate
tree from every build and dead code from the release binary, in line with the
2026-06 footprint work. the perf-footprint audit did not look at this.

---

## correctness found along the way

these came up while reading hot paths. none are in the correctness audit.

### rev-06 — 2d per-layer projections collapse to the last one written

- **location:** crates/lunar-render/src/lib.rs:922-1036 (`update_projection_for_layer`),
  called mid-pass at :1668; single uniform at :662 / bind group at :717
- **impact:** high (parallax is broken; screen flashes jump the world)
- **effort:** S–M

the draw loop calls `update_projection_for_layer` each time the layer changes, and
that function does `queue.write_buffer(&self.uniform_buf, 0, ...)` to the same 64
bytes. queue writes are all applied before the command buffer executes, so every
draw in the frame sees the **last** projection written. consequences:

- `Camera::set_layer_parallax` has no effect: all layers use the final layer's offset
- any `POST_PROCESS` draw (`draw_screen_rect`, screen flash, `PostProcessStack`)
  switches the whole frame, world sprites included, to screen-space projection
- the letterboxed viewport projection is applied or lost depending on which layer is
  drawn last

confirmed with a throwaway headless test (not committed): a red layer-0 rect covering
pixel (8,8) reads `[255,0,0]` alone, and `[75,75,75]` (the clear color) once a single
4×4 POST_PROCESS rect is added to the same frame.

fix: one 256-byte-aligned slot per distinct layer in a uniform buffer with a dynamic
offset, written once before the pass; `set_bind_group(0, globals_bg, &[slot])` per
layer. a headless test can catch it: one sprite on layer 0 plus one POST_PROCESS
rect, then read back with `read_target_rgba` and check the sprite's pixel position.

### rev-07 — text can overrun the 2d vertex buffer and abort

- **location:** crates/lunar-render/src/lib.rs:1694-1697 (capacity check),
  :1747-1753 (glyph loop)
- **impact:** high (process abort under `panic = "abort"`)
- **effort:** S

the overflow guard checks room for one 6-vertex quad per command, but a `Text`
command then writes one quad per glyph. a long string drawn near capacity writes
past the end of the buffer, which is a wgpu validation error. with no uncaptured-error
handler (see the harness runtime-bug doc) that is a hard abort. fix: check
`count * 6 * VERTEX_STRIDE` for text before writing, or clamp and set
`overflow_flag`.

### rev-08 — behaviors that despawn their own entity skip `on_destroy`

- **location:** crates/lunar-core/src/behavior.rs:161-197 (`dispatch_behaviors`),
  :206-216 (`despawn_with_behaviors`)
- **impact:** medium
- **effort:** S

dispatch takes the entity's `Behaviors` list out of the component while hooks run.
if a hook calls `despawn_with_behaviors(world, ctx.entity)`, which is the documented
route for "destroy hooks always run", `dispatch_one` finds an empty list and no
`on_destroy` fires. likewise, a behavior that attaches another behavior to its own
entity during a hook is overwritten when the list is put back (`*items_mut() =
items`). fix: queue self-despawns and run them after the entity's hooks return, and
merge rather than overwrite on restore.

---

## code quality / organization

### rev-09 — resize-dependent bind groups are built in several places

- **location:** crates/lunar-render-3d/src: `[water] bg0` is built 3× (init.rs:2806,
  config.rs, post.rs:1337), `[composite] bg` 3×, `[ssr] bg0`, `[fog] bg0`,
  `[decal] bg0`, `[atmos] bg0`, `[fxaa] bg` 2× each
- **impact:** medium (the root cause behind three filed bugs)
- **effort:** M

rt-03 (water samples the live hdr target), corr-05 (bloom resources on resize) and
corr-24 (contact-shadow texture never resized) share one pattern: a bind group's
inputs are listed separately at init and at every rebuild site, and the copies
drift. before the arch-02 per-feature split lands, a cheap step is one
`fn build_<feature>_bg(&self) -> wgpu::BindGroup` per group, called from init, resize
and msaa rebuild, so each group's layout is defined in exactly one place.

### rev-10 — auto-trait checks on the render engines overflow the recursion limit

- **location:** crates/lunar-render-3d/src/lib.rs:1472, crates/lunar-render/src/lib.rs:430
  (plus the `Resource` bound sites in lunar/src/bootstrap.rs:111,
  headless_probe, render-bench)
- **impact:** medium (ci's clippy `-D warnings` fails on this; worse on future nightlies)
- **effort:** L (the fix is arch-02); S workaround

clippy on current nightly warns "overflow evaluating the requirement
`RenderEngine3d: Sync`" and the same for `RenderEngine` and the `Resource` bounds.

**correction:** the first draft blamed the struct's size. the full diagnostic shows
the depth comes from wgpu's own handle types (`TextureView` → `Texture` →
`DispatchTexture` → `Arc<CoreTexture>` → `ContextWgpuCore` → `wgpu_core::Global` →
hub registries), not from `RenderEngine3d`'s field count, so it is not evidence
for arch-02. rustc flags it as a future hard error. fixed with
`#![recursion_limit = "256"]` on the crates that check auto traits on those types.

### rev-11 — out-of-workspace tools duplicate engine types, and one pipeline is disconnected

- **location:** tools/bake-pvs, tools/gen-lods, tools/compress-textures,
  tools/gen_assets (each has its own `[workspace]` and Cargo.lock)
- **impact:** medium
- **effort:** S–M

- `bake-pvs` re-declares `BspBlob`, `BspNode` and `PortalData` by hand
  (tools/bake-pvs/src/main.rs:53-61) and relies on bincode field order matching
  lunar-bsp/src/level.rs:55-71. nothing builds it in ci, so a field added on either
  side silently corrupts levels. depend on `lunar-bsp` instead, and add these tools to
  the workspace so clippy and ci see them.
- `gen-lods` writes `mesh_lodN.bin` + `mesh.lod.ron`, but nothing in the engine reads
  either file, or its flat `.bin` input format. `MeshLod` only takes handles to meshes
  built in code. the README advertises "LOD generation" as part of the offline
  pipeline, but today it is a dead end (see capabilities).
- `gen_assets` is a 28-line placeholder.

### rev-12 — smaller quality items

- **2d projection dead code:** lib.rs:968-977 computes `_sx` / `_sy` and a derivation
  comment ending "Actually simpler: compute clip directly". delete the dead path.
- **game-loop docs contradict the code:** game_loop.rs:12 ("capped at 5"), :62
  ("0-5") and README ("capped at 5 ticks/frame") vs the implementation at :150-166,
  which caps *time* at 250 ms and says "never cap the count". at 240 Hz that is up to
  60 ticks in one frame.
- **mixer comment:** mixer.rs:54 calls `clamp(-1, 1)` a "soft clamp"; it is a hard
  clip.
- **mixed indentation:** 20 files (all of lunar-audio, bindings/c, plugin-loader,
  dotnet-host, scene_format_3d, xtask, examples) use spaces while rustfmt.toml sets
  `hard_tabs = true`; covered by build-12's one-time `cargo fmt` sweep.
- **README drift:** it lists a `lunar-camera-3d` crate that lives in lunar-plugins,
  and "12 triples" vs the spec's 13.
- **test density:** lunar-render-3d has 19 tests for 17.3k lines; lunar-lightmap,
  bindings/c, lunar-dotnet-host and lunar have none. rt-01..03 show that render feature
  passes regress unnoticed; one headless "every feature renders a frame" test per pass
  (the rt-01 regression test is the template) would have caught all three.

---

## new findings from the fix loop

### rev-13 — the public `Fog` resource was ignored; volumetric fog was hardcoded

- **location:** crates/lunar-3d/src/fog.rs (api), crates/lunar-render-3d/src/post.rs
  (volumetric fog pass)
- **status:** fixed `df87264`

`Fog` is exported in the prelude and documented as "insert as a resource to enable
scene fog. without this resource, no fog is applied", but nothing in the renderer
read it. volumetric fog instead ran with density 0.01, a 200-unit march and a
sky-derived color whenever the tier and dev profile allowed, i.e. in every
max-quality frame. at that density 86% of anything past ~200 units is replaced by
dark fog, which is why the static-city and feature-reel golden frames were almost
black (mean luminance ~15/255). the fog pass now derives its inputs from `Fog` and
is skipped without it. **visual change:** scenes that relied on the implicit fog
at mid+ tier lose it unless they insert a `Fog`.

### rev-14 — clipmap terrain ignored the sun

- **location:** crates/lunar-render-3d/src/passes.rs (terrain params upload)
- **status:** fixed `7907982`

terrain.wgsl expects `sun_dir` to point toward the sun, but got the light's
travel direction (the fog code negates it; terrain did not), so every
upward-facing texel had `dot(n, sun) < 0` and terrain only ever showed its 0.15
ambient. the intensity was also raw lux, so fixing the sign alone would blow out
to white; it now uses the main shader's 80 000-lux normalization.

### rev-15 — the pvs bake's rng only sampled the lower half of each leaf

- **location:** crates/lunar-bsp-build/src/pvs.rs (`Lcg::next_f32`)
- **status:** fixed `54c1e84`

`(x >> 33) as f32 / u32::MAX as f32` tops out at 0.5, so every sample point sat in
the lower half of its leaf box; a sightline through the upper half (a window over
a low wall) was never tried and the pair was marked hidden.

### rev-16 — ci never ran any step

- **location:** .github/workflows/ci.yml, docs.yml; repository actions policy
- **status:** ci.yml fixed `a7b5cad`; docs.yml needs a settings change

build-01 recorded 8/8 failed runs (the github api now shows 150 for ci). every
job failed in "Set up job" in 1–2 s: "The actions actions/checkout@v6 and
swatinem/rust-cache@v2 are not allowed in 5unekku/Lunar because all actions must
be from a repository owned by 5unekku". ci.yml now checks out with plain git and
runs without the cache action, and installs the x11 packages SDL3's from-source
build hard-requires once `libegl-dev` brings in the x11 headers (xcursor first).
docs.yml needs the pages actions, so it stays broken until the repo's actions
policy allows github-owned actions. pushes from this session did not trigger a
run, so the fixed workflow is unverified in ci itself; the checkout script and
the sdl3 build were verified locally.

### rev-17 — gtao and ssr darken every max-quality frame (open)

- **location:** crates/lunar-render-3d/src/composite.wgsl:126-141, ssr.wgsl
- **status:** open: a visual design change that needs sign-off per the program's
  golden-frame rule

with fog fixed, static-city at max quality still averages ~15/255 against ~38 with
the classic profile. turning off gtao alone lifts it to ~25, ssr alone to ~21,
both to ~35. two causes:

- composite blends ssr as `mix(hdr, reflection, alpha * 0.3)` into **every**
  surface. no roughness or F0 reaches the pass (there is no g-buffer channel for
  it), so rough diffuse ground takes a 30% mirror blend of whatever it reflects.
  ssr needs a roughness/specular weight, which means writing roughness in the
  prepass.
- composite multiplies the final hdr by ao (`hdr_color *= ao`), which darkens
  direct sunlight as well as ambient. ao should scale only the ambient/indirect
  term, i.e. be applied in the lighting shader, not after it.

feature-reel also shows horizontal striping across flat ground and black metallic
meshes (no environment to reflect); both are worth a look on the reference gpu
before trusting lavapipe.

### rev-18 — directional shadows never held a caster

- **location:** crates/lunar-render-3d/src/frame.rs (`cascade_light_space`),
  passes.rs (`record_shadows`)
- **status:** fixed `ac83446`

two independent bugs. (1) the cascade ortho got light-view z values (negative in
a right-handed view) as near/far, while glam's rh projection takes positive
distances: every slice corner landed at ndc depth > 1, so casters were clipped
out and receivers compared against the far plane (the straight-edged half-plane
"shadow" in the feature-reel golden). (2) at most one "dirty" cascade was
rendered per frame and every other cascade was cleared each frame, so in steady
state no cascade held anything; since cascades follow the camera, the caching
could not have been valid anyway. all active cascades now render every frame.
design limit noted: the shadow pipeline culls front faces, so one-sided meshes
lit from their front never cast.

### rev-19 — z-prepass and colour pass disagreed on depth

- **location:** crates/lunar-render-3d/src/shader.wgsl (`vs_depth`)
- **status:** fixed `3ab32dd`

`vs_depth` computed `view_proj * model * p` (matrix product first) and `vs_main`
`view_proj * (model * p)`. the rounding difference made the `LessEqual` colour
pass drop pixels in stair-stepped rows across large surfaces at mid/high tier.
same operation order now, positions `@invariant`. on lavapipe the raw
feature-reel frame went from mostly clear-colour rows to fully shaded.

### rev-20 — point shadows ignored moving casters

- **location:** crates/lunar-render-3d/src/passes.rs (point shadow dirty check)
- **status:** fixed `af53dab`; one limit open

faces re-rendered only when a light moved or the draw count changed. the check
now hashes the draw list's entity, mesh and model matrix. still open: point
shadows draw from the camera-visible list, so casters outside the view frustum
cast no point shadow onto visible receivers.

---

## capabilities

what a game can and cannot do today, judged against the README's claims and the
common baseline for a 2d+3d engine. "gap" means missing, not a bug.

### strong

- 3d renderer feature set is broad: clustered forward PBR, CSM + point shadows, GTAO,
  SSR, volumetric fog, bloom, STAA/FXAA, contact shadows, atmosphere/panorama skies,
  clipmap terrain, GPU particles, decals, HZB + GPU-driven culling, bindless, LOD
  selection, impostors, planar reflections, lightmaps, BSP/PVS/portals.
- ecs, fixed-tick loop with interpolation, save/load, scenes (2d + 3d RON → binary),
  world manifests with spatial chunks, behaviors with exported fields + hot reload,
  native/C/C#/C++ plugin surfaces, wasm/webgpu.

### broken or half-wired (exists in the api, does not work end to end)

| capability | state | where |
|---|---|---|
| detail sprites / grass | ~~crashes on first use~~ fixed | rt-02 |
| water | ~~crashes on first use~~ fixed | rt-03 |
| 2d parallax layers | ~~no effect~~ fixed | rev-06 |
| scene fog (`Fog`) | ~~ignored~~ fixed | rev-13 |
| skinned meshes | `SkinWeights` stored on `MeshData`, never uploaded; no skinning in any shader. `lunar-3d::animation` only animates rigid joint transforms | lunar-3d/src/mesh.rs:55-90, lunar-render-3d (no `skin` references) |
| offline LOD pipeline | `gen-lods` output is never read | rev-11 |

### gaps

- **no runtime mesh or scene import.** `AssetServer` loads textures, sounds and fonts
  only; meshes come from code (`primitives.rs`, `MeshRegistry::add_mesh`). glTF is
  parsed only by the offline BSP compiler. for a 3d engine this is the largest single
  gap: a `load_mesh("x.glb")` path (gltf crate, already in the lockfile) that fills
  `MeshRegistry` + `MaterialData`, plus skins once skinning exists.
- **audio is fire-and-forget.** `AudioPlayer::play` returns nothing: no handle to
  stop, pause, fade or change volume, no buses (music / sfx / ui), no spatial or
  panned audio, no built-in streaming. `play` fully decodes to f32 on the calling
  (game) thread on first play (decoder.rs:43-51). plugin.rs:33-38 documents this and
  tells callers to decode music off-thread themselves via `play_source`, but a
  4-minute stereo track is still ~92 MB of PCM once decoded. a streaming
  `AudioSource` for music belongs in the engine.
- **input:** no mouse wheel, no text entry / IME (so no name fields, chat, or
  console), no touch, no gamepad rumble, no clipboard. action maps are
  `&str`-keyed (a typo fails silently to `false`).
- **2d text:** the glyph atlas has a fixed size and drops glyphs once full
  (text.rs:238), so large CJK or multi-size text sets degrade silently. it needs a
  second page or LRU eviction.
- **2d ui coordinates:** `draw_ui_text` / `draw_ui_rect` convert only the position
  through the world camera, so ui size scales with zoom and rotates with the camera.
  there is no true screen-space ui layer apart from POST_PROCESS, which rev-06
  currently breaks.
- **robustness:** no `on_uncaptured_error` handler, so any wgpu validation error
  aborts a shipped game (the common thread of rt-01..03 and rev-07). release builds
  could log and skip the frame's feature pass instead.

---

## tooling signals

`cargo clippy --workspace --all-targets` (nightly 2026-09-28): builds; 23 warnings.

- 16 × `chunks_exact` → `as_chunks` (new nightly lint; mechanical)
- 5 × recursion overflow on `Sync` / `Resource` for the two render engines (rev-10)
- 1 × collapsible `if` (lunar-render-3d/src/cull.rs:252)
- 1 × manual slice fill (lunar-render-3d/src/passes.rs:1401)
- wgpu 29.0.4 / naga / wgpu-core / wgpu-hal flagged future-incompatible by rustc

every one of these fails ci's `clippy -D warnings`. that is expected with the
unpinned nightly (build-11), and one more reason to pin it.

building SDL3 from source here also needed libasound2-dev, libpulse-dev and
libxcursor-dev (+ the xi/xrandr/xss/xtst set). the ci apt lines
(.github/workflows/ci.yml:20,31,40) install none of them, which is a plausible
direct cause of build-01's 8/8 failed runs and worth checking first when ci is
revived.

---

## suggested order

1. write the phase-2 backlog and commit a bench baseline (unblocks everything else)
2. S-effort correctness: rev-06, rev-07, rt-02, rt-03, rev-08
3. S-effort perf: rev-01 (numbers above), rev-03, rev-04, rev-05
4. rev-09 bind-group builders as the first step of arch-02, which then resolves rev-10
5. capabilities, by leverage: mesh/glTF import → audio handles + streaming → skinning
   → input (wheel, text entry)
