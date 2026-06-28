//! checks `#[derive(Behavior)]` generates the exported-field surface from `#[export]`.

use lunar_core::behavior::{Behavior, ExportedFields, FieldValue};
use lunar_macros::Behavior;

#[derive(Behavior, Default)]
struct Mover {
	#[export]
	speed: f32,
	#[export]
	enabled: bool,
	// not exported, must not appear in the schema
	internal: u32,
}

// the derive only generates ExportedFields; the author writes the lifecycle impl.
impl Behavior for Mover {}

#[test]
fn derive_exposes_only_exported_fields() {
	let mut mover = Mover::default();
	let names: Vec<_> = mover.fields().into_iter().map(|field| field.name).collect();
	assert_eq!(names, vec!["speed".to_string(), "enabled".to_string()]);
	mover.set_field("speed", FieldValue::Float(3.0));
	assert_eq!(mover.get_field("speed"), Some(FieldValue::Float(3.0)));
	mover.set_field("enabled", FieldValue::Bool(true));
	assert_eq!(mover.get_field("enabled"), Some(FieldValue::Bool(true)));
	assert_eq!(mover.get_field("internal"), None);
	// internal stays at its default, untouched by set_field
	assert_eq!(mover.internal, 0);
}
