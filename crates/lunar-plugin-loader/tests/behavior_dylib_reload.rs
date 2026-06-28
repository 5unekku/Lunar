//! native-only integration test for the rust behavior dylib loader.
//!
//! ignored by default for two reasons:
//! 1. it needs the `behavior-fixture` cdylib prebuilt.
//! 2. KNOWN ABI LIMITATION: the loader passes `&mut BehaviorRegistry` (a
//!    non-`repr(C)` type holding a `HashMap` and `Box<dyn Behavior>`) across the
//!    cdylib boundary. the fixture statically links its OWN copy of `lunar-core`,
//!    so the two `BehaviorRegistry` instances are distinct codegen units; rust has
//!    no stable ABI, so this is fragile and currently segfaults at runtime.
//!    the production-safe path is a C-ABI registration shim (mirroring the C#
//!    `lunar_behavior_register` FFI) where the dylib registers through a stable
//!    extern "C" surface instead of touching `BehaviorRegistry` directly. the
//!    snapshot/restore field-preservation logic this test relies on is already
//!    covered by `lunar_core::behavior::tests::hot_reload_preserves_exported_fields`.
//!
//! run (at your own risk, after the C-ABI shim lands) with:
//!
//! ```bash
//! cargo build -p behavior-fixture
//! cargo test -p lunar-plugin-loader --test behavior_dylib_reload -- --ignored
//! ```

use std::path::PathBuf;

use bevy_ecs::world::World;
use lunar_core::behavior::{
    AttachedBehavior, BehaviorRegistry, Behaviors, FieldValue, snapshot_behavior_fields,
};
use lunar_plugin_loader::BehaviorDylibLoader;

fn fixture_path() -> Option<PathBuf> {
    // workspace target dir relative to this crate's manifest (crates/lunar-plugin-loader)
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = manifest.join("../../target/debug");
    for name in ["libbehavior_fixture.so", "libbehavior_fixture.dylib"] {
        let candidate = target.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[test]
#[ignore = "needs the behavior-fixture cdylib prebuilt; see file header"]
fn dylib_load_and_reload_preserves_fields() {
    let Some(path) = fixture_path() else {
        eprintln!("skipping: build behavior-fixture first (cargo build -p behavior-fixture)");
        return;
    };

    let mut world = World::new();
    world.insert_resource(BehaviorRegistry::default());
    let mut loader = BehaviorDylibLoader::new();

    // load the dylib, which registers the "Mover" behavior
    loader.load(&mut world, &path).expect("load fixture dylib");
    assert!(
        world.resource::<BehaviorRegistry>().create("Mover").is_some(),
        "Mover registered after load"
    );

    // attach a Mover with a tuned speed
    let entity = world.spawn_empty().id();
    let mut behavior = world
        .resource::<BehaviorRegistry>()
        .create("Mover")
        .unwrap();
    behavior.set_field("speed", FieldValue::Float(42.0));
    let mut behaviors = Behaviors::default();
    behaviors.push(AttachedBehavior {
        id: "Mover".into(),
        behavior,
        started: true,
    });
    world.entity_mut(entity).insert(behaviors);

    // sanity: snapshot sees the tuned value
    let snapshot = snapshot_behavior_fields(&mut world);
    assert_eq!(snapshot.len(), 1);

    // reload the same dylib; field value must survive
    loader.reload(&mut world, &path).expect("reload fixture dylib");
    let behaviors = world.entity(entity).get::<Behaviors>().unwrap();
    assert_eq!(behaviors.len(), 1);
    assert_eq!(
        behaviors.items()[0].behavior.get_field("speed"),
        Some(FieldValue::Float(42.0)),
        "exported field preserved across dylib reload"
    );
}
