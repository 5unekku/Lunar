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

## Current Workspace Structure

```
lunar/
├── Cargo.toml              # workspace root + binary
├── src/main.rs             # entry point (wiring prototype)
├── crates/
│   ├── engine-core/        # game loop, ECS wiring, state machine, command registry
│   ├── engine-render/      # wgpu 2D rendering
│   ├── engine-input/       # SDL3 input handling
│   ├── engine-audio/       # miniaudio stub
│   ├── engine-math/        # glam re-exports
│   └── engine-api/         # public-facing API for game logic
```

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
