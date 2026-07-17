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
shadow/feature coverage and every run has failed since 2026-06-13), so bind-group and
pipeline-layout regressions from the 2026-06/07 render work landed unvalidated.

---

## rt-01 — shadow-cascade dynamic-offset mismatch — FIXED (commit 614c790)

- **location:** crates/lunar-render-3d/src/init.rs:619 vs passes.rs:1687,1944
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

## rt-02 — detail-sprite pipeline layout missing its bind group — OPEN

- **location:** crates/lunar-render-3d — `[detail sprite] pipeline` creation (detail_sprite.wgsl + its pipeline-layout construction in init.rs)
- **impact:** critical (process abort on pipeline creation)
- **status:** open — reproduced, not yet fixed

with a `DetailDensity` component present, the engine builds `[detail sprite] pipeline`
and wgpu rejects it: the vertex shader declares `@group(1) @binding(0)` but that binding
is absent from the pipeline layout ("Shader global ResourceBinding { group: 1, binding: 0 }
is not available in the pipeline layout"). the detail-sprite (grass/foliage) feature
therefore crashes the moment any `DetailDensity` entity exists.

repro: add a `DetailDensity` to any 3d scene and render one frame (the bench's
feature-reel scene did exactly this before the component was removed from it).
fix direction: reconcile the detail-sprite pipeline layout with the group set the WGSL
actually binds (add the missing @group(1) BGL, or drop the unused binding from the shader).

## rt-03 — hdr color attachment used as RESOURCE and COLOR_TARGET in one pass — OPEN

- **location:** crates/lunar-render-3d — the water / decal / particle feature passes and the `[hdr] color attachment` texture
- **impact:** critical (process abort)
- **status:** open — reproduced, not yet fully isolated to a single feature

with a `Water` plane (and/or `Decal` / `ParticleEmitter` entities) present, a frame aborts:
"Texture with '[hdr] color attachment' label ... conflicting usages. Current usage
TextureUses(RESOURCE) and new usage TextureUses(COLOR_TARGET)". one of these passes binds
the hdr color target as a sampled resource (e.g. water refraction reading the scene color)
while it is still the active color attachment, which wgpu forbids within a single pass
scope.

repro: add a `Water` plane to any 3d scene and render one frame (the bench's feature-reel
scene did this before water/decal/particle were removed from it). the three features were
removed together, so the exact culprit among them is not yet pinned down.
fix direction: the feature that samples the scene color must render into a separate target
(or ping-pong / copy the hdr color to a read texture) before sampling it, rather than
reading the live color attachment.

---

## bench coverage impact

the phase-0 harness scenes exercise the passes that render cleanly today (static geometry,
dynamic geometry, cascade + point shadows, clipmap terrain, atmospheric sky, 2d sprites +
text). the feature-reel scene deliberately omits DetailDensity, Water, Decal, and
ParticleEmitter pending rt-02 / rt-03; re-add them to feature-reel as those fixes land so
their passes gain golden-frame coverage.
