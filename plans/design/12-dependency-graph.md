# Crate Dependency Graph

```
lunar (binary)
├── engine-core
│   ├── bevy_ecs
│   ├── lunar
│   ├── engine-math
│   └── log
├── engine-render
│   ├── wgpu
│   ├── raw-window-handle
│   ├── engine-math
│   └── log
├── engine-input
│   ├── sdl3
│   ├── engine-math
│   └── log
├── engine-audio
│   ├── miniaudio (future)
│   └── log
├── engine-math
│   └── glam
├── lunar
│   ├── bevy_ecs (re-export)
│   ├── engine-math (re-export)
│   └── log
├── sdl3
├── wgpu
├── raw-window-handle
├── tokio
├── env_logger
└── log
```

Game project:
```
my-game
├── lunar          # primary dependency
└── lunar          # for lunar_app! macro
```

---

[← Back to Extensibility](11-extensibility.md) | [Next: Initialization Order →](13-initialization-order.md)
