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

### rev-10 — the render god-structs now overflow rustc's auto-trait check

- **location:** crates/lunar-render-3d/src/lib.rs:1472, crates/lunar-render/src/lib.rs:430
  (plus the `Resource` bound sites in lunar/src/bootstrap.rs:111,
  headless_probe, render-bench)
- **impact:** medium (ci's clippy `-D warnings` fails on this; worse on future nightlies)
- **effort:** L (the fix is arch-02); S workaround

clippy on current nightly warns "overflow evaluating the requirement
`RenderEngine3d: Sync`" and the same for `RenderEngine` and the `Resource` bounds.
the struct is large enough that auto-trait evaluation hits the recursion limit.
concrete evidence for arch-02. `#![recursion_limit = "256"]` silences it in the
meantime.

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
| detail sprites / grass | crashes on first use | rt-02 |
| water | crashes on first use | rt-03 |
| 2d parallax layers | no effect | rev-06 |
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
