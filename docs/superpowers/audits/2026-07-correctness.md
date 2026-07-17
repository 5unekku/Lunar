# correctness & robustness audit

date: 2026-07-16
method: multi-agent fan-out (one finder agent per lens), location dedup, then every
surviving finding was handed to an independent adversarial verifier whose default was
to refute it — only findings that held up under the verifier's own reading of the code
are listed below. read-only audit; no source was modified.

scope: hierarchy/ECS transform propagation and scheduling, asset loading (textures,
audio, scenes, BSP levels) against malformed/hostile/corrupted data, render resource
lifecycle across resize, input handling (keyboard/gamepad/web), physics/collision math,
plugin hot-reload, save/persist, and doc-vs-code contract mismatches.

**42 written-up findings** (6 critical, 19 high, 15 medium, 2 low). 2 duplicate pairs
were merged into a single entry each (see the "merged" note on corr-21 and corr-24) —
the counts below are post-merge.

discovery stats: 29 raw findings from 5 lenses (+4 gap follow-ups), 29 after dedup,
44 confirmed, 2 refuted.

every finding carries: id, location (file:line), claim, evidence, impact, effort
(S/M/L), confidence, and verdict CONFIRMED with a one-line summary of the verifier's
own independent reasoning. claim wording is the verifier's corrected wording where the
skeptic adjusted it (noted inline). ids are assigned in severity order and are stable
references for the phase-2 backlog synthesis, which has not seen this conversation —
anchors are kept precise and unabbreviated for that reason.

---

## critical

## corr-01 — crates/lunar-3d/src/systems.rs:317-326 (depth_of); crates/lunar-2d/src/lib.rs:139-148 (depth_of_2d)

- **impact:** critical
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

both hierarchy depth helpers recurse on `parent_idx` with no cycle guard. a mutual
`Parent` cycle between two entities (`parent_idx[a]=Some(b)`, `parent_idx[b]=Some(a)`)
causes unconditional unbounded recursion that stack-overflows and SIGABRTs the whole
process. confirmed with a live repro against the real `propagate_transforms_3d` /
`propagate_transforms` entry points in both crates (external repro crate, path deps
only, no repo files touched): both runs terminated with "stack overflow, aborting",
exit code 134.

