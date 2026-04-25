# Lunar

a 2D game engine built in Rust.

## stack

- **wgpu** — cross-platform graphics API (Vulkan/DX12/Metal/WebGPU)
- **SDL3** — windowing and input
- **bevy_ecs** — entity component system (standalone)
- **glam** — math library

## architecture

- rendering is decoupled from game logic
- engine owns all memory, game logic operates on handles
- fixed tickrate correlated to frame cap with three buckets:
  - frame cap 1-60: 60hz tick
  - frame cap 61-120: 120hz tick
  - frame cap 121+: 240hz tick
- rendering runs uncapped for smooth high-framerate gameplay

## crates

| crate | purpose |
|---------|
| `engine-core` | game loop, ECS wiring, engine state, command registry |
| `engine-render` | wgpu 2D rendering |
| `engine-input` | SDL3 input handling |
| `engine-audio` | audio subsystem (stub) |
| `engine-math` | glam re-exports and custom math |
| `engine-api` | public API for game logic |

## getting started

```bash
cargo run
```

## targets

- Windows 7+
- Windows 10/11
- Linux
- macOS
- Web (via WebGPU)
