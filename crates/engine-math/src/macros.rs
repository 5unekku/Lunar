//! convenience macros for common math types.

/// create a [`Transform`] from components.
///
/// # example
///
/// ```ignore
/// use engine_math::{transform, Vec2};
///
/// let t = transform!(pos: Vec2::new(10.0, 20.0), rot: 0.5, scale: Vec2::ONE);
/// ```
#[macro_export]
macro_rules! transform {
    (pos: $pos:expr, rot: $rot:expr, scale: $scale:expr) => {
        $crate::Transform {
            translation: $pos,
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
            translation: $pos,
            rotation: 0.0,
            scale: $crate::Vec2::ONE,
        }
    };
}

/// create a [`Color`] from components.
///
/// # example
///
/// ```ignore
/// use engine_math::color;
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

/// create a [`Rect`] from components.
///
/// # example
///
/// ```ignore
/// use engine_math::rect;
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
/// this macro simplifies common query patterns by wrapping bevy_ecs query filters
/// into a single expression. it is designed to be used in system function signatures.
///
/// # example
///
/// ```ignore
/// use engine_math::query;
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
