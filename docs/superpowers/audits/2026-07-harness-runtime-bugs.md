# harness-discovered runtime bugs

date: 2026-07-17
source: the phase-0 render-bench harness, first runs on RX 7800 XT / RADV (Mesa 26.1.4), Vulkan

these are process-abort crashes the bench caught at runtime by actually rendering the
engine's public "everything on" configuration (`QualitySettings::maximum()` +
`DevRenderProfile::full()`). none were surfaced by the six static audits — they only
manifest when a specific feature pass records GPU commands, which no existing test or
example does. each is a wgpu validation error, which the engine handles as fatal (no
uncaptured-error handler is registered), so under the release `panic = "abort"` profile
each is a hard process kill the moment the feature renders.

the common root cause is the same across all three: these feature passes are never
exercised by any test, example, or CI leg (see the build-matrix audit — CI has no GPU
shadow/feature coverage, every ci.yml run in history failed, and none has run at all
since 2026-06-13), so bind-group and pipeline-layout regressions from the 2026-06/07
render work landed unvalidated.

---

## rt-01 — shadow-cascade dynamic-offset mismatch — FIXED (commit 614c790)

- **location:** crates/lunar-render-3d/src/init.rs:626 (`has_dynamic_offset`) vs passes.rs:1879 (parallel
  rayon recording) and passes.rs:1944 (serial recording)
- **impact:** critical (process abort)
- **status:** fixed 2026-07-17, test-first

the `[shadow globals]` bind-group layout declared `has_dynamic_offset: false`, but the
cascade shadow pass binds it per cascade with a 256-byte dynamic offset (one slot per
cascade, buffer sized `NUM_CASCADES * UNIFORM_STRIDE`). any shadow-casting directional
light aborted the process with a dynamic-offset-count validation error the moment the
cascade pass ran — i.e. every outdoor scene with sun shadows at a tier/profile that
enables cascades.

fix: declare `has_dynamic_offset: true` and bind a single 64-byte slot window (a
whole-buffer binding would let offset 512 + size 768 run past the buffer end). a headless
regression test (`headless_directional_shadows_render_without_validation_errors`,
lib.rs) spawns a shadow-casting sun + caster and renders three frames; it SIGABRTs before
the fix, passes after.

## rt-02 — detail-sprite pipeline layout missing its bind group — FIXED

- **location:** crates/lunar-render-3d/src/resources.rs:880-930 (`ensure_detail_sprite_resources`,
  lazy layout + pipeline creation) vs detail_sprite.wgsl:98-101; bind site in passes.rs
- **impact:** critical (process abort on pipeline creation)
- **status:** fixed 2026-09-29, test-first (`headless_detail_sprites_render_without_validation_errors`).
  the render layout is now `[None, Some(bgl)]` with the draw binding group 1 (the
  shader module is shared with the compute pass, which owns group 0); the instance
  buffer is vertex-visible; `SpriteGlobals` mirrors the engine `Globals` and the
  billboard basis is derived from `cam_pos` (cylindrical, y-up)

with a `DetailDensity` component present, the engine builds `[detail sprite] pipeline`
and wgpu rejects it: the vertex shader declares `@group(1) @binding(0)` but that binding
is absent from the pipeline layout ("Shader global ResourceBinding { group: 1, binding: 0 }
is not available in the pipeline layout"). the detail-sprite (grass/foliage) feature
therefore crashes the moment any `DetailDensity` entity exists.

repro: add a `DetailDensity` to any 3d scene and render one frame (the bench's
feature-reel scene did exactly this before the component was removed from it).
fix direction: the mismatch is the whole group index, not one binding. all four render
bindings in detail_sprite.wgsl are `@group(1)`, but the render layout puts its only BGL at
index 0 and passes.rs sets bind group 0. renumber the render bindings to `@group(0)`
(the compute bindings already use group 0 in a separate pipeline). do not drop
`sprite_globals` — `vs_sprite` reads it.

latent second bug, exposed once the layout is fixed: binding 0 is the engine's shared
`globals_buf` (`Globals` in shader.wgsl: view_proj, cam_pos, elapsed_secs, delta_secs,
lighting_model, ...), but the shader reads it as `SpriteGlobals { view_proj, cam_right,
cam_up, cam_pos }`. `cam_right` / `cam_up` would alias cam_pos/elapsed and
delta/lighting_model, producing garbage billboards. either bind a dedicated sprite
uniform with camera right/up, or derive right/up from `view_proj` in the shader and
declare binding 0 as the real `Globals`. do not re-add DetailDensity to feature-reel
until both are fixed, or the golden frame records broken output.

## rt-03 — hdr color attachment used as RESOURCE and COLOR_TARGET in one pass — FIXED

- **location:** crates/lunar-render-3d — the water pass (passes.rs:811-842) and `[water] bg0`
  (init.rs:2816, config.rs:325, rebuilt in post.rs:1337)
- **impact:** critical (process abort)
- **status:** fixed 2026-09-29, test-first (`headless_water_renders_without_validation_errors`).
  the water pass copies the hdr target into `[water] refraction source` (sized lazily)
  and `[water] bg0` samples that; the bind group is now built in one place
  (`build_water_bg0`) instead of three

with a `Water` plane present, a frame aborts:
"Texture with '[hdr] color attachment' label ... conflicting usages. Current usage
TextureUses(RESOURCE) and new usage TextureUses(COLOR_TARGET)". one of these passes binds
the hdr color target as a sampled resource (e.g. water refraction reading the scene color)
while it is still the active color attachment, which wgpu forbids within a single pass
scope.

only water does this: `[water] bg0` binds `hdr_view` at binding 1 for refraction, and
the `[water] pass` uses `hdr_view` as its color (or MSAA resolve) target. decal and
particle are not implicated — `[decal] bg0` samples `gtao_depth_tex`, and
`particle_render.wgsl` binds only globals and the particle storage buffer.

repro: add a `Water` plane to any 3d scene and render one frame (the bench's feature-reel
scene did this before water/decal/particle were removed from it).
fix direction: copy the hdr color into a separate refraction texture
(`copy_texture_to_texture` before the water pass) and bind that in `[water] bg0`,
rather than reading the live color attachment.

---

## bench coverage impact

the phase-0 harness scenes exercise the passes that render cleanly today (static geometry,
dynamic geometry, cascade + point shadows, clipmap terrain, atmospheric sky, 2d sprites +
text). the feature-reel scene omits DetailDensity (rt-02) and Water (rt-03) until those
fixes land. Decal and ParticleEmitter were removed alongside Water but are not
implicated in rt-03 and can be re-added now so their passes gain golden-frame coverage.
