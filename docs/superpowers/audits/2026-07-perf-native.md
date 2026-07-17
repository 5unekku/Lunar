# native performance audit

date: 2026-07-16
concern: native performance (cpu hot paths, gpu submission, bandwidth, startup)

method: multi-agent fan-out (one finder agent per lens), location dedup, then every
surviving finding was handed to an independent adversarial verifier whose default was
to refute it — only findings that held up under the verifier's own reading of the code
are listed below. read-only audit; no source was modified. this doc was transcribed by
the orchestrator from the verified finding set after the workflow's writer stage was
cut off by a subagent model-credit limit — the findings and verdicts are the agents', unedited.

scope: cpu hot paths, gpu submission, bandwidth, and cold-start on the native (linux/windows) render path.

**13 confirmed findings** (1 critical, 1 high, 5 medium, 6 low).

discovery stats: 54 raw findings from 5 lenses (+0 gap follow-ups), 54 after dedup, 13 confirmed, 0 refuted.

every finding carries: id, location (file:line), impact, effort (S/M/L), and the
verified claim (verifier-corrected wording where the skeptic adjusted it). ids are
assigned in severity order and are stable references for the phase-2 backlog synthesis.

---

## perf-01 — crates/lunar-render-3d/src/passes.rs:710 (terrain, also :809 water, :883 decal)

- **impact:** critical
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

On mid+ render tiers, terrain, water, and decal passes rewrite a single shared params uniform buffer inside their per-draw loops (passes.rs:710/:809/:883) while all draws are recorded on the frame encoder submitted once at frame.rs:2106, so wgpu's write-at-next-submit ordering makes every draw execute with the last iteration's params. For terrain this is a live correctness bug at defaults (clipmap_rings=5, identical ring patch meshes positioned solely by ring_origin/lod_cell_size in terrain.wgsl): all rings render as the coarsest ring, disabling clipmap LOD and making 4 of 5 draws redundant overdraw; for water and decals it corrupts rendering whenever 2+ such entities exist. The pattern also opens one full Load/Store render pass (plus MSAA resolve for terrain/water) per ring/entity instead of one pass per stage. Fix: pack per-draw params at UNIFORM_STRIDE slots bound with dynamic offsets, as already done for point_shadow_globals_buf (passes.rs:1443-1452, 1500-1504), collapsing each stage to a single pass.

## perf-02 — crates/lunar-render-3d/src/frame.rs:1471 (also frame.rs:1073-1174, frame.rs:1656-1707)

- **impact:** high
- **effort:** L
- **verdict:** confirmed (survived refute-by-default verification)

Every frame render_frame re-packs and re-uploads the full per-entity uniform + material staging range for all slots (needed * 320 bytes/frame: UNIFORM_STRIDE=256 + MATERIAL_UNIFORMS_SIZE=64), recomputing a 3x3 inverse-transpose per entity — plus an SH probe sample when a probe grid or IrradianceSH is active — even when nothing about a static entity changed. Slots are draw_scratch-indexed (a constraint passes.rs:161-163 documents as forcing per-frame static-bundle diffing), so change-gating requires persistent slot allocation (anchorable on static_entity_slots, cull.rs:561-601) plus an instance-to-slot indirection, since contiguous draw_scratch-order slots are load-bearing for instanced batch ranges, the CPU indirect-args builder, and the GPU-driven cull path.

## perf-03 — crates/lunar-3d/src/collision.rs:647-664 (build_collision_world_3d)

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

build_collision_world_3d (collision.rs:647-664) clears, rebuilds, and fully re-sorts (O(n log n) sort_unstable_by on min_x) the entire collider list every Physics tick — potentially multiple times per display frame under fixed-timestep catch-up — even when no collider or transform changed, with no change-detection early-out. A Changed<WorldTransform3d>/Changed<Collider3d> + count-delta gate (same pattern as systems.rs:58-84) would skip both rebuild and sort, but note the skip window is frames where the whole transform scene is quiet: propagate_transforms_3d's writeback (systems.rs:261-269) marks every WorldTransform3d changed whenever any transform changes, so the gate helps idle/static scenes but not scenes with any continuously animating transform.

