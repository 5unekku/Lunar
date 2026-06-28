//! per-entity behaviors: logic attached to a single entity, run by the dispatcher.
//! this is the per-entity layer over the existing system-centric scripting.

use bevy_ecs::prelude::{Component, Entity, Resource, World};
use std::collections::HashMap;

/// a tunable field a behavior exposes to the editor (Godot's @export analog).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldSchema {
	pub name: String,
	pub kind: FieldKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
	Float,
	Int,
	Bool,
	Vec3,
	Color,
	Text,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
	Float(f32),
	Int(i64),
	Bool(bool),
	Vec3([f32; 3]),
	Color([f32; 4]),
	Text(String),
}

/// world access handed to a behavior during a lifecycle call. exclusive access is
/// deliberate: it gives behaviors full capability (spawn, query others, physics).
pub struct BehaviorContext<'a> {
	pub entity: Entity,
	pub world: &'a mut World,
}

/// the exported-field surface of a behavior. split from the lifecycle trait so
/// `#[derive(Behavior)]` (in lunar-macros) can generate just these three methods
/// while the author writes the lifecycle hooks in a separate `impl Behavior`.
pub trait ExportedFields {
	/// exported field schema (emitted by `#[derive(Behavior)]`).
	fn fields(&self) -> Vec<FieldSchema>;
	fn get_field(&self, name: &str) -> Option<FieldValue>;
	fn set_field(&mut self, name: &str, value: FieldValue);
}

/// a unit of logic bound to one entity. default-empty hooks mean a behavior
/// implements only what it needs. `ExportedFields` carries the tunable fields.
pub trait Behavior: ExportedFields + Send + Sync {
	fn on_ready(&mut self, _ctx: &mut BehaviorContext) {}
	fn on_update(&mut self, _ctx: &mut BehaviorContext) {}
	fn on_physics(&mut self, _ctx: &mut BehaviorContext) {}
	fn on_destroy(&mut self, _ctx: &mut BehaviorContext) {}
}

type BehaviorFactory = Box<dyn Fn() -> Box<dyn Behavior> + Send + Sync>;

/// maps a stable behavior id to a factory producing a fresh instance with defaults.
/// the project plugin populates this at build/init; the loader and hot reload use it.
#[derive(Resource, Default)]
pub struct BehaviorRegistry {
	factories: HashMap<String, BehaviorFactory>,
}

impl BehaviorRegistry {
	pub fn register(
		&mut self,
		id: impl Into<String>,
		factory: impl Fn() -> Box<dyn Behavior> + Send + Sync + 'static,
	) {
		self.factories.insert(id.into(), Box::new(factory));
	}
	pub fn create(&self, id: &str) -> Option<Box<dyn Behavior>> {
		self.factories.get(id).map(|factory| factory())
	}
	pub fn ids(&self) -> impl Iterator<Item = &str> {
		self.factories.keys().map(String::as_str)
	}
}

/// one attached behavior instance plus the registry id it was created from.
pub struct AttachedBehavior {
	pub id: String,
	pub behavior: Box<dyn Behavior>,
	/// whether on_ready has fired yet
	pub started: bool,
}

/// the per-entity behavior list. many behaviors per entity (ECS composition).
#[derive(Component, Default)]
pub struct Behaviors {
	items: Vec<AttachedBehavior>,
}

impl Behaviors {
	pub fn push(&mut self, attached: AttachedBehavior) {
		self.items.push(attached);
	}
	pub fn len(&self) -> usize {
		self.items.len()
	}
	pub fn is_empty(&self) -> bool {
		self.items.is_empty()
	}
	pub fn ids(&self) -> impl Iterator<Item = &str> {
		self.items.iter().map(|attached| attached.id.as_str())
	}
	pub fn items(&self) -> &[AttachedBehavior] {
		&self.items
	}
	pub fn items_mut(&mut self) -> &mut Vec<AttachedBehavior> {
		&mut self.items
	}
	pub fn take_items(&mut self) -> Vec<AttachedBehavior> {
		std::mem::take(&mut self.items)
	}
}

/// the lifecycle stage a dispatch pass runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BehaviorStage {
	Ready,
	Update,
	Physics,
	Destroy,
}

