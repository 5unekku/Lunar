# Initialization Order

## Explicit Init Sequence

```
1. LogPlugin
   └── Initialize env_logger

2. Window creation (via SDL3)
   └── Create window, get handles

3. RenderPlugin
   ├── Create wgpu instance
   ├── Create surface from window
   ├── Request adapter
   ├── Request device
   └── Configure surface

4. InputPlugin
   └── Initialize SDL3 input subsystem

5. AudioPlugin (reserved slot — currently no-op)
   └── Will initialize Moonwalker (cpal-based) once that crate is integrated.
       The slot stays in the documented order so future wire-up is mechanical.

6. TimePlugin
   └── Initialize time tracking

7. Game plugins (in registration order)
   └── Register systems, resources, scenes

8. Plugin finish (in registration order)
   └── Post-build setup

9. Startup systems (in order)
   └── One-time initialization logic

10. Game loop begins
```

## Loose Coupling

Each plugin only depends on what it needs:
- `RenderPlugin` needs the window handle (provided by app builder)
- `InputPlugin` needs nothing from other plugins
- `AudioPlugin` (when reintroduced) will need nothing from other plugins
- Game plugins may depend on engine plugins (declared via `PluginDependencies`)

The app builder validates dependencies and fails fast if a dependency is missing.

---

[← Back to Dependency Graph](12-dependency-graph.md) | [Next: Complete Example →](appendix-a-complete-example.md)