## perf-04 — crates/lunar-3d/src/systems.rs:64-100 (also 260, 276)

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

propagate_transforms_3d builds fresh QueryStates via world.query_filtered/world.query up to five times per frame - two of them on the change-detection early-out path itself - paying archetype re-matching plus matched-archetype Vec allocation every frame, plus an O(n) entity walk for the count.

## perf-05 — crates/lunar-3d/src/visibility.rs:423-498 (build_cull_soa)

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

build_cull_soa (crates/lunar-3d/src/visibility.rs:422-498, scheduled unconditionally at plugin.rs:52) re-snapshots every visible (Aabb3d, WorldTransform3d) entity and re-runs the Mat3::from_quat rotate-expand (visibility.rs:409-417) for all of them every frame with no change-detection early-out, unlike the June gate in propagate_transforms_3d (systems.rs:58-84), whose recipe (Changed<> filters + relevant-entity count delta for despawns/removals) applies directly. Scope caveat: propagate_transforms_3d's writeback sweep (systems.rs:255-286) blanket-marks every WorldTransform3d/ComputedVisibility changed whenever it runs at all, so a Changed<>-based gate is all-or-nothing per frame — build_cull_soa's early-out fires exactly on the frames where propagate's own early-out fires (fully static world: menus, paused, idle scenes), and per-entity incremental rebuild is not achievable unless that writeback becomes change-aware (set_if_neq-style). wasmMacStory: portable — the wasm serial variant (visibility.rs:501-527) takes the same gate as plain system params; no cfg divergence needed.

## perf-06 — crates/lunar-render-3d/src/frame.rs:1800-1824 (late GPU indirect cull)

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The per-frame late-cull upload rebuild (frame.rs:1800-1824, gated on gpu_indirect_active) issues one random-access world.get::<Aabb3d>() archetype lookup per drawn entity plus one mega_mesh_entries hash probe per entity, although the gather query already fetches Option<&Aabb3d> (lib.rs:1405, discarded at cull.rs:487-501) and could plumb the AABB through raw_scratch/draw_scratch. Note the fix must plumb the world-space AABB (computable in the for_each from aabb + wt, as CullSoa does via world_space_aabb), not the raw component: the current code uploads the local-space Aabb3d to a world-space frustum test (cull_indirect.wgsl:53-63), so this is a wrong-space correctness smell as well as a perf cost; cull_aabb_scratch cannot be reused directly because it is in CullSoa order, not draw_scratch order.