/// run a lifecycle stage for every entity that carries behaviors.
/// takes each entity's behavior list out so hooks get exclusive `&mut World`,
/// then restores it. on_ready fires once per behavior before its first update.
pub fn dispatch_behaviors(world: &mut World, stage: BehaviorStage) {
	// snapshot the entities to visit (cannot hold a query while mutating the world)
	let mut entities: Vec<Entity> = Vec::new();
	{
		let mut query = world.query_filtered::<Entity, bevy_ecs::prelude::With<Behaviors>>();
		for entity in query.iter(world) {
			entities.push(entity);
		}
	}

	for entity in entities {
		// take the behavior list out of the component so hooks get the whole world
		let Some(mut items) = world
			.entity_mut(entity)
			.get_mut::<Behaviors>()
			.map(|mut behaviors| behaviors.take_items())
		else {
			continue;
		};

		for attached in &mut items {
			let mut ctx = BehaviorContext { entity, world };
			match stage {
				BehaviorStage::Ready => {
					if !attached.started {
						attached.behavior.on_ready(&mut ctx);
						attached.started = true;
					}
				}
				BehaviorStage::Update => {
					if !attached.started {
						attached.behavior.on_ready(&mut ctx);
						attached.started = true;
					}
					attached.behavior.on_update(&mut ctx);
				}
				BehaviorStage::Physics => attached.behavior.on_physics(&mut ctx),
				BehaviorStage::Destroy => attached.behavior.on_destroy(&mut ctx),
			}
		}

		// put the list back (a behavior may have despawned the entity; guard it)
		if let Ok(mut entity_mut) = world.get_entity_mut(entity)
			&& let Some(mut behaviors) = entity_mut.get_mut::<Behaviors>()
		{
			*behaviors.items_mut() = items;
		}
	}
}

/// fire `on_destroy` for one entity's behaviors then despawn it. the runtime's
/// despawn routes (FFI, editor host) call this so destroy hooks always run.
pub fn despawn_with_behaviors(world: &mut World, entity: Entity) {
	if let Ok(entity_ref) = world.get_entity(entity)
		&& entity_ref.contains::<Behaviors>()
	{
		dispatch_one(world, entity, BehaviorStage::Destroy);
	}
	if let Ok(entity_mut) = world.get_entity_mut(entity) {
		entity_mut.despawn();
	}
}

/// run a lifecycle stage for a single entity (used by despawn destroy + attach ready).
fn dispatch_one(world: &mut World, entity: Entity, stage: BehaviorStage) {
	let Some(mut items) = world
		.entity_mut(entity)
		.get_mut::<Behaviors>()
		.map(|mut behaviors| behaviors.take_items())
	else {
		return;
	};
	for attached in &mut items {
		let mut ctx = BehaviorContext { entity, world };
		match stage {
			BehaviorStage::Ready => {
				if !attached.started {
					attached.behavior.on_ready(&mut ctx);
					attached.started = true;
				}
			}
			BehaviorStage::Update => attached.behavior.on_update(&mut ctx),
			BehaviorStage::Physics => attached.behavior.on_physics(&mut ctx),
			BehaviorStage::Destroy => attached.behavior.on_destroy(&mut ctx),
		}
	}
	if let Ok(mut entity_mut) = world.get_entity_mut(entity)
		&& let Some(mut behaviors) = entity_mut.get_mut::<Behaviors>()
	{
		*behaviors.items_mut() = items;
	}
}

/// data-only refs to behaviors awaiting instantiation. attached by the scene loader
/// (which only has `Commands`, no resource access); the instantiation system below
/// resolves them against the `BehaviorRegistry`.
#[derive(Component, Clone)]
pub struct PendingBehaviors(pub Vec<BehaviorRefData>);

/// one pending behavior: a registry id plus exported field overrides.
#[derive(Clone)]
pub struct BehaviorRefData {
	pub id: String,
	pub fields: Vec<(String, FieldValue)>,
}

use crate::app::{App, GamePlugin};
use crate::schedule::UpdateStage;
use bevy_ecs::prelude::{Commands, Entity as EntityParam, Query, Res};

