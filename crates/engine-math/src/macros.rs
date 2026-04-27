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
