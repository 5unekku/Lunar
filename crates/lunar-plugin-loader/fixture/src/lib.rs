//! test fixture: a rust behavior `cdylib` that registers one behavior with an
//! exported field, exercising the `lunar_register_behaviors` reload ABI.

use lunar_core::behavior::{Behavior, BehaviorContext, BehaviorRegistry};
use lunar_macros::Behavior;

/// a behavior with a single tunable field. the derive generates its exported-field
/// surface; the lifecycle hook just bumps the field so a reload's effect is visible.
#[derive(Behavior, Default)]
struct Mover {
	#[export]
	speed: f32,
}

impl Behavior for Mover {
	fn on_update(&mut self, _ctx: &mut BehaviorContext) {
		self.speed += 1.0;
	}
}

/// the reload entry point the loader calls. registers this crate's behaviors into
/// the live registry, overwriting any prior factories for the same ids.
///
/// # Safety
/// `registry` must be a valid pointer to the host's `BehaviorRegistry`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_register_behaviors(registry: *mut BehaviorRegistry) {
	let registry = unsafe { &mut *registry };
	registry.register("Mover", || Box::new(Mover::default()));
}