/// resolve `PendingBehaviors` into live `Behaviors` using the registry. runs on
/// startup and early each update so both scene-loaded and runtime-spawned pending
/// refs get instantiated.
pub fn instantiate_pending_behaviors(
	mut commands: Commands,
	registry: Option<Res<BehaviorRegistry>>,
	pending: Query<(EntityParam, &PendingBehaviors)>,
) {
	let Some(registry) = registry else {
		return;
	};
	for (entity, pending) in pending.iter() {
		let mut behaviors = Behaviors::default();
		for reference in &pending.0 {
			match registry.create(&reference.id) {
				Some(mut behavior) => {
					for (name, value) in &reference.fields {
						behavior.set_field(name, value.clone());
					}
					behaviors.push(AttachedBehavior {
						id: reference.id.clone(),
						behavior,
						started: false,
					});
				}
				None => {
					log::warn!("behavior id '{}' not in registry, skipping", reference.id)
				}
			}
		}
		commands
			.entity(entity)
			.insert(behaviors)
			.remove::<PendingBehaviors>();
	}
}

/// snapshot of one behavior's id, exported field values, and started flag.
/// used by hot reload to preserve tunable state across a dylib/assembly swap.
pub struct BehaviorFieldSnapshot {
	pub id: String,
	pub fields: Vec<(String, FieldValue)>,
	pub started: bool,
}

/// all behavior snapshots for one entity.
pub struct EntityBehaviorSnapshot {
	pub entity: Entity,
	pub behaviors: Vec<BehaviorFieldSnapshot>,
}

/// capture every live behavior's exported field values, keyed by entity + id. call
/// this before a hot reload so the new instances can be restored to the same state.
pub fn snapshot_behavior_fields(world: &mut World) -> Vec<EntityBehaviorSnapshot> {
	let mut entities: Vec<Entity> = Vec::new();
	{
		let mut query = world.query_filtered::<Entity, bevy_ecs::prelude::With<Behaviors>>();
		for entity in query.iter(world) {
			entities.push(entity);
		}
	}
	let mut out = Vec::new();
	for entity in entities {
		let Some(behaviors) = world.entity(entity).get::<Behaviors>() else {
			continue;
		};
		let mut snapshots = Vec::new();
		for attached in behaviors.items() {
			let fields = attached
				.behavior
				.fields()
				.into_iter()
				.filter_map(|schema| {
					attached
						.behavior
						.get_field(&schema.name)
						.map(|value| (schema.name, value))
				})
				.collect();
			snapshots.push(BehaviorFieldSnapshot {
				id: attached.id.clone(),
				fields,
				started: attached.started,
			});
		}
		out.push(EntityBehaviorSnapshot {
			entity,
			behaviors: snapshots,
		});
	}
	out
}

/// re-create behaviors from the (freshly re-registered) `BehaviorRegistry` and restore
/// the snapshotted field values. the entity-to-id mapping survives via the snapshot, so
/// identity is preserved across the reload. behaviors whose id is no longer registered
/// are dropped with a warning.
pub fn reinstantiate_behaviors(world: &mut World, snapshot: Vec<EntityBehaviorSnapshot>) {
	if !world.contains_resource::<BehaviorRegistry>() {
		log::warn!("reinstantiate_behaviors: no BehaviorRegistry resource, skipping");
		return;
	}
	world.resource_scope(|world, registry: bevy_ecs::world::Mut<BehaviorRegistry>| {
		for entity_snapshot in snapshot {
			let mut behaviors = Behaviors::default();
			for snapshot in entity_snapshot.behaviors {
				match registry.create(&snapshot.id) {
					Some(mut behavior) => {
						for (name, value) in snapshot.fields {
							behavior.set_field(&name, value);
						}
						behaviors.push(AttachedBehavior {
							id: snapshot.id,
							behavior,
							started: snapshot.started,
						});
					}
					None => log::warn!(
						"reinstantiate_behaviors: id '{}' no longer registered, dropping",
						snapshot.id
					),
				}
			}
			if let Ok(mut entity_mut) = world.get_entity_mut(entity_snapshot.entity) {
				entity_mut.insert(behaviors);
			}
		}
	});
}

fn dispatch_update_system(world: &mut World) {
	dispatch_behaviors(world, BehaviorStage::Update);
}

