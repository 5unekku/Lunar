//! convenience macros for common math types.

/// create a `Transform` from components.
///
/// # example
///
/// ```ignore
/// use lunar_math::{transform, Vec2};
///
/// let t = transform!(pos: Vec2::new(10.0, 20.0), rot: 0.5, scale: Vec2::ONE);
/// ```
#[macro_export]
macro_rules! transform {
	(pos: $pos:expr, rot: $rot:expr, scale: $scale:expr) => {
		$crate::Transform {
			translation: $crate::Vec2::new($pos.x, $pos.y),
			rotation: $rot,
			scale: $scale,
		}
	};
	(x: $x:expr, y: $y:expr) => {
		$crate::Transform {
			translation: $crate::Vec2::new($x, $y),
			rotation: 0.0,
			scale: $crate::Vec2::ONE,
		}
	};
	(pos: $pos:expr) => {
		$crate::Transform {
			translation: $crate::Vec2::new($pos.x, $pos.y),
			rotation: 0.0,
			scale: $crate::Vec2::ONE,
		}
	};
}

/// create a `Color` from components.
///
/// # example
///
/// ```ignore
/// use lunar_math::color;
///
/// let c = color!(r: 1.0, g: 0.0, b: 0.0);
/// let d = color!(r: 1.0, g: 0.0, b: 0.0, a: 0.5);
/// ```
#[macro_export]
macro_rules! color {
	(r: $r:expr, g: $g:expr, b: $b:expr) => {
		$crate::Color {
			r: $r,
			g: $g,
			b: $b,
			a: 1.0,
		}
	};
	(r: $r:expr, g: $g:expr, b: $b:expr, a: $a:expr) => {
		$crate::Color {
			r: $r,
			g: $g,
			b: $b,
			a: $a,
		}
	};
	(hex: $hex:expr) => {{
		let hex = $hex;
		let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
		let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
		let b = (hex & 0xFF) as f32 / 255.0;
		$crate::Color { r, g, b, a: 1.0 }
	}};
}

/// create a `Rect` from components.
///
/// # example
///
/// ```ignore
/// use lunar_math::rect;
///
/// let r = rect!(x: 0, y: 0, w: 100, h: 50);
/// ```
#[macro_export]
macro_rules! rect {
	(x: $x:expr, y: $y:expr, w: $w:expr, h: $h:expr) => {
		$crate::Rect {
			x: $x,
			y: $y,
			w: $w,
			h: $h,
		}
	};
	(pos: $pos:expr, size: $size:expr) => {
		$crate::Rect {
			x: $pos.x,
			y: $pos.y,
			w: $size.x,
			h: $size.y,
		}
	};
}

/// convenience wrapper for creating ecs query types.
///
/// this macro simplifies common query patterns by wrapping `bevy_ecs` query filters
/// into a single expression. it is designed to be used in system function signatures.
///
/// # example
///
/// ```ignore
/// use lunar_math::query;
/// use bevy_ecs::prelude::Query;
///
/// // query for entities with Position and Velocity
/// fn my_system(query: query!(Position, Velocity)) {
///     for (pos, vel) in query.iter() {
///         // ...
///     }
/// }
///
/// // query with filters
/// fn filtered(query: query!(Position, with: Player, without: Dead, changed: Velocity)) {
///     // ...
/// }
/// ```
#[macro_export]
macro_rules! query {
    // query!(A, B)
    ($($component:ty),+ $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+)>
    };
    // query!(A, B, with: C)
    ($($component:ty),+, with: $with:ty $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+), bevy_ecs::prelude::With<$with>>
    };
    // query!(A, B, without: C)
    ($($component:ty),+, without: $without:ty $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+), bevy_ecs::prelude::Without<$without>>
    };
    // query!(A, B, changed: C)
    ($($component:ty),+, changed: $changed:ty $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+), bevy_ecs::prelude::Changed<$changed>>
    };
    // query!(A, B, with: C, without: D)
    ($($component:ty),+, with: $with:ty, without: $without:ty $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+), (bevy_ecs::prelude::With<$with>, bevy_ecs::prelude::Without<$without>)>
    };
    // query!(A, B, with: C, changed: D)
    ($($component:ty),+, with: $with:ty, changed: $changed:ty $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+), (bevy_ecs::prelude::With<$with>, bevy_ecs::prelude::Changed<$changed>)>
    };
    // query!(A, B, without: C, changed: D)
    ($($component:ty),+, without: $without:ty, changed: $changed:ty $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+), (bevy_ecs::prelude::Without<$without>, bevy_ecs::prelude::Changed<$changed>)>
    };
    // query!(A, B, with: C, without: D, changed: E)
    ($($component:ty),+, with: $with:ty, without: $without:ty, changed: $changed:ty $(,)?) => {
        bevy_ecs::prelude::Query<($(& $component),+), (bevy_ecs::prelude::With<$with>, bevy_ecs::prelude::Without<$without>, bevy_ecs::prelude::Changed<$changed>)>
    };
}

#[cfg(test)]
mod macro_tests {
	#[test]
	fn transform_macro_full() {
		let t = transform!(pos: crate::Vec2::new(1.0, 2.0), rot: 3.0, scale: crate::Vec2::new(4.0, 5.0));
		assert_eq!(t.translation.x, 1.0);
		assert_eq!(t.translation.y, 2.0);
		assert_eq!(t.rotation, 3.0);
		assert_eq!(t.scale.x, 4.0);
		assert_eq!(t.scale.y, 5.0);
	}

	#[test]
	fn transform_macro_xy() {
		let t = transform!(x: 10.0, y: 20.0);
		assert_eq!(t.translation.x, 10.0);
		assert_eq!(t.translation.y, 20.0);
		assert_eq!(t.rotation, 0.0);
		assert_eq!(t.scale, crate::Vec2::ONE);
	}

	#[test]
	fn transform_macro_pos_only() {
		let t = transform!(pos: crate::Vec2::new(5.0, 6.0));
		assert_eq!(t.translation.x, 5.0);
		assert_eq!(t.rotation, 0.0);
	}

	#[test]
	fn color_macro_rgb() {
		let c = color!(r: 1.0, g: 0.5, b: 0.0);
		assert_eq!(c.r, 1.0);
		assert_eq!(c.g, 0.5);
		assert_eq!(c.b, 0.0);
		assert_eq!(c.a, 1.0);
	}

	#[test]
	fn color_macro_rgba() {
		let c = color!(r: 0.5, g: 0.5, b: 0.5, a: 0.25);
		assert_eq!(c.a, 0.25);
	}

	#[test]
	fn color_macro_hex() {
		let c = color!(hex: 0xFF8800);
		assert!((c.r - 1.0).abs() < 0.001);
		assert!((c.g - 0.533).abs() < 0.001);
		assert!((c.b - 0.0).abs() < 0.001);
	}

	#[test]
	fn rect_macro_xywh() {
		let r = rect!(x: 0.0, y: 10.0, w: 100.0, h: 50.0);
		assert_eq!(r.x, 0.0);
		assert_eq!(r.y, 10.0);
		assert_eq!(r.w, 100.0);
		assert_eq!(r.h, 50.0);
	}

	#[test]
	fn rect_macro_pos_size() {
		let r = rect!(pos: crate::Vec2::new(5.0, 5.0), size: crate::Vec2::new(10.0, 20.0));
		assert_eq!(r.x, 5.0);
		assert_eq!(r.y, 5.0);
		assert_eq!(r.w, 10.0);
		assert_eq!(r.h, 20.0);
	}
}
