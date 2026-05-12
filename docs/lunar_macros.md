# lunar_macros

internal proc-macro crate for lunar.

Wraps the ECS derives (`Component`, `Resource`, `Event`, `Message`) so they
emit paths through `::lunar::__bevy_ecs` instead of `::bevy_ecs`. This is
the mechanism that lets game crates depend on `lunar` alone without
needing `bevy_ecs` in their `Cargo.toml`.

Game code should never name this crate directly — `lunar` re-exports the
derives at its crate root (`lunar::Component`, `lunar::Resource`, etc.)
and through `lunar::prelude`.
