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
spent on geometry that is definitely visible. our engine does the opposite — every
frame tests every entity and draws every shadow.

---

## current gaps (in priority order)

### 1. no precomputed visibility (BSP/PVS/portals)

**cost**: we test every entity with frustum cull + HZB every frame. for a 500-entity
indoor level, that is ~500 AABB tests + HZB projections per frame.

**what those games did**: for a 500-entity indoor level, PVS might visit 30 entities.
the rest are eliminated by a bitset lookup in microseconds.

**fix**: implement a BSP/BVH offline compiler for static level geometry. runtime
rendering walks only the visible nodes. for indoor maps, add area/portal culling as a
separate pass that prunes entire rooms before frustum testing.

**impact**: this is the single largest gap. on indoor levels it could cut visible entity
count by 80-95%, which cascades into fewer draw calls, fewer shadow map draws, less
vertex processing, less fragment shading.

### 2. single-threaded render command submission

**cost**: every draw call is issued from one thread. on a 16-core CPU, 15 cores sit
idle during `render_frame`. command encoding in wgpu is `Send`, so this is an artificial
bottleneck we imposed.

**what those games did**: also single-threaded, but their draw lists were short enough
(5-15% of geometry due to BSP/PVS) that single-thread submission was fast. we don't
have that geometric reduction yet, so single-thread hurts us more.

**fix**: parallel command buffer recording via Rayon. the render graph DAG already
models pass dependencies. each independent pass (shadow, zprepass, opaque, particles)
can record its `CommandEncoder` on a separate Rayon thread, then submit all in order.

**impact**: near-linear scaling with CPU core count for the command recording phase.
on an 8-core CPU, the opaque pass + shadow passes + post-processing could record in
parallel, collapsing ~60% of render_frame wall time.

### 3. no parallel ECS system execution

**cost**: game logic systems run one at a time on the main thread. physics, AI, animation,
pathfinding, particle updates — all sequential.

**fix**: bevy_ecs supports parallel system scheduling natively. its scheduler can
run non-conflicting systems concurrently. the `lunar-core` stage runner needs to use
`Schedule::run_with_executor` with a parallel executor (rayon-backed). this is wiring
work, not a new algorithm.

**impact**: on a 4-core CPU, AI + physics + animation could run simultaneously instead
of sequentially, recovering ~50% of game logic frame time.

### 4. synchronous GPU cull readback (new regression from tier 4)

**cost**: the GPU frustum cull and HZB cull both issue a `device.poll(Wait)`, which
blocks the CPU until the compute pass completes. this adds a synchronization point
that stalls the CPU for ~0.1-0.5ms per frame.

**fix**: 1-frame pipelined readback. copy cull results to a staging buffer, submit,
and read the *previous* frame's results. the 1-frame lag is imperceptible for culling.
this eliminates the stall entirely.

**impact**: removes 0.1-0.5ms CPU stall per frame (small but consistent).

### 5. three shadow map cascades every frame

**cost**: 3 depth passes over visible shadow-casting geometry, every frame, even when
the sun hasn't moved and nothing in the scene changed.

**what those games did**: Doom 3 used stencil volumes (CPU-computed, only for dynamic
lights). Halo CE used baked lightmaps for static geometry and only paid runtime shadow
cost for dynamic objects (characters). Quake 3 had projected blobs, not shadow maps.

**fix**: dirty-flag cascade re-rendering. cascade N is only re-rendered when geometry
or the light direction in that cascade's frustum has changed. static-only scenes can
freeze cascade re-rendering entirely between camera movements.

**impact**: in scenes with little dynamic content (most of any game), shadow passes drop
from 3 per frame to 0-1 per frame.

### 6. full post-processing stack on all quality tiers

**cost**: GTAO + SSR + volumetric fog + bloom + composite + FXAA is 6 full-screen passes
(7+ when counting bloom mip levels). on a 1080p screen that's 6 × ~2M texel reads/writes.

**what those games did**: Quake 3 had no post-processing at all. Halo CE had minimal
post (bloom only, added in PC port). accessibility came from having a quality toggle
that genuinely disabled expensive passes, not just reduced their resolution.

**fix**: add `QualityPreset::Minimum` that disables GTAO, SSR, fog, and bloom entirely.
post-processing should be strictly opt-in features at minimum settings, not present at
reduced quality. the current `Low` tier still runs several passes.

**impact**: on minimum settings, frame time from post-processing should be near zero.

### 7. no precomputed lighting / lightmaps

**cost**: every static surface is lit at runtime by the directional light and any nearby
point lights. this is correct physically but unnecessary for surfaces that never change.

**what those games did**: Halo CE, Quake, Doom — all precomputed lighting for static
geometry into lightmaps. runtime lighting cost applies only to dynamic objects.

**fix**: add a lightmap baker (offline tool or in-editor) that produces UV2 lightmap
textures for static geometry. the existing `uv_lightmap` field on `Vertex3d` is
already there, waiting. the shader already has a `uv_lightmap` input. wire it up.

**impact**: eliminates runtime lighting cost for static geometry, which is most of
any level. directional light + GTAO only needs to apply to dynamic entities.

### 8. no texture streaming

**cost**: all textures are loaded into VRAM at level start. for large worlds this
either hits VRAM limits or requires keeping low-res fallbacks.

**what those games did**: Halo 3 and later Quake engines streamed mipmaps based on
screen coverage. a distant rock face uses mip 4; walk toward it and mip 0 loads in.

**fix**: async texture mip streaming tied to the distance/screen-coverage of entities
using each texture. pair with a fixed VRAM budget and an eviction policy (LRU).
already noted in `plans/performance.md` under far-term.

### 9. no impostor/billboard system for far-distance objects

**cost**: MeshLod allows coarser meshes at distance, but at the farthest distances
(500m+) even the coarsest LOD mesh is more expensive than a camera-facing quad.

**what those games did**: Halo 3 replaced distant objects (trees, rocks, vehicles)
with flat billboards that approximate the object's silhouette. near-zero render cost.

**fix**: add a `MeshImpostor` component. at distances beyond the last LOD threshold,
render a camera-facing quad with a pre-rendered impostor texture instead of any mesh.
Ogre's `StaticGeometry` / impostor page system is the cleanest reference.

---

## what we already have that those games didn't

for fairness: things we do that those games *could not* do that help accessibility
at higher entity counts:

- HZB GPU occlusion: finer-grained than portal culling for complex outdoor scenes.
- Sweep-and-prune physics: matches or exceeds Quake 3's BSP-based clip hull tests
  for dynamic objects.
- FXAA: cheaper than MSAA while giving similar results for non-hard edges.
- parallel ECS execution: game logic systems run concurrently on multiple cores.

note: GPU instancing is **not yet implemented** — the draw loop still issues one
draw call per entity. this remains a known gap (GPU instancing); the draw loop
should batch entities sharing a mesh + material into a single instanced draw.

---

## realistic milestone: 60fps on a 2015 i5/Ryzen 3

this is achievable with items 1 (static BSP/portals), 2 (parallel command recording),
3 (parallel ECS), 5 (dirty cascade shadows), and 6 (true minimum quality tier).

items 7 (lightmaps) and 8 (streaming) are needed for large worlds but not for a
small-to-medium game. items 4 and 9 are polish.

the core thesis is correct: a 2025 mid-range CPU has 100× the compute of the hardware
those classic games targeted. the engine's job is to not waste that headroom on
redundant work that precomputation or smarter culling would eliminate.
