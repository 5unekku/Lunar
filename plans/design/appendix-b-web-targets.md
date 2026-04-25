# Web Target Considerations

- **No `std::thread`** — use async task pools (tokio for native, web-compatible async for wasm)
- **No file system access** — assets loaded via fetch/XHR, bundled at compile time
- **WebGPU surface** — uses canvas element, not window
- **SDL3 on web** — SDL3 has Emscripten support, but input may need web-specific handling
- **Conditional compilation** — `#[cfg(target_arch = "wasm32")]` gates for platform-specific code

The engine abstracts these behind plugins so game code is identical across targets.

---

[← Back to Complete Example](appendix-a-complete-example.md) | [Next: 3D Future Compatibility →](appendix-c-3d-future.md)