## perf-07 — crates/lunar-render-3d/src/passes.rs:1599-1619

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Whenever the point-shadow block is active (dev_point_shadows true), unused point-shadow slots are re-cleared every frame with one render pass per face: with zero shadow-casting point lights that is 24 (MAX_POINT_SHADOW_LIGHTS=4 x 6 faces) depth render passes per frame, forever, re-clearing already-cleared point_shadow_res^2 (256-512^2) Depth32Float layers; with N casting lights it is (4-N)*6 passes. The block is skipped under an inserted classic()/standard() DevRenderProfile (point_light_shadows: false), but runs in the common out-of-the-box case where no DevRenderProfile resource is inserted, because passes.rs:1352-1355 uses unwrap_or(true) — contradicting the documented missing-resource default of classic() (lib.rs:966-967) — and always runs for games that enable point shadows (full() or with_point_light_shadows(true)). Fix: track the previous used-slot count and clear a slot only on the used->unused transition (also re-marking point_shadow_dirty on unused->used, since the slot's dirty flags were consumed and stale-equal last_positions would otherwise leave the re-used slot holding cleared depth); separately align the unwrap_or(true) with the documented classic() default.

## perf-08 — crates/lunar-render-3d/src/cull.rs:603-660 (gather_draw_list material resolve)

- **impact:** low
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

gather_draw_list (cull.rs:606-659) re-resolves material state per visible entity per frame with no per-mat_id memoization; the genuinely redundant part is cull.rs:654's unconditional mat_texsets.insert(mat_id, texset) — a hash write per raw_scratch entry every frame into a persistent map of per-id-immutable materials, re-writing identical data after first sight of each mat_id. Gating that insert (or recording texsets once per distinct mat_id) removes n hash writes/frame in many-entities-few-materials scenes. The per-entity get_material hash read and flag/cutoff quantization are near-washes under memoization (a memo lookup replaces the registry lookup 1:1, and the flag build is ~10 branchless ALU ops), so expect savings only from the write side unless a profile shows the resolve loop hot.

## perf-09 — crates/lunar-render-3d/src/cull.rs:9-14 (mapped_u32s, used at cull.rs:129 and cull.rs:266)

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Both per-frame GPU readbacks (LOD select indices, cull.rs:129; HZB occlusion flags, cull.rs:265) materialize the mapped buffer into a freshly allocated Vec<u32> of the pending entity count each frame on high tier; neither consumer needs materialization — the HZB path only tests flags[i] == 0 and the LOD path only walks indices sequentially — so inline chunks_exact(4) iteration (or a reused scratch Vec on self, matching the function's existing scratch-field pattern) removes a 4*n-byte alloc + copy from both hot readback paths.

## perf-10 — crates/lunar-render-3d/src/frame.rs:1282 (also post.rs:1054)

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The unjittered camera view-projection matrix is recomputed from scratch twice more per frame with identical inputs (pure quat-to-matrix + projection build + 4x4 multiply): at frame.rs:1282 for cluster params — inside render_frame itself, where the line-188 local view_proj_unjittered is still in scope and can be used directly — and at post.rs:1054 for GTAO params, which would need FrameContext (constructed frame.rs:2073-2101, currently carrying only the jittered view_proj) to also carry the unjittered matrix.

## perf-11 — crates/lunar-render-3d/src/frame.rs:633-654 (texture coverage hints)

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Coverage hints do a sqrt (Vec3::length) plus an FxHashMap entry-or-insert per lightmapped draw entry per frame, un-deduplicated (hashing per draw rather than per distinct lm_id), and the map is cleared and rebuilt each frame — yet the machinery's only consumer, desired_mip_count at frame.rs:679-680, discards its result ("upload full for now") and runs only on lightmap cache-miss upload, so the entire per-frame rebuild is currently dead overhead. Cheapest fix: skip or gate the hint block until mip-limited uploads are implemented; when they are, aggregate max-coverage per distinct lm_id in the reused sorted scratch (lm_needed_scratch pattern, frame.rs:658-666) before touching the map.

## perf-12 — crates/lunar-render-3d/src/passes.rs:1362-1378 (record_shadows caster gather)

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Every frame record_shadows iterates a second QueryState over shadow casters, rebuilds an FxHashSet<Entity> from it, and probes that set once per draw_scratch entry — yet shadow_list is only ever the intersection with draw_scratch, whose entries all come from the cullables gather and are already visibility-filtered. Carrying a Has<ShadowCaster> bool through the existing cullables gather (as surface_shaders already does with Has<Overlay>) eliminates the redundant caster query, the per-frame set rebuild, and the per-draw set probes, with identical shadow_list output; the single-use shadow_casters QueryState (lib.rs:1424) can then be deleted.

## perf-13 — crates/lunar-render-3d/src/passes.rs:137-138 (record_scene_passes sky gather)

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

When a panorama sky is active, a fresh QueryState is constructed every frame via world.query_filtered for SkySurface entities, contradicting the crate's own cached-FrameQueries pattern and paying archetype re-matching plus allocation per frame.
