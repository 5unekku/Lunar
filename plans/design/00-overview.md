# Lunar Game Engine — Design Document

> Version: 0.2 (design phase)
> Date: 2026-04-25
> Status: In-progress architecture specification

Lunar is a 2D game engine written in Rust. It targets Windows 7+, Windows 10/11, Linux, macOS, and web (WebGPU). The engine is fast and lean, designed to run on low-end hardware. The API is clean Rust — a mix of functions and macros, no custom scripting language.

## Core Design Principles

- **Engine owns all memory.** Game logic operates on handles, never direct references to engine-owned resources.
- **Rendering is decoupled from game logic.** The render layer is separate for purity. It is not designed to be swappable, but game code never touches it directly.
- **Core game loop is its own layer.** Fixed tickrate buckets (60/120/240hz) based on frame cap.
- **Command registry exists from day one.** Hookable from anywhere.
- **Subsystem init order is explicit and loose.** No hidden dependencies.
- **No god objects.** State is always inspectable from outside subsystems.
- **Web target kept in mind.** No wasm-breaking assumptions.
- **2D architecture should not paint 3D into a corner.** Spatial types and render pipeline are structured to allow future 3D expansion.
- **Game code never touches native APIs.** SDL3, wgpu, OS handles — they are beneath the engine boundary. The `lunar` facade crate is the game's only dependency.
- **`unsafe` is never required for basic engine features.** The engine shouldn't force `unsafe` onto game code. Game code can opt into `unsafe` for its own optimizations, but the engine should never mandate it for routine tasks.
- **2D only — for real.** No 3D scaffolding in this engine. 3D, if it ever exists, will be a sister project. See [appendix C](appendix-c-3d-future.md). Prioritize 2D simplicity now; pay the rewrite tax later if needed.
- **The ECS backend is internal.** `bevy_ecs` is an implementation detail like SDL3 or wgpu. Game code uses `lunar::prelude` exclusively and never names `bevy_ecs` in its `Cargo.toml`. The prelude re-exports a curated, stable set of types; the underlying ECS can be swapped without breaking game code.
- **Editor lives downstream, not in this repo.** A visual editor is an *end goal* but not part of the engine. It will consume `lunar` as a dependency from a separate project (the inverse of the Moonwalker relationship). The engine never depends on editor code; direct API usage is always sufficient.
- **High-level draw API is the contract.** Game code spawns `Sprite` / `Text` components and the engine renders them; immediate-mode helpers (`draw_sprite`, `draw_rect`, `draw_text`, `draw_line`) cover HUD/debug. The raw `RenderQueue::push(DrawCommand{…})` is internal — `#[doc(hidden)]`.
- **Domain systems live in their own crates.** Dialogue, localization, world-zones — RPG-shaped systems — are separate crates outside `engine-core` and outside the default `lunar` re-export. Games that need them opt in by adding the crate; games that don't pay zero compile cost.

## Current Workspace Structure

```
lunar/
├── Cargo.toml              # workspace root + binary
├── src/main.rs             # entry point (wiring prototype)
├── crates/
│   ├── engine-core/        # game loop, ECS wiring, state machine, command registry
│   ├── engine-render/      # wgpu 2D rendering
│   ├── engine-input/       # SDL3 input handling
│   ├── engine-assets/      # handle-based asset server, async loading, hot reload
│   ├── engine-image/       # custom image format (zstd-compressed)
│   ├── engine-atlas/       # texture atlas packer
│   ├── engine-math/        # glam re-exports
│   └── lunar/           # public-facing API for game logic (use lunar::prelude::*)
```

> **Audio:** `engine-audio` is intentionally absent. Audio is owned by a separate
> project (Moonwalker, cpal-based, WASM-compatible) and will return as a crate
> once mature. The `AudioPlugin` slot in init order, plugin system, and subsystem
> APIs is reserved for that integration — no stub crate lives in this workspace.

## Table of Contents

1. [Overview](00-overview.md) (current)
2. [Game Developer Experience](01-developer-experience.md)
3. [Entity/Component Model](02-ecs-model.md)
4. [Handle System](03-handle-system.md)
5. [Subsystem APIs](04-subsystem-apis.md)
6. [World and Zone Management](05-world-zones.md)
7. [Dialogue and Text System](06-dialogue-system.md)
8. [Asset Pipeline](07-asset-pipeline.md)
9. [Plugin System](08-plugin-system.md)
10. [Macros](09-macros.md)
11. [Error Handling](10-error-handling.md)
12. [Extensibility](11-extensibility.md)
13. [Crate Dependency Graph](12-dependency-graph.md)
14. [Initialization Order](13-initialization-order.md)

**Appendices:**
- [Complete Example — Top-Down Shooter](appendix-a-complete-example.md)
- [Web Target Considerations](appendix-b-web-targets.md)
- [3D Future Compatibility](appendix-c-3d-future.md)

---

[Next: Game Developer Experience →](01-developer-experience.md)