evidence: `depths[idx]` is written only *after* the recursive call returns
(`systems.rs:324`, `lib.rs:146`), so the memoization guard
`if depths[idx] != u32::MAX { return depths[idx]; }` never fires for a node still
mid-recursion. the `+ 1` after the recursive call makes this a non-tail call, so no
optimization level turns it into a loop. `propagate_transforms_3d` is registered at
`UpdateStage::Render` every tick (`lunar-3d/src/plugin.rs:48`) and reaches `depth_of`
whenever any entity is parented (`systems.rs:137,197`); `propagate_transforms` is
registered at `UpdateStage::Update` (`lunar-2d/src/lib.rs:51`) and calls `depth_of_2d`
unconditionally every frame with **no** change-detection early-out at all. no cycle
guard exists anywhere in the hierarchy machinery (`lunar-core/src/hierarchy.rs`'s
`sync_children` only mirrors `Parent` onto `Children`, doesn't validate it).

verifier: read both functions byte-for-byte against current source, confirmed the
guard genuinely never fires mid-cycle, confirmed both call sites are reached
unconditionally in normal frame execution, and confirmed no mitigation exists anywhere
in the hierarchy code — not a re-report of any prior-work-rejected item.

## corr-02 — crates/lunar-assets/src/lib.rs:278-297 (AssetStore::allocate_slot) and :442-448 (AssetStore::remove)

- **impact:** critical
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`AssetStore`'s generational handle protection is completely inert: every slot is
permanently generation 0, so a stale `Handle<T>` captured before a texture/sound/font
is released can silently resolve to a totally unrelated asset loaded later into the
same freed slot. this directly contradicts the module doc's own claim that reload
"prevents use-after-free bugs."

evidence: `allocate_slot`'s free-slot search only ever picks an index where
`entries[id]` is `None`, so the immediately-following generation lookup
(`self.entries.get(id).and_then(|e| e.as_ref()).map_or(0u16, |e| e.generation.wrapping_add(1))`)
always sees `None` and always yields 0 — the increment branch is dead code for slot
reuse. `remove()` (lines 442-448) confirms the root cause: `slot.take()` drops the
whole `AssetEntry` including its generation, with no side table to retain it.
`is_ready`/`is_loaded`/`get` (lines 337, 346, 365) gate purely on generation equality,
so a stale handle and a fresh handle to whatever gets allocated into that same id are
both generation 0 and indistinguishable. reachable via the standard public path:
`release_texture` (line 1350) calls `store.remove(id)` on ref-count zero, then any
subsequent `load_texture()` can reuse that id. the crate's own test
(`asset_store_generation_increments_on_reuse`, line 2123) admits in a comment it
doesn't exercise the remove+reuse path.

verifier: manually traced the repro (allocate "a" → Handle(0,0); remove(0); allocate
"b" → Handle(0,0) again) and confirmed identical generations; confirmed no persistent
side table exists in the `AssetStore` struct that could retain a freed generation.

## corr-03 — crates/lunar-assets/src/lib.rs:777-825 (BctexLoader::load, esp. line 801) with Cargo.toml:132-137

- **impact:** critical
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`BctexLoader::load` slices the input buffer using a `base_size` computed entirely from
header fields with no bounds check against the actual byte length, so a truncated or
malformed `.bctex` file panics. because the workspace release profile sets
`panic = "abort"`, and this panic happens on a background `IoTaskPool` worker thread
with no `catch_unwind` anywhere in the repo, the panic aborts the entire game process.

evidence: `let base_size = width.div_ceil(4) * height.div_ceil(4) * block_bytes; let mut offset = 16usize; let pixels = bytes[offset..offset + base_size as usize].to_vec();`
(lines 799-801) — `width`/`height`/`block_bytes` all come from the file's own header,
no `if offset + base_size as usize > bytes.len() { return Err(...) }` guard exists
before the slice, contrasting with the mip loop just below (lines 806-815) which does
check bounds before slicing. runs inside `IoTaskPool`'s worker loop
(`thread::spawn` at line 524, `loader.load(bytes)` at line 530) with no
`catch_unwind` anywhere in the repo (grepped). root `Cargo.toml:130-137` sets
`[profile.release] panic = "abort"` workspace-wide.

verifier: confirmed the asymmetry against the mip loop's own bounds check just a few
lines below; confirmed `catch_unwind`/`panic::set_hook` appear zero times in the whole
repo; reachable through ordinary corruption (interrupted writes, partial downloads),
not just malicious tampering.

## corr-04 — crates/lunar-input/src/lib.rs:752-756, 966-988 (cfg(not(wasm32)) only), 1474-1561, 1305-1351, 766-784

- **impact:** critical
- **effort:** M
- **confidence:** high
- **verdict:** confirmed

on the wasm/web target, `InputState::add_gamepad` is never invoked for browser
gamepads, so `InputState.gamepads` stays permanently empty and every gamepad
button/axis event is silently swallowed — gamepad input does not work at all on web
builds, not just after a disconnect, with no log or error anywhere.

evidence: `add_gamepad` (752-756) is called from exactly three places repo-wide: line
972 inside `SdlGamepadProvider::handle_event`'s `ControllerDeviceAdded` arm, gated
`#[cfg(not(target_arch = "wasm32"))]` (line 954), plus two `#[cfg(test)]` fixtures
(1761, 1777) — neither exercises the wasm connect path. the wasm-only `poll_gamepads`
(`#[cfg(target_arch = "wasm32")]`, 1474-1561) reads `navigator.get_gamepads()` and
calls `push_gamepad_button`/`release_gamepad_button`/`push_gamepad_axis` directly,
never `add_gamepad`. `drain_to_input` (1305-1351) then applies those queued events
through setters (766-784) that each guard with
`if let Some(gamepad) = self.gamepads.get_mut(gamepad_index)` — always `None` since
the Vec's length is 0 for the whole wasm session.

verifier: traced the full chain and confirmed `add_gamepad` is genuinely never called
on any wasm code path; docs/input.md documents gamepad methods with no web-platform
caveat, so this isn't a documented limitation.

## corr-05 — crates/lunar-render-3d/src/resources.rs:1140-1158 (build_bloom_resources), called from config.rs:48-70 (resize) and init.rs:1822-1833 (construction)

- **impact:** critical
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

the bloom mip-chain texture is created with `width/2, height/2` and no minimum-1
clamp, so a window resize (or construction) that drives render width/height down to 1
produces a `wgpu::Extent3d` of 0, and since no uncaptured-error handler is registered
anywhere in either render crate, wgpu's default handler unconditionally panics the
process on that validation error.

evidence: `size: wgpu::Extent3d { width: width / 2, height: height / 2, depth_or_array_layers: 1 }`
(resources.rs:1143-1144) has no `.max(1)` — contrast with every sibling half-res
resource in the same crate, which does clamp (`resources.rs:643-644`,
`init.rs:2304-2305`, `init.rs:2445-2446`, `init.rs:3096-3097`). `config.rs:48-49`
computes `render_w`/`render_h` with `.max(1)` (so they legitimately can equal 1) and
passes them straight into `build_bloom_resources`. vendored
`wgpu-core-29.0.4/src/conv.rs:251-253` rejects any zero texture dimension; vendored
`wgpu-29.0.4/src/backend/wgpu_core.rs:685-688`'s `default_error_handler` does
`panic!("wgpu error: {err}")`; grepping the crate for `on_uncaptured_error`/
`push_error_scope` returns zero hits.

verifier: confirmed reachability end to end — `window_host.rs:115-137` forwards the
raw SDL3 window pixel size to `resize()` with no minimum-size enforcement anywhere in
the repo, so a user shrinking the window to a handful of pixels hits this directly.

## corr-06 — crates/lunar-render/src/lib.rs:1108-1116 (2D RenderEngine::resize)

- **impact:** critical
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

the 2D `RenderEngine::resize()` has no zero-dimension guard, so a window-minimize
event (0x0) reaches `surface.configure()` and, per wgpu 29's documented contract on
`Surface::configure`, panics; combined with the workspace's `panic = "abort"` release
profile this aborts the whole process. the sibling 3D engine explicitly guards
against exactly this case.

evidence: `pub fn resize(&mut self, width: u32, height: u32) { self.config.width = width; self.config.height = height; if let Some(surface) = &self.surface { surface.configure(&self.device, &self.config); } ... }`
has no `width == 0 || height == 0` check anywhere, unlike
`lunar-render-3d/src/config.rs:27-30`'s
`if width == 0 || height == 0 { return; }`. traced the actual wgpu error path (not
just the doc comment): `ConfigureSurfaceError::ZeroArea` routes through
`handle_error_or_return_handler` to `default_error_handler`'s unconditional
`panic!()` when no uncaptured-error handler or error scope is installed — grepped and
found none anywhere in lunar-render/lunar/lunar-render-3d. reachable end to end:
`crates/lunar/src/window_host.rs:114-137` reads window size unconditionally every
frame and calls the caller's resize closure on any change with no zero filter;
`bootstrap.rs:130-134` wires that into `RenderEngine::resize` on a real windowed
surface. `bootstrap` (2D) is exported unconditionally for every non-wasm build
regardless of '2d'/'3d' feature flags and is what `lunar_app!` expands to — the
officially blessed easiest way to start any native Lunar game, not a legacy path.

verifier: traced the full runtime path from `ZeroArea` error to the unconditional
`panic!()` independently rather than trusting the doc-comment citation; confirmed no
minimize/occlusion handling exists anywhere in `crates/lunar` to intercept a 0x0 size
before it reaches `resize`.

---

## high

## corr-07 — crates/lunar-2d/src/collision.rs:159-175 (shapes_overlap AABB-vs-circle), :52-63 (Collider::aabb), :32-37 (ColliderShape)

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`shapes_overlap`'s AABB-vs-circle branch calls `f32::clamp(min, max)` with min/max
derived directly from `half_extents` with no validation that it's non-negative.
`Collider::aabb(size)` and the public `ColliderShape::Aabb` variant both allow a
negative `half_extents` component to be constructed, which makes `min > max` and
causes `f32::clamp` to panic — unconditionally, not just under `debug_assertions`.

evidence: `circle_pos.x.clamp(aabb_pos.x - half_extents.x, aabb_pos.x + half_extents.x)`
(lines 166-174) with no prior validation. `Collider::aabb` (55-59) does
`half_extents: size * 0.5` with no sign check, so `Collider::aabb(Vec2::new(-10.0, 10.0))`
yields `half_extents.x = -5.0`. `ColliderShape`'s fields are also directly public
(32-37), so callers can bypass the constructor entirely. verified against the pinned
nightly toolchain that `f32::clamp` opens with an unconditional
`const_assert!(min <= max, ...)` — not gated by `debug_assertions`.

verifier: confirmed no validation, `debug_assert`, or test anywhere in the file
catches a negative-half-extent AABB before this code path; broad-phase filtering in
`CollisionWorld` only screens x-range before calling `shapes_overlap`, so the panic
fires as soon as a malformed AABB and any circle become x-range candidates.

## corr-08 — crates/lunar-2d/src/collision.rs:416-444 (ray_vs_circle), :327-361 (ray_cast_2d), :371-382 (ray_vs_aabb, contrast)

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`ray_vs_circle` has no zero-direction guard (unlike its sibling `ray_vs_aabb`), so a
zero-length ray direction yields a NaN distance/point/normal that slips past all
checks, and `ray_cast_2d`'s nearest-hit accumulator then permanently favors that NaN
hit over any real hit for the rest of the query.

evidence: for `direction = Vec2::ZERO`: `a = direction.dot(direction) = 0.0` (424),
discriminant `= 0.0` (427, not `< 0.0` so the guard at 428 doesn't fire),
`t = (-b - sqrt_d) / (2.0 * a) = 0.0/0.0 = NaN` (432), the filter
`distance < 0.0 || distance > max_dist` (438) is false for NaN either way, so the NaN
hit is returned. in `ray_cast_2d`, `nearest.as_ref().is_none_or(|n| distance < n.distance)`
(349) is true on the first (`None`) iteration regardless of value, recording the NaN
hit; every later real hit's `distance < n.distance` against a NaN `n.distance` is
false under IEEE-754, so the NaN result can never be displaced. `ray_vs_aabb`
(371-382), by contrast, guards each axis's inverse-direction with an
`f32::EPSILON` check.

verifier: hand-traced the IEEE-754 arithmetic to confirm the exact NaN propagation;
confirmed `lunar_math::Vec2` is a plain glam alias using native f32 semantics; no test
exercises a zero-length direction.

## corr-09 — crates/lunar-3d/src/light.rs:148-162, bundles.rs:155-161, scene_format_3d.rs:89,246-260,478-488, crates/lunar-render-3d/src/lib.rs:1388-1430,1435-1436, frame.rs:339-386

- **impact:** high
- **effort:** L
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* `SpotLight` is a fully-wired authoring/ECS component
(component in `light.rs`, spawn bundle in `bundles.rs`, `.ls3` scene-file field with
inner/outer cone angles in `scene_format_3d.rs`) and is publicly documented in
`docs/3d/lighting.md` as a working light type on par with directional and point
lights (including shadow-casting), but `lunar-render-3d` never queries or shades it:
`FrameQueries` (lib.rs:1388-1430, constructed at 1435-1436) has query fields only for
`dir_lights` and `point_lights`, `SpotLight` isn't even imported into lib.rs, and
`frame.rs`'s light-gathering block (~333-386) only reads `DirectionalLight` and
`PointLight`. no shader file (surface.wgsl, cluster.wgsl, shadow.wgsl,
point_shadow.wgsl, etc.) contains any cone/spot-angle logic either. any spot light
placed in a scene, via code or via a `.ls3` file, contributes zero illumination and
silently ignores its documented shadow-casting option.

evidence: repo-wide grep for `SpotLight`/`spot`/`cone`/`inner_angle`/`outer_angle`
across all of lunar-render-3d (`.rs` and `.wgsl`) returns zero matches outside this
audit's own citations.

verifier: confirmed every cited location verbatim; found the bonus corroboration that
`docs/3d/lighting.md` has a full "spot light" section presenting it as equally
first-class, with no caveat, understating rather than overstating real-world impact.

## corr-10 — crates/lunar-assets/src/lib.rs:861-871 (resolve_asset_path)

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`resolve_asset_path` performs no sanitization of `..` path components before
prefixing with `"assets/"`, so any texture/sound/font path containing traversal
sequences (e.g. from a scene file's `sprite_texture` field, or any other data-driven
path string) resolves outside the assets directory and is read from arbitrary
locations on disk via `std::fs::read`.

evidence: `let cleaned = path.strip_prefix("./").unwrap_or(path); if Path::new(cleaned).is_absolute() { return cleaned.to_string(); } if cleaned.starts_with("assets/") || cleaned.starts_with('/') { return cleaned.to_string(); } format!("assets/{cleaned}")`
(861-871). for input `"../../secret.png"`, none of the three early-return branches
trigger, so the function returns `"assets/../../secret.png"`, which the filesystem
resolves two directories above the assets root. this is the sole path-resolution
point used by `load_texture`/`load_sound`/`load_font` (1141, 1174, 1186) before
`std::fs::read(&path)` (line 528) runs unchanged on an `IoTaskPool` worker thread.

verifier: confirmed `EntityDefinition.sprite_texture` in `lunar-core/src/scene_format.rs`
is a plain `String` field parsed straight from author-facing RON scene files with zero
validation — a concrete, not hypothetical, data-driven trigger.

## corr-11 — crates/lunar-audio/Cargo.toml:19-21; decoder.rs:134-136; plugin.rs:39-44

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

every `AudioFormat::Wav` sound (any `.wav`/`.wave` asset) fails to decode at runtime
and is silently dropped, because lunar-audio never enables symphonia's `"pcm"`
feature (the crate that actually decodes WAVE_FORMAT_PCM/IEEE_FLOAT samples) — only
the `"wav"` demux-only feature is enabled.

evidence: `symphonia = { version = "0.5", default-features = false, features = ["ogg", "vorbis", "wav", "flac"] }`
(Cargo.toml:19-21), no `"pcm"`. vendored `symphonia-format-riff`'s `wav` feature
purely gates the demuxer module, zero decoder dependency; symphonia's facade default
feature set includes `"pcm"` alongside `"wav"`, but lunar-audio opts out of defaults
and hand-picks features, omitting it — confirmed via `Cargo.lock`, no
`symphonia-codec-pcm` anywhere in the resolved graph. `CodecRegistry::make()` falls to
`unsupported_error("core (codec):unsupported codec")` since no PCM descriptor is ever
registered. `decoder.rs:134-136` wraps that as an error string, and
`plugin.rs:39-44`'s `AudioPlayer::play` just logs and drops the sound — no propagation,
retry, or fallback.

verifier: checked that WAVE_FORMAT_PCM/IEEE_FLOAT (the two encodings covering the
overwhelming majority of real `.wav` files) map exclusively to `PcmDecoder`; confirmed
lunar-audio is the sole symphonia dependent in the workspace so there's no
feature-unification path supplying `"pcm"` indirectly; no existing test exercises the
real `decode()` path against actual encoded WAV bytes.

## corr-12 — crates/lunar-audio/Cargo.toml:19-21; decoder.rs:108-113 (OggOpus hint) and :134-136

- **impact:** high
- **effort:** M
- **confidence:** high
- **verdict:** confirmed

`AudioFormat::OggOpus` (`.opus` assets) can never be decoded: the pinned symphonia
0.5.5 has no Opus decoder crate/feature at all, even though its Ogg demuxer
unconditionally recognizes and demuxes Opus streams — so playback fails identically
to the WAV case, silently, and no Cargo.toml change to this pinned version could fix
it (a dependency bump would be required).

evidence: symphonia 0.5.5's full `[features]` block has no `"opus"` feature at all,
under any combination including `all-codecs`. `symphonia-format-ogg`'s
`mappings/opus.rs` is unconditionally compiled and matches the `OpusHead` magic
signature, registering a track tagged `CODEC_TYPE_OPUS` — so the demux/probe step
genuinely succeeds. `decoder.rs:108-113` explicitly routes
`AudioFormat::OggOpus => hint.with_extension("ogg")`, confirming this format is
intentionally supported code, not dead. `CodecRegistry::make()` then has no
descriptor for `CODEC_TYPE_OPUS` and fails, silently swallowed by
`plugin.rs:39-44` same as the WAV case. `lunar-assets/src/lib.rs:898` maps
`"opus" => AudioFormat::OggOpus`, confirming reachability from real asset extensions.

verifier: confirmed via `Cargo.lock` no `symphonia-codec-opus`-equivalent exists in
the resolved graph; confirmed no custom `CodecRegistry` construction exists anywhere
in lunar-audio that could work around this.

## corr-13 — crates/lunar-bsp/src/level.rs:112-137 (BspLevel::camera_leaf), reached every frame from crates/lunar-render-3d/src/cull.rs:414-418

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`camera_leaf()` walks the deserialized BSP node tree indexing `blob.nodes[node_idx]`
and following `left_or_start`/`right_or_end` as raw node indices with zero bounds
validation after `bincode::deserialize`. any blob whose indices don't correspond
exactly to its own node count (stale/mismatched-version compile output, a
partially-regenerated level after a compiler change) causes an index-out-of-bounds
panic on the first frame after load, and every subsequent frame since
`gather_draw_list` calls it unconditionally once a `BspLevel` is loaded.

evidence: `let mut node_idx = 0usize; loop { let node = &blob.nodes[node_idx]; if node.left_or_start < 0 { return node.leaf_index as usize; } ... node_idx = if coord >= node.split_value { node.right_or_end as usize } else { node.left_or_start as usize }; }`
— no `.get()`/bounds check, unlike the sibling method `for_each_visible_leaf` two
methods below, which explicitly uses `blob.pvs.get(base + word_idx)` and breaks on
`None`. `bincode::deserialize` only validates structural/type layout, not that
`left_or_start`/`right_or_end` are valid indices into `blob.nodes`. `cull.rs:414-418`
calls this unconditionally inside `gather_draw_list`, the per-frame draw-list
builder, whenever a `BspLevel` resource with `blob.is_some()` exists.

verifier: confirmed the level.rs module doc's own promise ("returns an error string
if deserialization fails — corrupt or wrong-version blob") is false for
semantically-invalid-but-structurally-parseable data, which is exactly the gap this
finding exploits; `level.rs`'s doc explicitly documents a runtime asset-server load
path (not just build-time embedding), making a version-mismatched blob realistic.

## corr-14 — crates/lunar-core/src/persist.rs:73-84 (persist::save)

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`persist::save()` writes new save data directly to the destination path via
`std::fs::write`, which truncates the existing file before writing new bytes; if the
write fails partway (disk full, process killed, I/O error), the previous good save is
already destroyed before the caller ever learns of the `Err`, with no atomic
temp-file+rename to protect the prior save.

evidence: `std::fs::write(path, content.as_bytes())?;` (line 82) — equivalent to
open-with-create+truncate+write, then `write_all`; the truncate happens before any new
bytes land. `persist::save` is a public API re-exported from `lunar-core`'s prelude
with zero internal callers, framed by the module's own doc as the game's actual
save/load mechanism. none of the four unit tests (117-163) exercise a mid-write
failure.

verifier: confirmed no tempfile/rename/atomic-write pattern exists anywhere else in
the repo that this could be inconsistent with — it's a genuine gap in the only save
path the engine offers, not a stray oversight contradicted elsewhere.

## corr-15 — crates/lunar-core/src/scene_format.rs:442-455 (2D loader); crates/lunar-3d/src/scene_format_3d.rs:501-510 (3D loader)

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

the RON scene loaders for both 2D and 3D resolve each entity's textual `parent` id
into a live `Parent` component via a plain id-map lookup with zero cycle validation,
so a hand-edited or corrupted scene file with two entities naming each other as
parent creates the mutual-`Parent` cycle from corr-01 purely from data at scene-load
time — no runtime reparenting bug, editor drag-and-drop, or script is required to
reach the crash.

evidence: both loaders build `id_map` fully in a first pass, then in a second pass do
`if let Some(&parent_entity) = id_map.get(&parent_id) { commands.entity(entity).insert(Parent(parent_entity)); ... } else { log::warn!(...); }`
with no ancestry/cycle check. a RON file with `{id: "a", parent: "b"}` /
`{id: "b", parent: "a"}` inserts a genuine mutual cycle. the only other
`Parent`-writing code (`lunar-core/src/hierarchy.rs`'s `sync_children`) likewise
performs no cycle check — it only mirrors `Parent` onto the parent's `Children` list.

verifier: confirmed no cycle-detection code exists anywhere in the repo (grepped for
"cycle"/"cyclic", zero hits); confirmed `lunar-gamedata-build`'s RON→binary converter
does no parent/graph validation either, so even the baked binary scene format carries
the cycle through untouched; this is a genuine escalation of corr-01's reachability
via untrusted data files, not a restatement of it.

## corr-16 — crates/lunar-input/src/lib.rs:211-217, 237-239, 248-250, 262-264

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* `GamepadAxis` binding thresholds: the
`ActionBuilder::axis` method doc (211-213) promises directional sign semantics
("0.5" for one direction, "-0.5" for the opposite), but `InputBinding::axis_active`
(237-239) compares `value.abs() >= threshold.abs()`, discarding sign entirely, so a
negative-threshold binding fires identically to its positive-magnitude counterpart.
concretely, binding `move_left` to `GamepadAxis(0, LeftStickX, -0.5)` and
`move_right` to `GamepadAxis(0, LeftStickX, 0.5)` makes both fire when the stick is
pushed fully in either direction.

evidence: git history confirms this is a real regression, not a documentation nit —
`axis_active` was originally signed
(`if threshold >= 0.0 { value > threshold } else { value < threshold }`) when the
`axis()` doc was written (commit b58411b), then commit 6fd6ad5 changed it to
abs-only magnitude comparison the same day without updating the doc or the test.
note: `InputBinding::GamepadAxis`'s own enum-variant doc and `docs/input.md` already
describe (and agree with) the current abs/deadzone behavior — the drift is localized
specifically to `ActionBuilder::axis`'s rustdoc, which is the doc callers actually
read when choosing a threshold sign.

verifier: confirmed via `git blame` this is a traceable regression from a same-day
follow-up commit that touched the implementation but not the doc or test; the cited
test (`action_map_gamepad_axis`) only ever supplies a positive threshold, never
catching the mismatch.

## corr-17 — crates/lunar-input/src/lib.rs:759-763, 951, 968-988, 989-1012

- **impact:** high
- **effort:** M
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* disconnecting any gamepad other than the
currently-highest-indexed one desyncs the cached engine index for every
still-connected gamepad whose original index was higher than the removed one (not
literally every connected pad — lower-indexed pads are unaffected).
`InputState::remove_gamepad` (759-763) uses `Vec::remove`, shifting down all later
elements, but `SdlGamepadProvider::open_gamepads` (951) caches each pad's engine
index once at `ControllerDeviceAdded` (968-981) and `ControllerDeviceRemoved`
(983-988) never refreshes the cached indices of the other entries. subsequent
`ControllerButtonDown`/`Up`/`AxisMotion` events (989-1012) for those higher-indexed
pads then use the stale index, which either falls out of range (silently dropped) or,
if a new pad connects afterward and reclaims that slot number, gets silently
misrouted onto the new pad.

evidence: with pads A(0), B(1), C(2), disconnecting B causes `gamepads.remove(1)` →
C is now physically at slot 1, but `open_gamepads` still maps C's SDL id to engine
index 2. C's future button events call `press_gamepad_button(2, ...)`, which either
no-ops or, if a 4th pad connects, gets rerouted onto that new pad's slot.

verifier: confirmed both failure modes (dropped, misrouted) are reachable; confirmed
no compensating reindexing pass exists anywhere in the crate.

## corr-18 — crates/lunar-plugin-loader/src/lib.rs:136-171 (reload_nativeaot), analogous at :176-207 (reload_coreclr)

- **impact:** high
- **effort:** M
- **confidence:** high
- **verdict:** confirmed

`reload_nativeaot` unconditionally clears the Update/FixedUpdate/Shutdown schedules
and sets the world's `is_reload` flag to true before any of the three fallible steps
that follow (temp-file copy, dlopen, symbol lookup); if any of those fail, the
function returns `Err` with the schedules permanently empty and `is_reload` stuck
true, leaving the ECS world in a broken half-reloaded state that the caller only logs
and otherwise ignores.

evidence: `clear_schedule` x3 (146-148) run unconditionally, then
`versioned_plugin_copy(path, version)?` (150), `set_is_reload(world, true)` (151),
`Library::new(&versioned)` (155), `lib.get(...)` (158) — three `?` early-returns all
occur after the schedules are already wiped, and the last two after `is_reload` is
set true. `set_is_reload(world, false)` (168) is never reached on any of those error
paths. the sole caller, `dispatch_ffi_update_hot` (402-425), just logs the error and
continues the frame loop on the same broken registry every subsequent frame — no
rollback or re-registration. `reload_coreclr` (176-207) has the identical ordering.

verifier: confirmed `dispatch_systems` simply iterates whatever's currently in the
schedule's HashMap, so a cleared schedule stays permanently empty across all future
frames with no lazy repopulation; no test in the crate exercises this failure path.

## corr-19 — crates/lunar-render-3d/src/cull.rs:195-199,529-540, lod_select.wgsl:1-19

- **impact:** high
- **effort:** M
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* the GPU LOD-selection compute path (high tier, gated
by `gpu_cull_enabled = render_tier == RenderTier::High`, init.rs:4092) buckets every
entity's LOD level from four hardcoded global squared-distance thresholds
(cull.rs:195-199: 225/2500/22500/160000) instead of that entity's own
`MeshLod.levels` thresholds (`lunar-3d/src/mesh.rs:371-395`), so authored per-entity
LOD switch distances are ignored whenever the high tier is active. worse than just
wrong switch distances: `gather_draw_list` (cull.rs:529-540) reuses the resulting
global-bucket index directly as `l.levels.get((gpu_lod-1) as usize)`, and when an
entity's `levels` vec is shorter than that index, `.get()` returns `None` and the code
falls back to the *base* (finest) mesh via `unwrap_or(mesh.0)` — the opposite of
`MeshLod::select`'s documented CPU-path fallback to the *coarsest* level when
thresholds are exceeded. so a distant entity with a short custom LOD chain can render
at full detail under the high tier while rendering correctly-coarsened under lower
tiers, undermining both visual consistency and the performance goal of the GPU LOD
feature.

evidence: `lod_select.wgsl`'s only per-entity input is AABB center/half-extent — no
per-entity threshold buffer exists anywhere in the bind group layout
(`resources.rs:929-996`, exactly 3 bindings). commit f8a17b9's own message says the
gather "uses it instead of CPU `MeshLod::select` when available" — i.e. it was
intended to be behaviorally equivalent, not a deliberate approximation.

verifier: derived the mismatch direction independently — GPU path falls back to the
*finest* mesh on out-of-range index, CPU path falls back to the *coarsest* — making
the bug worse than "wrong switch distance" alone.

## corr-20 — crates/lunar-render-3d/src/lib.rs:2228-2231 (RenderPlugin3d::build); reads at cull.rs:31,33 and frame.rs:1827

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* `render_3d_system` is only ordered
`.after(propagate_transforms_3d)`, yet `cull_entities` (cull.rs:31,33) and
`frame.rs:1827` read the `Frustum` and `CullSoa` resources every frame with no
`.after(update_frustum)` / `.after(build_cull_soa)` edge against their writers
(registered at `lunar-3d/src/plugin.rs:51-52`). in bevy_ecs 0.18.1's `MultiThreaded`
executor (used by lunar-core's Render stage on any >2-core machine), unordered
systems get no implicit sequencing. today's correct behavior is an accident of
registration order (`Plugin3d` is added before `RenderPlugin3d` in
`bootstrap_3d.rs:136-137`), not an enforced guarantee; this is the same hazard class
already fixed for `propagate_transforms_3d` (lib.rs:2223-2227, following a real,
observed one-frame flicker bug) but left open for its sibling Frustum/CullSoa
dependencies.

evidence: verified in bevy_ecs's own source (`executor/multi_threaded.rs`) that
exclusive systems with ambiguities are documented by the framework itself as
susceptible to being displaced from topological order; `can_run` blocks purely on a
running-systems counter, not on topological position.

verifier: independently confirmed the ECS access conflict (`update_frustum` takes
`ResMut<Frustum>` vs `build_bvh_visible`'s read, `propagate_transforms_3d` is an
exclusive `&mut World` system) is real and live, not hypothetical.

## corr-21 — crates/lunar-render-3d/src/lib.rs:958-967 (doc) vs 1019-1050 (impl)

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed
- **merged:** corr-41 (identical finding surfaced by a second lens, rated medium there — kept at this entry's "high" rating and evidence set)

`DevRenderProfile`'s struct-level rustdoc claims `default()`/no-resource-inserted
means "all on" / "every feature the hardware supports is available to the user", but
`Default` actually returns `classic()`, which explicitly disables shadows, bloom,
ssao, ssr, volumetric_fog, point_light_shadows, soft_shadows, and contact_shadows.

evidence: doc (960, 966-967): "a developer building a photorealistic game uses
`DevRenderProfile::default()` (all on)" / "if not inserted, `default()` is used;
every feature the hardware supports is available to the user." impl (1019-1025):
`fn default() -> Self { Self::classic() }`, doc'd inline as "the cheapest possible
starting point." `classic()` (1033-1055) sets every listed feature to false. the
"all on" profile the struct doc is actually describing is `full()` (1084-1109), not
`default()`. any game that skips inserting a `DevRenderProfile` resource silently
gets the minimal profile instead of the documented full-featured one.

verifier: found fresh corroboration beyond the original citation — `frame.rs:138-143`
has a comment by the same author explicitly warning against the exact misreading the
struct doc invites ("NOT turn every effect on... would otherwise enable
SSAO/SSR/bloom with no setup and crush lit geometry to black"), and no
`log::warn!` fires when the resource is absent, so a developer has no way to discover
the discrepancy short of reading source. git blame shows a later commit touched both
the struct doc and the `Default` impl doc in the same commit and still left the
contradiction unresolved.

## corr-22 — crates/lunar-render-3d/src/mesh.rs:180-187 and 279-283 (forsyth_optimize), called from mesh.rs:95 (upload_mesh_data)

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`forsyth_optimize`'s triangle-score computation indexes `vert_score[vi as usize]`
directly from raw index-buffer values without the bounds check the function already
applies a few lines earlier when building `vert_tris`; any `MeshData` whose index
buffer references a vertex index `>= vertex_count` (a mismatched/stale index buffer
from a hand-built or imported mesh — no validation exists anywhere upstream) panics
with an index-out-of-bounds the first time that mesh is uploaded.

evidence: `vert_tris` build (149-155) guards with
`if (vi as usize) < vertex_count { ... }`, but the very next block (180-187) has no
such guard: `(0..tri_count).map(|ti| indices[ti*3..ti*3+3].iter().map(|&vi| vert_score[vi as usize]).sum())`.
the same unguarded pattern repeats at 279-283. `mesh.rs:95` confirms this runs for
every real mesh via `upload_mesh_data`. `MeshData::new` (`lunar-3d/src/mesh.rs:152`)
performs no index-bounds validation, and `MeshRegistry::add_mesh` is an intended
public entry point for game code with no validation either.

verifier: confirmed release's `panic = "abort"` makes this a full process abort, not
a recoverable unwind; confirmed no test in the file covers `forsyth_optimize` with
out-of-range indices.

## corr-23 — crates/lunar-render-3d/src/mesh.rs:330-369 (upload_heightmap) and :386-391 (build_terrain_gpu); crates/lunar-3d/src/terrain.rs:14-33 (Terrain)

- **impact:** high
- **effort:** M
- **confidence:** medium
- **verdict:** confirmed

*(verifier-corrected wording)* `Terrain` exposes independently-public
`heightmap: Vec<u8>`, `heightmap_width: u32`, `heightmap_height: u32` fields with no
constructor or validation tying them together (only a `Default` impl exists);
`build_terrain_gpu` passes the raw heightmap bytes straight to `upload_heightmap`,
which calls `queue.write_texture` sized by `width*height` as an R16Float (2
bytes/sample) texture. any terrain whose heightmap byte length is **less than**
`width * height * 2` (e.g. a procedural/streaming terrain system that bumps
width/height and marks dirty before reallocating/refilling the byte buffer to match)
causes wgpu-core's `validate_linear_texture_data` to return
`Err(BufferEndOffsetOverrun)`. since Lunar registers no error scope and no
`on_uncaptured_error` handler anywhere in the repo, this reaches wgpu's
`default_error_handler`, which unconditionally panics the process. (an oversized
heightmap buffer, by contrast, does not trigger this — only undersized data does.)

evidence: only `w`/`h` are `.max(1)`-clamped (mesh.rs:386-389); `terrain.heightmap` is
passed through with no length check; `queue.write_texture` fires whenever `data` is
non-empty. `Terrain` is publicly re-exported from lunar-3d (`lib.rs:88`) and is not
part of the scene-format serde path, so no engine-side gate prevents a consumer from
constructing a mismatched `Terrain`.

verifier: traced the reachable calling context — `passes.rs:606-647` invokes
`build_terrain_gpu` whenever `terrain.dirty` is true or the entity is new, the
intended rebuild path any streaming/procedural terrain system would use.

## corr-24 — crates/lunar-render-3d/src/resources.rs:645-646, config.rs:89, post.rs:373-379

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed
- **merged:** corr-42 (identical finding surfaced by a second lens, rated medium there — kept at this entry's "high" rating and evidence set)

`contact_shadow_tex` is created once and gated on `is_none()`, but the window-resize
path only invalidates `contact_shadow_bg`, never `contact_shadow_tex` — so the
contact-shadow render target stays pinned at the window's original resolution for the
rest of the session even as every other post-processing resource resizes correctly,
producing a permanent visual mismatch (stretched/misaligned contact shadows) after
any window resize with contact shadows enabled.

evidence: `let needs_tex = self.contact_shadow_tex.is_none(); if needs_tex { ... }`
(resources.rs:645-646) gates (re)creation solely on the Option being empty.
`config.rs`'s resize routine rebuilds hdr_texture, bloom mips, and
gtao_depth_texture/view, and at line 89 does only
`self.contact_shadow_bg = None; // the contact-shadow pass bind group references gtao_depth_view: invalidate it`
— `contact_shadow_tex` itself is never referenced anywhere in `config.rs`. `post.rs`
calls `ensure_contact_shadow_resources(width, height, ...)` every frame with the live
surface size, but sees a still-`Some` `contact_shadow_tex` and skips recreation.

verifier: additionally confirmed `config.rs:342-345` rebuilds `composite_bg` on every
resize and reuses `self.contact_shadow_view` unconditionally — actively wiring the
stale, original-resolution view back into the live composite bind group after every
resize, not merely leaving it dangling; confirmed `contact_shadows: true` is a real
default in `DevRenderProfile::full()`, a legitimate production configuration.

## corr-25 — crates/lunar-render/src/lib.rs:1452-1456

- **impact:** high
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

the 2D `RenderEngine::render()` never reconfigures the surface when acquisition
reports `Outdated` or `Lost`; it just returns, so once the surface enters that state
it stays broken (every future frame silently skipped) until an unrelated real
window-size change happens to trigger `resize()`. the 3D engine explicitly
reconfigures in this branch.

evidence: `match surface.get_current_texture() { Success(f) | Suboptimal(f) => Some(f), _ => return, }`
— `Outdated`, `Lost`, `Timeout`, `Occluded`, and `Validation` are all folded into the
same `_ => return` arm with no call to `surface.configure`. compare
`lunar-render-3d/src/frame.rs:1589-1602`'s equivalent path:
`Outdated | Lost => { surface.configure(&self.device, &self.surface_config); return 0; }`.
the only other `.configure()` calls in the 2D file are inside `resize()` and initial
setup, both gated on a literal pixel-dimension change.

verifier: traced the caller chain confirming `resize()`/`configure()` is unreachable
unless SDL3's reported window size literally differs from the last frame's cached
size — an `Outdated`/`Lost` status without such a delta (e.g. compositor-driven
present-mode change, some alt-tab/minimize-restore cycles) has no recovery path at
all in the 2D renderer; confirmed this is live code driving the actual native/wasm 2D
game entry points, not a stub.

---

## medium

## corr-26 — crates/lunar-3d/src/systems.rs:64-84

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`propagate_transforms_3d`'s change-detection early-out does not detect an entity
losing its `Parent` component (un-parenting); the entity's `WorldTransform3d` then
stays stale — still composed with the old parent's matrix — for as long as nothing
else in the scene changes.

evidence: the early-out compares a relevant-entity count
(`Or<(With<LocalTransform3d>, With<Visibility>)>`) against `any_changed`
(`Or<(Changed<LocalTransform3d>, Changed<Visibility>, Changed<Parent>)>`); if both
hold, the function returns without recomputing. removing `Parent` neither changes
that count (the entity still has `LocalTransform3d`/`Visibility`) nor is observable
via `Changed<Parent>` (bevy_ecs's `Changed<T>` fetch requires the entity to still
have `T`). no `RemovedComponents<Parent>` check exists anywhere in the crate.

verifier: confirmed `hierarchy.rs`'s `Parent` has no `on_remove` component hook and
`sync_children` only reacts to `Added<Parent>`, never removal; the function's own doc
comment claims the count delta "covers despawns and component removals, which
`Changed` can't observe" — true for despawns, false for this specific case,
corroborating this is an unintentional gap.

## corr-27 — crates/lunar-assets/src/lib.rs:1956-1981 (AssetWatcher::new)

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`AssetWatcher::new` silently swallows both the `notify::recommended_watcher`
construction failure and the subsequent directory-watch failure via `.ok()`/
`let _ =`, with zero logging; hot-reload is silently disabled for the whole run with
no diagnostic trace that anything went wrong, while `AssetWatcherPlugin` still logs
"asset watcher registered" regardless.

evidence: `let mut watcher = notify::recommended_watcher(...).ok(); if let Some(ref mut w) = watcher { let _ = w.watch(Path::new(watch_dir), RecursiveMode::Recursive); }`
— neither failure path (constructor `Err`, e.g. exhausted inotify instances; or
`watch()` failing, e.g. missing directory) logs anything.
`AssetWatcherPlugin::build` (2054-2059) calls
`log::info!("AssetWatcherPlugin: asset watcher registered")` unconditionally,
regardless of success.

verifier: found a third silent-failure path not in the original evidence — the
per-event callback closure (1962-1971) also discards `Err(notify::Error)` per-event
with no logging, reinforcing the pattern; confirmed `log` is actively used elsewhere
in this exact crate (plugin.rs), so the omission here is a real inconsistency, not
"no logging anywhere in the crate."

## corr-28 — crates/lunar-assets/src/lib.rs:2016-2058 (AssetChanged / dispatch_asset_changes / AssetWatcherPlugin::build)

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* the `AssetChanged` message double-buffer is never
rotated: nothing in the engine ever calls bevy_ecs's
`Messages::<AssetChanged>::update()` (directly or via `message_update_system`/
`MessageRegistry::run_updates`), so `Messages<AssetChanged>.messages_b` grows
without ever being swapped/cleared for as long as a process runs
`AssetWatcherPlugin` and the file watcher reports changes. confirmed at
`lib.rs:2056` (`MessageRegistry::register_message::<AssetChanged>`) and `:2057`
(`app.add_system(dispatch_asset_changes)` — the writer only); a repo-wide grep for
`message_update_system` / `signal_message_update_system` /
`MessageRegistry::run_updates` / `add_message` turns up nothing outside that one
`register_message` call, and Lunar's `App`/`Engine` is a hand-rolled wrapper directly
over bevy_ecs `World`+`Schedule` with no `bevy_app` dependency and no
message-rotation system of its own.

evidence: bevy_ecs 0.18.1's own doc for `Messages<T>` states "the buffers ... will
grow indefinitely if `update()` is never called"; `MessageRegistry::run_updates` is
the only thing that calls it, and `message_update_system` is its only caller — never
scheduled here.

verifier: note the finding's original claim that this "bypasses bevy's higher-level
`App::add_message`" is inaccurate — `add_message` is a `bevy_app::App` method and this
workspace depends only on `bevy_ecs` (no `bevy_app` anywhere in Cargo.toml), so there
is no such method being bypassed; Lunar simply never implemented message-buffer
rotation in its own plugin system. the underlying growth bug is otherwise real. impact
is bounded by actual file-change-event volume, not literal unconditional per-frame
growth, but is genuinely unbounded/never-reclaimed over a long dev session — exactly
this plugin's intended use case.

## corr-29 — crates/lunar-assets/src/lib.rs:300-311 (AssetStore::insert / mark_failed) vs :452-456 (LoadResult) and :1525-1570 (AssetServer::update)

- **impact:** medium
- **effort:** M
- **confidence:** medium
- **verdict:** confirmed

*(verifier-corrected wording)* async load results are matched back to store entries
by raw slot id only (`LoadResult<T>` carries no generation field), and
`AssetStore::insert`/`mark_failed` write unconditionally into whatever entry
currently occupies that slot, with no generation check anywhere in
`AssetServer::update`. this is concretely exploitable today for textures:
`AssetServer::release_texture` can free a slot via `AssetStore::remove` while that
slot's async load is still `Loading` (nothing prevents release before completion),
`allocate_slot`'s free-slot search can immediately hand that slot to an unrelated new
`load_texture` call, and neither the native thread-pool `IoTaskPool` nor the WASM
fetch-based `IoTaskPool` guarantees load completion order — so the stale original
result can arrive after the new load's result and silently overwrite the new
occupant's pixel data, or mark it `Failed`, undetected.

evidence: `struct LoadResult<T: Asset> { id: u32, path: String, data: Result<T, String> }`
has no generation field; `insert`/`mark_failed` both index `self.entries[id as usize]`
directly with no generation parameter. `AssetServer::update()` drains task-pool
results and calls `self.texture_store.insert(result.id, data)` using only
`result.id`.

verifier: sound/font stores share the same flawed `insert`/`mark_failed` logic but
currently have no public release path, so the live trigger surface today is textures
specifically; confirmed `release_texture` has exactly one call site (its own
definition) — unused internally today but live public API surface.

## corr-30 — crates/lunar-audio/src/backend/native.rs:17-27, esp. line 25

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

the SDL3 audio callback discards any error from writing mixed PCM to the output
stream with `.ok()` and no log call whatsoever (not even the silent `let _ =` idiom
used elsewhere in this same file, which at least carries an explanatory comment) — a
persistent device-write failure on the dedicated OS audio thread would silently drop
all mixed audio forever with no diagnostic trail anywhere in the crate.

evidence: `stream.put_data_f32(&self.scratch).ok();` (line 25), contrasted with
`submit()`'s `let _ = self.sender.send(source); // ignore send errors, stream may have closed during shutdown`
(75-78), which at least documents the discard. `put_data_f32` returns a real
`Result` per the vendored sdl3 crate, backed by `SDL_PutAudioStreamData`, which SDL's
own docs note can fail (internal allocation failure, or a stream in an invalid/
disconnected state, e.g. hot-unplugged output device).

verifier: confirmed `log` is a listed and actively-used dependency in this exact
crate (plugin.rs uses `log::error!`/`log::info!`), making the omission a real
inconsistency rather than a crate-wide absence of logging; confirmed `pump()` never
overrides the no-op default, so there's no ECS-thread hook that could ever surface a
flag even if one existed.

## corr-31 — crates/lunar-audio/src/decoder.rs:172-179 (downmix match); cross-referenced with vendored symphonia-format-riff chunks.rs and common.rs

- **impact:** medium
- **effort:** S
- **confidence:** medium
- **verdict:** confirmed

the channel-downmix arm `n => raw.chunks(n).flat_map(|frame| [frame[0], frame[1]]).collect()`
has no guard for `n==0` (panics inside `chunks()`) nor for a trailing chunk shorter
than 2 samples (`frame[1]` indexes out of bounds) — currently dormant given today's
working decoders (FLAC's channel field is structurally 1-8; the ogg-vorbis
ident-header parser explicitly rejects `n_channels==0`; and WAV decode never reaches
this code today per corr-11's Cargo-feature gap), but it is a live landmine, not a
false alarm: a crafted WAVE_FORMAT_EXTENSIBLE header with both `nChannels` and
`dwChannelMask` set to 0 sails through symphonia-format-riff's `fix_channel_mask`
(channel_diff == 0, neither branch fires) and `Channels::from_bits(0)` (always
succeeds as `Channels::empty()`), unlike every other channel-count path in wav/flac/
vorbis which does reject 0 via `try_channel_count_to_mask`.

evidence: the moment corr-11's Cargo-feature gap is fixed (adding `"pcm"` so WAV
actually decodes), a WAV file with this crafted header would make
`channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2)` evaluate to
`Some(0)` (not `None`, so the fallback never applies), driving `raw.chunks(0)` and
panicking synchronously on whatever thread calls `AudioPlayer::play` — fatal under
this workspace's `panic=abort` release profile.

verifier: traced the crafted-header path through the actually-vendored symphonia
sources line by line to confirm `Channels::empty()` (count 0) is genuinely reachable,
not asserted on faith; confirmed FLAC and Vorbis's own 0-channel rejections by
reading their parsers directly.

## corr-32 — crates/lunar-bsp/src/bvh.rs:186-209 (build_bvh_visible / BvhPlugin::build)

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* `build_bvh_visible` reads `Res<Frustum>` and
`Query<&WorldTransform3d>` and is registered into `UpdateStage::Render` with no
`.after()`/`.chain()` constraint, even though the adjacent comment ("run after
transform propagation so `WorldTransform3d` is current") asserts that ordering is
guaranteed. the dependency is real and conflicting at the ECS level: `update_frustum`
writes `ResMut<Frustum>` and `propagate_transforms_3d` is an exclusive `&mut World`
system that conflicts with everything — both run in the same multithreaded schedule
as `build_bvh_visible`, with relative order controlled only by plugin registration
order (`BvhPlugin` declares no dependency on `Plugin3d`). this is not speculative:
`render_3d_system` already hit this exact bug class and was fixed with an explicit
`.after(propagate_transforms_3d)` edge after it caused a real, observed glitch
("flashing freshly-spawned overlay quads at screen center for one frame") — and
`build_bvh_visible` never received the analogous fix despite depending on two
upstream systems instead of one.

evidence: correction to the original submission — `Bvh`/`BvhVisible` are **not**
exported through `lunar::prelude`; `lunar_bsp::prelude` re-exports only `BvhPlugin`
(and `Area`/`CameraArea`/`Portal*`/`VisibleAreas`); the resources are reachable only
via the crate-qualified path `lunar::lunar_bsp::{Bvh, BvhVisible}`. also note for
impact calibration: `BvhPlugin`/`BspPlugin` is not currently wired into
`bootstrap_3d.rs`, `bootstrap_wasm_3d.rs`, or any example/tool, so the hazard is
presently dormant/opt-in rather than active in the shipped game loop.

verifier: found the render_3d_system precedent as stronger corroboration than the
original submission cited; corrected the prelude-export claim after checking
`lunar_bsp::lib.rs:45-50` directly.

## corr-33 — crates/lunar-bsp/src/portal.rs:100-109, 206-214 (cull_portals / PortalPlugin::build)

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

*(verifier-corrected wording)* `cull_portals` reads `&WorldTransform3d` and
`Res<lunar_3d::Frustum>`, and `PortalPlugin::build` registers it into
`UpdateStage::Render` with no `.after()`/`.before()`/`.chain()` edge against
`propagate_transforms_3d` or `update_frustum` — both registered in the same
multithreaded schedule. the exact same hazard shape was already found and fixed
elsewhere in this codebase (`render_3d_system.after(propagate_transforms_3d)`), and
`cull_portals` has no equivalent edge. its output `VisibleAreas` is genuinely
consumed: read at `cull.rs:443` into `portal_visible_scratch`, which gates the
per-entity draw-list filter at `cull.rs:477-482` — so a stale camera-transform/
frustum read in `cull_portals` can directly cause incorrect area-based occlusion in
the rendered frame, worst-case every frame if the app happens to register `BspPlugin`
before `Plugin3d`, and most visibly after camera teleports/fast cuts.

evidence: corrected citation from the original submission — the read site is
`cull.rs:443` (`world.get_resource::<VisibleAreas>()`), not line 431, which is
actually a write in the BSP-PVS branch that overwrites `VisibleAreas` when a
`BspLevel` is loaded.

verifier: confirmed the load-bearing consumer at `cull.rs:477-482` is real, not dead
code, and confirmed no `dependencies()` override exists in `BspPlugin` to guarantee
ordering relative to `Plugin3d`.

## corr-34 — crates/lunar-core/src/behavior.rs:410-418 (BehaviorPlugin::build)

- **impact:** medium
- **effort:** S
- **confidence:** medium
- **verdict:** confirmed

*(verifier-corrected wording)* `instantiate_pending_behaviors` and
`dispatch_update_system` are both added to the Update stage with no `.after()`/
`.chain()` between them. both bevy_ecs executors default `apply_final_deferred: true`
and Lunar never overrides it, and each stage is its own top-level `Schedule` run
directly via `schedule.run(&mut world)`. that final apply-deferred call fires only
once, after every system in that schedule invocation has already executed. so within
a single Update-stage tick, `instantiate_pending_behaviors`'s Commands (inserting
`Behaviors` / removing `PendingBehaviors`) are flushed into the World strictly after
`dispatch_update_system` (an exclusive `world: &mut World` system) has already run
that same tick — **deterministically**, on every run, single- or multi-threaded,
regardless of execution order. the result is a guaranteed one-tick delay between when
a `PendingBehaviors` ref becomes resolvable and when its `on_ready`/`on_update`
actually fires, contradicting the "early each update ... so ... runtime-spawned
pending refs get instantiated" comment.

evidence: reachable at runtime, not just startup — `lunar-3d/src/scene_format_3d.rs`'s
public `spawn_scene`/`load_and_spawn` attach `PendingBehaviors` via Commands and can
be called during gameplay (e.g. spawning a prefab/enemy mid-game), so any such
entity's behaviors start one tick later than the code implies. no existing test
checks same-tick dispatch, only post-tick component presence.

verifier: this is a stronger, more precise mechanism than the original submission's
framing (a nondeterministic multi-threaded race) — it's a deterministic one-tick lag
baked into bevy's deferred-command-apply model, confirmed by reading the vendored
executor source directly.

## corr-35 — crates/lunar-core/src/world_manifest.rs:500-512 (EntityData::get_component / set_component)

- **impact:** medium
- **effort:** S
- **confidence:** medium
- **verdict:** confirmed

*(verifier-corrected wording)* `EntityData::get_component` uses `.ok()` on
`serde_json::from_value`, so a component that is present but has the wrong/
incompatible JSON shape for `T` is indistinguishable from a component that was never
set — both yield `None`. separately, `EntityData::set_component` discards the
`Result` of `serde_json::to_value` with no else branch and no logging, so on the rare
occasions serialization does fail (e.g. a custom `Serialize` impl that returns `Err`,
or a map-keyed component with non-string/non-finite keys) the write silently no-ops
with no diagnostic. note: NaN/Infinity float fields do **not** trigger this path —
`serde_json`'s `to_value` converts non-finite floats to JSON `null` and returns `Ok`
(verified in the vendored 1.0.150 source), so such components are still inserted; the
resulting `null` then surfaces as a `get_component` `.ok()`-swallowed deserialize
failure on read, not as a `set_component` drop.

evidence: both functions currently have zero callers anywhere in the engine besides
one unit test, but `EntityData` is re-exported as public API from lunar-core, so this
is reachable by game code, just not yet exercised internally.

verifier: disproved the original submission's central motivating example (NaN
causing the component to vanish entirely) by reading the vendored serde_json source
directly — the actual failure mode is narrower and different in kind, so the claim
needed correcting rather than standing as submitted.

## corr-36 — crates/lunar-input/src/lib.rs:1496-1528 (button loop), 1530-1559 (axis loop)

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`poll_gamepads` (wasm target) never reads the browser standard-mapping's analog
trigger buttons (index 6 = LT, 7 = RT), so `GamepadAxis::LeftTrigger`/`RightTrigger`
are permanently stuck at 0.0 on web regardless of how far a trigger is pulled — a
defect independent of, and still present after, corr-04 (the `add_gamepad` gap) is
fixed.

evidence: the button-index match (1504-1519) maps 0,1,2,3,4,5,8,9,10,11,12,13,14,15
and falls to `_ => None` for indices 6 and 7 (the standard Gamepad API's analog LT/RT,
exposed via `GamepadButton::value()`). the axis loop (1530-1559) only reads
`gamepad.axes().get(0..3)` for the two sticks; nothing ever calls
`push_gamepad_axis(index, GamepadAxis::LeftTrigger/RightTrigger, ...)`.

verifier: confirmed the native SDL path correctly populates these same axes via
`SdlAxis::TriggerLeft/TriggerRight`, and `GAMEPAD_AXIS_COUNT` is explicitly 6 with a
comment naming triggers as one of the tracked pairs — proving the engine's data model
expects these to be live; 2 of 6 tracked axis slots are structurally dead on wasm.

## corr-37 — crates/lunar-input/src/lib.rs:360-412 (KeyCode variants), 1106-1179 (keycode_from_sdl), 1385-1450 (key_from_web), 563-564 & 1193-1213 (GamepadButton::Share / gamepad_button_from_sdl)

- **impact:** medium
- **effort:** M
- **confidence:** high
- **verdict:** confirmed

roughly half of the public `KeyCode` variants (all punctuation beyond Minus/Equals/
brackets/Grave, the whole navigation cluster, numpad, lock keys, super keys, media
keys) plus `GamepadButton::Share` have no match arm in the native or web
input-translation functions, so binding them via `ActionMap` compiles but the key/
button can never actually fire on any shipping target, with no error or log.

evidence: `keycode_from_sdl` only matches A-Z, Num0-9, F1-F12, Escape/Space/Enter/
Tab/Backspace/arrows, shift/ctrl/alt pairs, and Minus/Equals/brackets/Grave, falling
to `_ => None` for everything else — confirmed the sdl3 crate does expose matching
variants for all the missing keys, so this isn't a backend limitation, just unmapped.
`key_from_web` has the identical gap. `GamepadButton::Share` has no arm in
`gamepad_button_from_sdl` even though `sdl3::gamepad::Button::Misc1` (the share/
capture button) exists in the vendored crate.

verifier: traced git blame — commit 87d9109 expanded `KeyCode` to a full common set
but never touched `keycode_from_sdl`/`key_from_web`, so the enum was expanded without
ever wiring the new variants to real input; no TODO/FIXME/"partial" comment anywhere
marks this as a known limitation.

## corr-38 — crates/lunar-input/src/lib.rs:7-10 (module doc), 254-266 (is_just_pressed), 268-278 (is_just_released)

- **impact:** medium
- **effort:** M
- **confidence:** medium
- **verdict:** confirmed

the crate's documented contract is that "just pressed" is edge-triggered (fires once,
the frame a button transitions from up to down), but `InputBinding::is_just_pressed`
for a `GamepadAxis` binding returns the same value as `is_held` on every tick the axis
stays past threshold, so an action bound to an analog axis (e.g. a trigger used as a
button) re-fires every tick it is held instead of once, unlike the same action's
Key/Mouse/GamepadButton bindings.

evidence: `is_just_pressed`'s `GamepadAxis` arm (262-264) is textually identical to
`is_held`'s `GamepadAxis` arm (248-250) ("axis has no edge-triggered press, treat as
held"). `is_just_released`'s `GamepadAxis` arm instead just returns `false`
unconditionally — the two edges are handled inconsistently with each other and with
the documented contract, and this asymmetry is never surfaced in the public rustdoc
for `InputBinding::GamepadAxis` or `ActionBuilder::axis`/`axis_for`.

verifier: confirmed Key/Mouse/GamepadButton's `is_just_pressed` genuinely route
through real per-frame edge-detected bitsets, making the "unlike Key/Mouse/
GamepadButton" comparison hold; confirmed the only existing axis test never
exercises `is_action_just_pressed` over consecutive ticks.

## corr-39 — crates/lunar-lightmap/src/baker.rs:272-297 (build_triangles), called from public LightmapBaker::bake (:122-127) and bake_directional (:186-189)

- **impact:** medium
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`build_triangles` indexes `verts[tri[0]]`, `verts[tri[1]]`, `verts[tri[2]]` directly
from the mesh's index buffer with no bounds check against `verts.len()`; any
`MeshData` passed to the public `LightmapBaker::bake`/`bake_directional` API whose
indices reference a vertex beyond the vertex array (a plausible caller mistake — no
validation exists anywhere upstream of this function) panics with an
index-out-of-bounds instead of returning a `Result`.

evidence: `indices.chunks_exact(3).map(|tri| { let v0 = &verts[tri[0]]; ... })` with
no `< verts.len()` check anywhere. `bake`/`bake_directional` are public functions
returning `BakeResult` directly (not `Result`), so an out-of-range index is an
unrecoverable panic. `MeshData` (`lunar-3d/src/mesh.rs:136-147`) has fully public
`vertices`/`indices` fields with no invariant enforcement.

verifier: confirmed no test module exists in `baker.rs` at all; confirmed the
lightmap baker is a strictly more panic-prone consumer of the same class of
malformed data than the renderer's own mesh-upload path (which never indexes verts
by tri index on the CPU side).

## corr-40 — crates/lunar-plugin-loader/src/lib.rs:173-207 (PluginLoader::reload_coreclr)

- **impact:** medium
- **effort:** M
- **confidence:** medium
- **verdict:** confirmed

*(verifier-corrected wording)* on the CoreCLR hot-reload path,
`reinstantiate_behaviors` (line 202) — which drops the old `CsBehavior` boxes and
thus frees their GCHandles — runs only after `host_reload` (line 193) has already
returned. `PluginHost.Reload` unloads the old `AssemblyLoadContext` and forces three
`GC.Collect()`+`WaitForPendingFinalizers()` rounds *before* those old GCHandles are
released, so that forced-collect loop can never actually finish reclaiming the
outgoing ALC (a live GCHandle to one of its types blocks the whole ALC from being
collected). the result is not the crash-shaped hazard of "GCHandles pointing into an
already-gone ALC" the in-code CAVEAT comment describes (the old ALC is very likely
still resident when `drop_instance` is called, which is why this doesn't currently
crash) — rather, the "old context is GC-collected... before the new version runs"
guarantee promised in the docs is not actually met: cleanup of the outgoing ALC is
silently deferred past the point the docs claim it completes, and stays open as the
in-file TODO already acknowledges.

evidence: `coreclr` is the workspace-default feature (root `Cargo.toml:72`,
`default = ["coreclr"]`), not an obscure opt-in, and the NativeAOT path is explicitly
documented as safe by contrast ("old libraries stay mapped").

verifier: traced through `PluginHost.cs`'s actual `Reload()` implementation to
determine the real mechanism, correcting the original submission's crash-shaped
"dangling GCHandle" framing to the more accurate "deferred/incomplete teardown"
characterization.

---

## low

## corr-43 — crates/lunar-3d/src/visibility.rs:128-151, crates/lunar-3d/src/camera.rs:32-43

- **impact:** low
- **effort:** S
- **confidence:** high
- **verdict:** confirmed

`Frustum::from_view_proj` derives the near clip plane using the OpenGL (-1..1 NDC)
Gribb/Hartmann formula `r3 + r2`, but every projection matrix in the engine
(`camera.rs:35,40`, `rh::proj::directx::perspective`/`orthographic`) is built for the
DirectX/Vulkan/wgpu 0..1 NDC-Z convention, whose correct near plane is `r2` alone; the
mismatch makes near-plane culling far more permissive than intended. because the
culler's own contract treats false positives as safe, the failure mode stays bounded
to redundant draw calls for geometry between the true near clip and roughly half that
distance, rather than dropped geometry — hence low impact despite being an
objectively wrong formula.

evidence: derived the crossing point algebraically for the buggy formula:
`z0 = -near*far/(2*far-near)`, which for `near<far` is always strictly closer to the
camera than the true near distance. verified numerically for fovy=60°, aspect=16:9,
near=0.1, far=500: the buggy formula only starts rejecting points past view-space
z≈-0.05, half of the configured near=0.1. both the CPU SIMD cull
(`cull_aabbs_soa`, consuming `frustum.planes`) and the GPU indirect cull
(`cull_indirect.wgsl`) share this bug, uploaded from the same `Frustum.planes`.

verifier: pulled glam 0.33.2's actual source to independently re-derive the correct
near-plane formula for a `[0,1]` Z-range matrix rather than trusting the claim's math;
confirmed existing SIMD tests only check internal consistency against whatever planes
`from_view_proj` produces, never against true view-space geometry, so this would not
be caught by CI.

## corr-44 — crates/lunar-3d/src/visibility.rs:89-113

- **impact:** low
- **effort:** S
- **confidence:** medium
- **verdict:** confirmed

*(verifier-corrected wording)* `Aabb3d::from_positions(&[])` (an empty positions
slice) produces a degenerate AABB that violates the type's own documented invariant
("half_extents ... always positive"): `min = f32::MAX`, `max = f32::MIN` are never
updated for an empty slice, so `half_extents = (max - min) * 0.5` overflows f32 range
and evaluates to exactly **negative infinity** (verified by direct execution), not
just "a large negative value." in `Frustum::intersects_aabb`, this `-inf`
half_extents combined with a frustum plane whose normal has all-nonzero components
(the common case for any non-axis-aligned camera) makes `signed_radius` evaluate to
exactly `-inf`, which forces the cull test to be **unconditionally true regardless of
the object's actual position** — a guaranteed, not merely risked, false-negative
(dropped-geometry) cull, matching the danger the module's own docs call out. when a
plane normal has a zero component instead, the term becomes NaN, which happens to
bias toward "not culled" instead — so behavior is orientation-dependent but the
dangerous branch is real and common.

evidence: this same degenerate value also propagates through `world_space_aabb` into
the SIMD cull path actually used by the renderer's mid/low tiers, not just the scalar
path. `Aabb3d` is public API with no internal callers of `from_positions` and no
`Default`/empty-mesh guard, so a mesh with zero vertices reaches this path unguarded.
correction to the original submission: `Aabb3d` is re-exported at the lunar-3d crate
root but is **not** included in `lunar_3d::prelude` — it is reachable via
`lunar::lunar_3d::Aabb3d`, not via `lunar::prelude::*`.

verifier: ran the actual f32 arithmetic to confirm the exact degenerate value (-inf,
not merely "large and negative") rather than trusting the claim's characterization;
this makes the mechanism more severe (a guaranteed cull, not just a risked one) than
originally stated.

---

## appendix — refuted findings

- **HZB occlusion buffer frozen at construction-time window size, allegedly causing
  stale occlusion culling / entity pop-in after resize** — refuted: both the build
  path (depth prepass) and the test path (occlusion query) always use the identical
  snapshotted `(view_proj, hzb_width, hzb_height)` triple plus the live GPU texture's
  real dimensions queried at draw time, so production and consumption of the HZB can
  never diverge regardless of the current window's aspect/resolution; the frozen size
  is a resolution/VRAM staleness nit, not a correctness bug (unlike the genuinely
  divergent contact-shadow sibling, corr-24).

- **Audio backend `submit()` silently no-ops forever once the mixer/callback thread
  has exited (device disconnect, callback panic, sidecar bridge loss), permanently
  dropping all future `AudioPlayer::play()` calls with no log** — refuted: sender and
  receiver are sibling fields sharing one lifetime and are dropped together atomically
  (no code path drops one without the other); SDL3 transparently reroutes audio
  underneath the app on a default-device disconnect per its own documented behavior;
  and a callback panic crossing a non-`C-unwind` extern boundary under this
  workspace's `panic=abort` release profile aborts the whole process outright rather
  than leaving a degraded, silently-no-op'ing process behind.
