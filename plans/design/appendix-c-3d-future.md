# 3D — Out of Scope (Sister Engine, Not an Extension)

**Decision (2026-05-03):** Lunar is a strictly 2D engine. 3D is *not* a future
extension of this codebase. If a 3D engine is built, it will be a separate sister
project that may share architectural patterns and lower-level building blocks
(handle system, asset crate, image format) but stands on its own.

## Why

- Trying to "leave the door open for 3D" forces speculative complexity into 2D-only
  code paths today (`Vec3` translations where `Vec2` would do, `Mat4` projections
  where `Mat3` suffices, etc.). YAGNI.
- 3D engines have substantially different concerns (depth, lighting, materials,
  meshes, scene graph, frustum culling) that warp every layer of an engine. A
  unified 2D/3D codebase pays for both up-front and is mediocre at each.
- Building 3D later as a sister engine is a "big change" — *acknowledged and
  accepted*. The win is a tight, focused 2D engine now.

## Implications for current code

- `Transform.translation` should be `Vec2`, not `Vec3`.
- `Camera` drops 3D-shaped fields it doesn't use.
- `lunar-math` keeps `Mat4`/`Vec3`/`Vec4` re-exports (glam provides them; cost is
  zero) but the engine's own APIs use 2D types.
- The render pipeline assumes orthographic 2D throughout — no `depth_stencil`,
  no view matrix beyond camera transform.

Tracked as Phase 11 / item 68 (2D-only strip) in `todo.md`.

---

[← Back to Web Targets](appendix-b-web-targets.md)
