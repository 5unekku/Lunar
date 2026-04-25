# Error Handling

## Design Philosophy

- Engine errors are **recoverable** where possible
- Game code receives errors through **events**, not panics
- Fatal errors (window creation failure, GPU unavailable) **panic** with clear messages
- Asset loading failures are **non-fatal** — handles become invalid, game code checks

## Error Types

```rust
/// Engine error types
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("window creation failed: {0}")]
    WindowCreation(String),

    #[error("GPU initialization failed: {0}")]
    GpuInit(String),

    #[error("asset load failed: {path}")]
    AssetLoad { path: String, source: Box<dyn std::error::Error + Send + Sync> },

    #[error("invalid handle: {0}")]
    InvalidHandle(String),

    #[error("scene not found: {0}")]
    SceneNotFound(String),

    #[error("command error: {0}")]
    Command(String),
}
```

## Error Events

```rust
/// Error event — emitted when a recoverable error occurs
#[derive(Event)]
pub struct ErrorEvent {
    pub source: ErrorSource,
    pub error: EngineError,
    pub recovered: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ErrorSource {
    Asset,
    Render,
    Audio,
    Input,
    Scene,
    Command,
}

// Game code can listen to these:
fn error_handler(mut events: EventReader<ErrorEvent>) {
    for event in events.read() {
        log::error!("error from {:?}: {}", event.source, event.error);
        if event.recovered {
            log::info!("  (recovered)");
        }
    }
}
```

## Result Types

```rust
/// Standard engine result
pub type EngineResult<T> = Result<T, EngineError>;

/// Asset loading result
pub type AssetResult<T> = Result<Handle<T>, AssetError>;
```

## Command Error Reporting

Commands return `Result<String, String>` (already in the existing code). This is sufficient for a console-style command system. For programmatic use, commands can emit error events.

## Panic Strategy

The engine panics on:
- Window creation failure
- GPU adapter/device request failure
- Surface creation failure
- Internal invariant violations (marked with `unreachable!`)

Game code panics are caught by the ECS scheduler and reported as errors.

---

[← Back to Macros](09-macros.md) | [Next: Extensibility →](11-extensibility.md)
