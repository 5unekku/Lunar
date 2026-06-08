# lunar-plugins

opt-in functionality lives in a separate workspace:
**https://gitlab.com/5unekku/lunar-plugins**

full documentation for each plugin is in that repo. what follows is a brief
directory so you know what exists and whether it's relevant to your game.

## adding a plugin

add it alongside `lunar` in your `Cargo.toml`:

```toml
[dependencies]
lunar = { version = "1", features = ["2d"] }
lunar-plugin-physics-2d = { version = "1" }
```

## available plugins

| crate | what it does |
|-------|-------------|
| `lunar-plugin-physics-2d` | gravity, velocity integration, AABB collision response for 2d |
| `lunar-plugin-physics-3d` | kinematic character controller — move-and-slide, slope/step handling |
| `lunar-plugin-ui` | screen-space UI: panels, labels, buttons, progress bars |
| `lunar-plugin-tilemap` | tile-based level rendering (requires `2d` feature) |
| `lunar-plugin-animation` | named animation clips, frame-based sprite animator |
| `lunar-plugin-camera-3d` | spring-arm orbit camera with wall-collision shortening |
| `lunar-plugin-pathfinding-rt` | realtime A* on a uniform grid — small maps, few agents |
| `lunar-plugin-pathfinding-pre` | precomputed Dijkstra flow-field — many agents, single target (RTS/crowds) |
| `lunar-plugin-nav` | 3d navmesh baking, pathfinding, crowd avoidance |
| `lunar-plugin-particles` | 2d particle emitters |
| `lunar-plugin-dialogue` | branching dialogue trees for NPCs and cutscenes |
| `lunar-plugin-spline` | Catmull-Rom splines and `PathFollower` component |
| `lunar-plugin-timeline` | timed track sequencer for cutscenes and scripted sequences |
| `lunar-plugin-localization` | multi-language string lookup |
| `lunar-plugin-zones` | world zone management — area transitions with fade-in/out |
| `lunar-plugin-ai` | behavior tree AI — Selector, Sequence, Condition, Action nodes |
