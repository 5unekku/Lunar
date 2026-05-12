# lunar

native smoke-test entry point.

`cargo run` boots the engine with no game logic — opens a window, clears
the surface, ticks the loop. Proves the bootstrap path compiles and runs.
Real games define their own `GamePlugin` and call `lunar::bootstrap`
(or use the `lunar_app!` macro).
