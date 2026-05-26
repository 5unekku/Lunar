# Contributing to Lunar

Welcome. This document gets you oriented fast: where things live, how to build
and test, what the rules are, and how to make changes that align with the
engine's direction.

## Project Direction

Lunar is a **2D game engine** written in Rust. Three priorities, in tension
but never in conflict — every change should serve all three:

1. **Maximum performance** — engine internals as fast as possible.
2. **Optimized resource usage** — moderate memory, moderate compression where
   the perf cost is negligible (e.g. zstd on the image format).
3. **Ease of use and abstraction** — game code is short, safe, and never names
   a backend dependency.

Read [`plans/design/00-overview.md`](plans/design/00-overview.md) for the
canonical design principles. Read [`plans/todo.md`](plans/todo.md) for the
active backlog and direction amendments.

## Non-Goals

- **Audio.** Owned by Moonwalker (separate project). The `AudioPlugin` slot is
  reserved in design docs but no audio crate is in this workspace.
- **3D.** Out of scope. If 3D ever exists, it will be a sister engine. Don't
  add `Vec3` translations or `Mat4` view matrices to engine-side APIs.
- **Editor.** A downstream project that will consume `lunar`. Don't add
  editor dependencies to this workspace. (`lunar-ui`, the in-game Taffy UI, is
  separate from the editor and *does* belong here.)

## Workspace Layout

```
crates/
├── lunar/         # public API facade — game code's only dependency
├── lunar-core/      # game loop, scheduler, plugins, time, scene, hierarchy
├── lunar-render/    # wgpu rendering pipeline
├── lunar-input/     # input handling
├── lunar-math/      # math types (Vec2, Mat3, Transform, Color, Rect)
├── lunar-assets/    # handle-based asset server
├── lunar-image/     # zstd-compressed image format
└── lunar-atlas/     # texture atlas packer
src/
├── main.rs           # native smoke-test binary
├── web.rs            # WASM entry point
└── bin/              # example binaries (rpg-example, etc.)
plans/
├── design/           # design documents (00-overview through 13 + appendices)
├── todo.md           # implementation backlog with phases
└── codebase-audit.md # historical audits
```

## Build & Test

### Native

```bash
cargo check --workspace        # fast type check across all crates
cargo build --workspace        # build everything
cargo test --workspace         # run all tests
cargo run                      # smoke-test binary (opens a window)
cargo run --bin rpg-example    # full example
```

### WASM

```bash
./scripts/build-web.sh         # builds wasm + dist/
go run scripts/serve.go        # serves at http://localhost:8080
```

Requires Chrome 113+ or Firefox Nightly with `dom.webgpu.enabled`.

### Cross-compile checks

```bash
cargo test --test cross_compile   # checks all targets that are installed
```

## Code Style

Follow the project's style rules consistently. The conventions below come from
the global rules baked into the codebase.

### Comments
- Lowercase, casual, succinct. Capitalize only proper names and identifiers.
- Default to writing **no comment**. Add one only when the *why* is non-obvious:
  a hidden constraint, a workaround, surprising behavior. Don't explain *what*
  the code does — well-named identifiers handle that.
- Don't reference the current task or PR ("added for issue #123") — that belongs
  in the commit message.
- `unsafe` blocks **must** have a `// SAFETY:` block above them stating the
  invariant being upheld.

### Naming
- No abbreviated names. `request` not `req`, `texture` not `tex`,
  `configuration` not `cfg` (use `cfg` only when it's an actual `cfg!` macro
  context).
- Prefix unused parameters with `_`.

### Formatting
- Functions: no space before `(`, `(` touches `{`: `fn name(x: i32) {`
- Control flow (`if`, `for`, `while`): space before/after parens, space before
  brace: `if (x > 0) {`
- One-liners when possible — break to next line + indent only when using braces.
- Never put braces on a one-liner; use expression body / arrow syntax instead.

### Documentation
- Every public item gets a doc comment. Use rustdoc syntax so `cargo doc --no-deps`
  surfaces it.
- Crate-level doc comment in every `lib.rs` explaining what the crate does.
- Examples on key public types (App, RenderQueue, InputState, AssetServer).

## API Boundary Rules

The `lunar` crate is the **only** thing game code is allowed to depend on.

- **Don't expose backend types in the public API.** `bevy_ecs`, `sdl3`, `wgpu`,
  `raw-window-handle`, `glam` (beyond `lunar-math`'s curated re-exports) — none
  of these names should appear in `lunar::prelude` or in user-facing examples.
- **Don't require `unsafe` in game code.** Game code never needs `unsafe`. The
  engine may use `unsafe` for tightly-scoped fringe optimizations (with a
  documented `// SAFETY:` block) but never forces it on consumers.
- **Use `#[doc(hidden)]` for cross-crate visibility leaks.** If a type must be
  `pub` so another internal crate can name it but isn't part of the user API,
  hide it from rustdoc.

## Crate Boundaries

Where to put new code:

| Concern | Crate |
|---------|-------|
| Math primitives (Vec2, Mat3, Color, Rect, Transform) | `lunar-math` |
| ECS scheduling, plugins, app lifecycle, time | `lunar-core` |
| GPU pipeline, shaders, draw commands | `lunar-render` |
| Keyboard, mouse, gamepad, action maps | `lunar-input` |
| Asset loading, handles, hot reload | `lunar-assets` |
| Image decoding/encoding | `lunar-image` |
| Atlas packing | `lunar-atlas` |
| Public re-exports, bootstrap, app macro | `lunar` |

If a feature crosses crate boundaries, prefer adding a thin adapter in the
consuming crate over making the producing crate aware of the consumer.

## Commits

- Lowercase, casual, succinct — same style as comments.
- One commit per distinct piece of functionality.
- **Never** add `Co-Authored-By` or any authorship attribution.
- Reference design doc sections or todo items where useful (e.g. `seal bevy_ecs
  prelude (item 67)`).

## Pull Requests

- Run `cargo check --workspace` and `cargo test --workspace` before opening.
- Update `plans/todo.md` when you complete or significantly change a tracked
  item — check it off, or split it into the actual sub-tasks you ended up
  doing.
- Update relevant design docs in `plans/design/` if your change alters the
  contract described there.
- For new public API surface, add a rustdoc example.

## When in Doubt

The holy trinity (perf, resource usage, ease of use) breaks ties. After that:
**YAGNI** — don't add features, abstractions, or error handling for cases that
can't happen. Three similar lines beats a premature abstraction.
