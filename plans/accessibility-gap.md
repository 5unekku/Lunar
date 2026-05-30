# accessibility gap analysis

target: 60fps on any modern mid-range CPU for a full 3d game, matching the accessibility
of games like Halo CE/2/3, Quake 1/3, and Doom 1/3.

the premise is correct: PC gaming has become less accessible despite faster hardware.
modern engines optimise for visual parity with consoles and top-tier GPUs, not for
running well on modest hardware. a well-designed engine should achieve the same
accessibility those older games had — Halo CE ran at 30fps on a 733 MHz Pentium III.
a 2015 mid-range CPU is roughly 20-100× faster than that.

---

## what those engines actually did

**Quake 1 / Quake 3**
- BSP tree precomputed offline: the entire level is partitioned into convex leaves.
  rendering starts at the camera leaf and only visits nodes on the visible side of
  each partition plane. no runtime traversal cost for static geometry.
- PVS (potentially visible set): for every leaf, a precomputed bitset says exactly
  which other leaves can possibly be seen from it. culling is a bitset OR, not
  a geometric test. rendering a full Quake 3 map visits ~5-15% of geometry.
- hardware T&L offloaded transforms to the GPU from the start (Quake 3, 1999).

**Doom 1 / Doom 3**
- Doom 1: BSP + 2.5d rendering. extreme cache coherency, zero overdraw by design.
- Doom 3: area/portal system. the level is divided into areas (rooms, corridors).
  areas connect through portals. the engine only renders areas reachable through
  visible portals from the camera. a closed door eliminates entire wings of the map
  with zero GPU cost. shadow volumes were expensive but predictable and bounded.

**Halo CE / 2 / 3**
- Halo CE: BSP for level, precomputed lightmaps for all static lighting. runtime cost
  of a light is near zero for static geometry — it is already baked into textures.
  portal culling between BSP clusters. LOD for all outdoor geometry.
- Halo 2: added per-pixel lighting but kept lightmaps for static, runtime only for
  dynamic objects (players, projectiles, vehicles). strict LOD at every distance band.
- Halo 3: deferred lighting, but still lightmaps for statics. aggressive LOD + impostor
  system for far-distance objects. texture streaming based on screen coverage.

**the common thread**: all of them front-loaded expensive decisions offline (BSP build,
PVS computation, lightmap baking) so the runtime frame budget was almost entirely
spent on geometry that is definitely visible.

---

## gaps — all addressed

the gaps documented here have been closed. brief status:

1. **precomputed visibility**: `lunar-bsp` + `lunar-bsp-build` + `bake-pvs` tool. BSP
   compiler builds offline PVS bitmasks; runtime walk skips non-visible leaves.
2. **single-threaded render**: parallel command buffer recording via rayon, one encoder
   per graph node, submitted in topological order.
3. **serial ECS execution**: parallel system scheduling via bevy_ecs rayon executor.
4. **synchronous GPU cull readback**: 1-frame pipelined staging — reads previous frame's
   cull results, no `device.poll(Wait)` stall.
5. **all 3 shadow cascades every frame**: dirty-flag cascade re-rendering; static scenes
   skip all 3 cascade passes between camera moves.
6. **full post-processing on all tiers**: `QualityPreset::Minimum` disables GTAO, SSR,
   fog, and bloom entirely; composite reduces to tonemap + gamma only.
7. **no precomputed lighting**: `lunar-lightmap` baker and UV2 lightmap support wired into
   the renderer. runtime lighting applies to dynamic objects only.
8. **no texture streaming**: coverage-based mip streaming infrastructure with VRAM budget
   and LRU eviction.
9. **no impostors**: `MeshImpostor` component; camera-facing quad replaces mesh beyond
   the last LOD threshold.

---

## what we have that those games didn't

things we do that those games *could not* do, that help accessibility at higher entity counts:

- HZB GPU occlusion: finer-grained than portal culling for complex outdoor scenes.
- sweep-and-prune physics: matches or exceeds Quake 3's BSP-based clip hull tests
  for dynamic objects.
- FXAA + STAA: cheaper than MSAA for most content; STAA adds per-pixel temporal
  filtering for specular shimmering on non-moving surfaces.
- parallel ECS execution: game logic systems run concurrently on multiple cores.

**remaining known gap**: GPU instancing is not yet wired up — the draw loop still issues
one draw call per entity. batching entities sharing a mesh + material into a single
instanced draw is the next high-impact item for scenes with repeated geometry.
