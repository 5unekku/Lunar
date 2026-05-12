# api_seal_test

API seal test — verifies game code can use the full ECS contract through
`lunar` alone, without naming `bevy_ecs` anywhere.

If this compiles, the seal holds. If it ever fails, the abstraction is
leaking and the fix is in the engine, not here.