fn dispatch_physics_system(world: &mut World) {
	dispatch_behaviors(world, BehaviorStage::Physics);
}

/// wires per-entity behavior dispatch into the engine schedule and inserts the
/// registry. games `add_plugin(BehaviorPlugin)` to opt in.
pub struct BehaviorPlugin;

impl GamePlugin for BehaviorPlugin {
	fn name(&self) -> &str {
		"BehaviorPlugin"
	}
	fn build(&mut self, app: &mut App) {
		app.insert_resource(BehaviorRegistry::default());
		// instantiate pending refs on startup and early each update
		app.add_startup_system(instantiate_pending_behaviors);
		app.add_system_to_stage(UpdateStage::Update, instantiate_pending_behaviors);
		// exclusive dispatch systems, one per dispatched stage
		app.add_system_to_stage(UpdateStage::Update, dispatch_update_system);
		app.add_system_to_stage(UpdateStage::Physics, dispatch_physics_system);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Resource, Default)]
	struct TickLog(u32);

	#[derive(Resource, Default)]
	struct DestroyLog(bool);

	struct Counter {
		ticks: u32,
	}
	impl ExportedFields for Counter {
		fn fields(&self) -> Vec<FieldSchema> {
			Vec::new()
		}
		fn get_field(&self, _name: &str) -> Option<FieldValue> {
			None
		}
		fn set_field(&mut self, _name: &str, _value: FieldValue) {}
	}
	impl Behavior for Counter {
		fn on_update(&mut self, ctx: &mut BehaviorContext) {
			self.ticks += 1;
			if let Some(mut log) = ctx.world.get_resource_mut::<TickLog>() {
				log.0 += 1;
			}
		}
	}

	struct Bye;
	impl ExportedFields for Bye {
		fn fields(&self) -> Vec<FieldSchema> {
			Vec::new()
		}
		fn get_field(&self, _name: &str) -> Option<FieldValue> {
			None
		}
		fn set_field(&mut self, _name: &str, _value: FieldValue) {}
	}
	impl Behavior for Bye {
		fn on_destroy(&mut self, ctx: &mut BehaviorContext) {
			if let Some(mut log) = ctx.world.get_resource_mut::<DestroyLog>() {
				log.0 = true;
			}
		}
	}

	#[test]
	fn behavior_update_runs_with_context() {
		let mut world = World::new();
		let entity = world.spawn_empty().id();
		let mut counter = Counter { ticks: 0 };
		let mut ctx = BehaviorContext {
			entity,
			world: &mut world,
		};
		counter.on_update(&mut ctx);
		counter.on_update(&mut ctx);
		assert_eq!(counter.ticks, 2);
		assert_eq!(ctx.entity, entity);
	}

	#[test]
	fn registry_creates_by_id() {
		let mut registry = BehaviorRegistry::default();
		registry.register("Counter", || Box::new(Counter { ticks: 0 }));
		let made = registry.create("Counter");
		assert!(made.is_some());
		assert!(registry.create("Missing").is_none());
	}

	#[test]
	fn behaviors_component_holds_instances() {
		let mut behaviors = Behaviors::default();
		behaviors.push(AttachedBehavior {
			id: "Counter".into(),
			behavior: Box::new(Counter { ticks: 0 }),
			started: false,
		});
		assert_eq!(behaviors.len(), 1);
		assert_eq!(behaviors.ids().collect::<Vec<_>>(), vec!["Counter"]);
	}

	#[test]
	fn dispatch_update_fires_ready_then_update() {
		let mut world = World::new();
		world.insert_resource(TickLog::default());
		let entity = world.spawn_empty().id();
		let mut behaviors = Behaviors::default();
		behaviors.push(AttachedBehavior {
			id: "Counter".into(),
			behavior: Box::new(Counter { ticks: 0 }),
			started: false,
		});
		world.entity_mut(entity).insert(behaviors);

		dispatch_behaviors(&mut world, BehaviorStage::Update);
		dispatch_behaviors(&mut world, BehaviorStage::Update);

		assert_eq!(world.resource::<TickLog>().0, 2);
		assert_eq!(world.entity(entity).get::<Behaviors>().unwrap().len(), 1);
	}

	#[test]
	fn plugin_dispatches_update_each_tick() {
		use crate::app::App;
		let mut app = App::new();
		app.insert_resource(TickLog::default());
		app.add_plugin(BehaviorPlugin);
		let entity = app.world_mut().spawn_empty().id();
		let mut behaviors = Behaviors::default();
		behaviors.push(AttachedBehavior {
			id: "Counter".into(),
			behavior: Box::new(Counter { ticks: 0 }),
			started: false,
		});
		app.world_mut().entity_mut(entity).insert(behaviors);

		app.tick(1.0 / 60.0);
		app.tick(1.0 / 60.0);
		assert_eq!(app.world_mut().get_resource::<TickLog>().unwrap().0, 2);
	}

	#[test]
	fn on_destroy_fires_when_entity_despawned() {
		use crate::app::App;
		let mut app = App::new();
		app.insert_resource(DestroyLog::default());
		app.add_plugin(BehaviorPlugin);
		let entity = app.world_mut().spawn_empty().id();
		let mut behaviors = Behaviors::default();
		behaviors.push(AttachedBehavior {
			id: "Bye".into(),
			behavior: Box::new(Bye),
			started: true,
		});
		app.world_mut().entity_mut(entity).insert(behaviors);
		despawn_with_behaviors(app.world_mut(), entity);
		assert!(app.world_mut().get_resource::<DestroyLog>().unwrap().0);
	}

	struct Tunable {
		speed: f32,
	}
	impl ExportedFields for Tunable {
		fn fields(&self) -> Vec<FieldSchema> {
			vec![FieldSchema {
				name: "speed".into(),
				kind: FieldKind::Float,
			}]
		}
		fn get_field(&self, name: &str) -> Option<FieldValue> {
			(name == "speed").then(|| FieldValue::Float(self.speed))
		}
		fn set_field(&mut self, name: &str, value: FieldValue) {
			if name == "speed"
				&& let FieldValue::Float(v) = value
			{
				self.speed = v;
			}
		}
	}
	impl Behavior for Tunable {}

	#[test]
	fn hot_reload_preserves_exported_fields() {
		let mut world = World::new();
		// v1 factory defaults speed to 0
		let mut registry = BehaviorRegistry::default();
		registry.register("Tunable", || Box::new(Tunable { speed: 0.0 }));
		world.insert_resource(registry);

		let entity = world.spawn_empty().id();
		let mut behaviors = Behaviors::default();
		behaviors.push(AttachedBehavior {
			id: "Tunable".into(),
			behavior: Box::new(Tunable { speed: 7.5 }),
			started: true,
		});
		world.entity_mut(entity).insert(behaviors);

		// snapshot, then re-register a fresh factory (the "reloaded" version)
		let snapshot = snapshot_behavior_fields(&mut world);
		world
			.resource_mut::<BehaviorRegistry>()
			.register("Tunable", || Box::new(Tunable { speed: 0.0 }));
		reinstantiate_behaviors(&mut world, snapshot);

		let behaviors = world.entity(entity).get::<Behaviors>().unwrap();
		assert_eq!(behaviors.len(), 1);
		let attached = &behaviors.items()[0];
		assert!(attached.started, "started flag preserved");
		assert_eq!(
			attached.behavior.get_field("speed"),
			Some(FieldValue::Float(7.5)),
			"exported field value preserved across reload"
		);
	}

	#[test]
	fn instantiate_pending_creates_behaviors() {
		use crate::app::App;
		let mut app = App::new();
		app.add_plugin(BehaviorPlugin);
		// first tick builds the plugin and inserts the registry resource
		app.tick(1.0 / 60.0);
		app.world_mut()
			.resource_mut::<BehaviorRegistry>()
			.register("Counter", || Box::new(Counter { ticks: 0 }));
		let entity = app.world_mut().spawn_empty().id();
		app.world_mut()
			.entity_mut(entity)
			.insert(PendingBehaviors(vec![BehaviorRefData {
				id: "Counter".into(),
				fields: Vec::new(),
			}]));
		app.tick(1.0 / 60.0);
		let world = app.world_mut();
		assert!(world.entity(entity).get::<Behaviors>().is_some());
		assert!(world.entity(entity).get::<PendingBehaviors>().is_none());
	}
}
