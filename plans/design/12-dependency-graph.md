# Crate Dependency Graph

```
lunar-game (binary, this workspace)
├── lunar                         # public API facade
│   ├── lunar-macros               # wrapped Component/Resource/Event/Message derives
│   ├── lunar-core
│   │   ├── bevy_ecs                  (sealed via __bevy_ecs in lunar; not re-exported)
│   │   ├── lunar-math
│   │   └── log
│   ├── lunar-render
│   │   ├── wgpu
│   │   ├── raw-window-handle
│   │   ├── lunar-assets
│   │   ├── lunar-math
│   │   └── log
│   ├── lunar-input
│   │   ├── sdl3                      (cfg: not wasm)
│   │   ├── lunar-math
│   │   └── log
│   ├── lunar-assets
│   │   ├── lunar-image
│   │   ├── crossbeam-channel
│   │   └── notify                    (cfg: not wasm — hot reload)
│   ├── lunar-image
│   │   ├── zstd
│   │   └── image
│   ├── lunar-atlas
│   │   └── lunar-math
│   ├── lunar-math
│   │   └── glam                      (Vec3/Vec4/Mat3/Mat4 re-exported but not consumed
│   │                                   by the engine surface; engine API is strictly 2D)
│   ├── pollster                      (cfg: not wasm — block on async wgpu init)
│   └── env_logger                    (cfg: not wasm)
├── opt-in domain crates              # game depends on these only when needed
│   ├── lunar-dialogue
│   │   └── lunar-core
│   ├── lunar-localization
│   │   └── lunar-core
│   └── lunar-zones
│       └── lunar-math
│
└── (no tokio, no rayon — async runtime not needed; pollster + std::thread +
   wasm_bindgen_futures cover I/O; bevy_ecs is the parallel scheduler.)
```

> **Reserved slot:** `engine-audio` will reappear here when the Moonwalker
> audio engine is wired in. Until then, no audio crate is in the workspace.

> **Deleted:** `lunar-render::mesh` and `lunar-render::render_pass_3d` were
> empty 3D scaffolding (~570 LOC, never instantiated). Removed in the 2D-only
> commitment. 3D, if it ever exists, is a sister engine — see
> [appendix-c-3d-future.md](appendix-c-3d-future.md).

Game project (downstream consumer):
```
my-game
├── lunar                          # always — the only required dep
├── lunar-dialogue                   # add only if the game has dialogue
├── lunar-localization               # add only if the game ships multiple languages
└── lunar-zones                      # add only if the game uses zoned area loading
```

Games that don't need a domain crate pay zero compile cost for it.

---

[← Back to Extensibility](11-extensibility.md) | [Next: Initialization Order →](13-initialization-order.md)
