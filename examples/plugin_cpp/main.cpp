/*
 * smoke test C plugin demonstrating the lunar C FFI layer.
 *
 * what it shows:
 *   - registering a custom component type
 *   - spawning entities with components and a 3D transform
 *   - querying entities by component and mutating them each frame
 *
 * build (shared library):
 *   cc -shared -fPIC -I../../bindings/c/include -o plugin_cpp.so main.cpp
 *
 * load from the engine:
 *   loader.load(world, "plugin_cpp.so");
 */
#include "lunar.h"
#include <cstdio>

/* user-defined component: enemy health */
struct Health {
    float max;
    float current;
};

static LunarComponentId g_health_id = LUNAR_INVALID_COMPONENT_ID;

/* buffer shared between query callback and tick() */
struct MatchBuffer {
    LunarEntity entities[64];
    uint32_t    count;
};

static void on_match(LunarEntity entity, void* user_data) {
    auto* buf = static_cast<MatchBuffer*>(user_data);
    if (buf->count < 64)
        buf->entities[buf->count++] = entity;
}

static void tick(LunarWorld* world, void* /*user_data*/) {
    float dt = lunar_delta_seconds(world);

    LunarComponentId include[] = { g_health_id };
    MatchBuffer buf = {};
    lunar_query_foreach(world, include, 1, nullptr, 0, on_match, &buf);

    for (uint32_t i = 0; i < buf.count; ++i) {
        auto* hp = static_cast<Health*>(
            lunar_component_get_mut(world, buf.entities[i], g_health_id));
        if (!hp) continue;

        hp->current -= 10.0f * dt;
        if (hp->current <= 0.0f) {
            std::printf("[plugin_cpp] entity %u died\n", buf.entities[i]);
            lunar_despawn(world, buf.entities[i]);
        }
    }
}

extern "C" void lunar_plugin_init(LunarWorld* world) {
    g_health_id = lunar_component_register(
        world, "Health", sizeof(Health), alignof(Health));

    LunarEntity enemy = lunar_spawn(world);

    Health hp = { .max = 100.0f, .current = 100.0f };
    lunar_component_insert(world, enemy, g_health_id, &hp, sizeof(hp));

    LunarTransform3d transform = {
        .translation = { .x = 0.0f, .y = 1.0f, .z = -5.0f },
        .rotation    = { .x = 0.0f, .y = 0.0f, .z = 0.0f, .w = 1.0f },
        .scale       = { .x = 1.0f, .y = 1.0f, .z = 1.0f },
    };
    lunar_set_transform3d(world, enemy, &transform);

    std::printf("[plugin_cpp] spawned enemy %u with %.0f hp\n", enemy, hp.max);

    lunar_system_register(world, LUNAR_SCHEDULE_UPDATE, tick, nullptr);
}
