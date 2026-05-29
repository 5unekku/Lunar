//! built-in engine types: transform, color, rect
//!
//! these are the common types used across all 2D game code.

use crate::Vec2;
use bevy_ecs::prelude::Component;

/// 2D transform component: position, rotation, scale.
///
/// this is the primary way to represent an entity's placement in the world.
/// it supports translation (x, y), rotation (radians), and scale (x, y).
/// for depth sorting, use the `Layer` component from `lunar_render`.
///
/// # builder pattern
///
/// transforms can be constructed fluently:
///
/// ```ignore
/// let transform = Transform::from_xy(100.0, 200.0)
///     .with_rotation(std::f32::consts::PI / 4.0)
///     .with_scale(Vec2::new(2.0, 2.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct Transform {
    /// x, y position
    pub translation: Vec2,
    /// rotation in radians
    pub rotation: f32,
    /// x, y scale
    pub scale: Vec2,
}

impl Transform {
    /// create a transform from a 2D translation.
    /// rotation defaults to 0, scale to (1, 1).
    #[must_use]
    pub const fn from_translation(translation: Vec2) -> Self {
        Self {
            translation,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }

    /// create a transform from x, y coordinates.
    /// shorthand for [`from_translation`](Transform::from_translation).
    #[must_use]
    pub const fn from_xy(x: f32, y: f32) -> Self {
        Self::from_translation(Vec2::new(x, y))
    }

    /// set the rotation in radians.
    /// returns self for builder-style chaining.
    #[must_use]
    pub const fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }

    /// set the scale.
    /// returns self for builder-style chaining.
    #[must_use]
    pub const fn with_scale(mut self, scale: Vec2) -> Self {
        self.scale = scale;
        self
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }
}

/// local transform: position, rotation, and scale relative to the parent entity.
///
/// when an entity has no parent, this is equivalent to world space.
/// used in entity hierarchies for parent-child transform propagation.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct LocalTransform {
    /// x, y position relative to parent
    pub translation: Vec2,
    /// rotation in radians relative to parent
    pub rotation: f32,
    /// x, y scale relative to parent
    pub scale: Vec2,
}

impl LocalTransform {
    /// create a local transform from a 2D translation.
    #[must_use]
    pub const fn from_translation(translation: Vec2) -> Self {
        Self {
            translation,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }

    /// create a local transform from x, y coordinates.
    #[must_use]
    pub const fn from_xy(x: f32, y: f32) -> Self {
        Self::from_translation(Vec2::new(x, y))
    }

    /// set the rotation in radians.
    #[must_use]
    pub const fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }

    /// set the scale.
    #[must_use]
    pub const fn with_scale(mut self, scale: Vec2) -> Self {
        self.scale = scale;
        self
    }
}

impl Default for LocalTransform {
    fn default() -> Self {
        Self {
            translation: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }
}

/// world transform: absolute position, rotation, and scale in world space.
///
/// this component is computed automatically from [`LocalTransform`] and
/// parent hierarchy. do not modify directly — use [`LocalTransform`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct WorldTransform {
    /// absolute x, y position
    pub translation: Vec2,
    /// absolute rotation in radians
    pub rotation: f32,
    /// absolute scale
    pub scale: Vec2,
}

impl WorldTransform {
    /// create a world transform at the origin.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            translation: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }

    /// create a world transform from x, y coordinates.
    #[must_use]
    pub const fn from_xy(x: f32, y: f32) -> Self {
        Self {
            translation: Vec2::new(x, y),
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }
}

impl Default for WorldTransform {
    fn default() -> Self {
        Self::new()
    }
}

/// RGBA color type.
///
/// all channels are normalized to the range 0.0 - 1.0.
/// common colors are provided as associated constants.
///
/// # example
///
/// ```ignore
/// let red = Color::rgb(1.0, 0.0, 0.0);
/// let semi_transparent = Color::rgba(1.0, 1.0, 1.0, 0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// red channel (0.0 - 1.0)
    pub r: f32,
    /// green channel (0.0 - 1.0)
    pub g: f32,
    /// blue channel (0.0 - 1.0)
    pub b: f32,
    /// alpha channel (0.0 - 1.0)
    pub a: f32,
}

impl Color {
    /// pure black (0, 0, 0, 1).
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// pure white (1, 1, 1, 1).
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// pure red (1, 0, 0, 1).
    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    /// pure green (0, 1, 0, 1).
    pub const GREEN: Self = Self::rgb(0.0, 1.0, 0.0);
    /// pure blue (0, 0, 1, 1).
    pub const BLUE: Self = Self::rgb(0.0, 0.0, 1.0);
    /// fully transparent black (0, 0, 0, 0).
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);

    /// create an RGB color with full opacity (alpha = 1.0).
    #[must_use]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// create an RGBA color with explicit alpha.
    #[must_use]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::WHITE
    }
}

/// 2D rectangle: position + size.
///
/// represents a bounding box with top-left corner at (x, y)
/// and dimensions (w, h). useful for collision detection and UI layout.
///
/// # example
///
/// ```ignore
/// let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
/// if rect.contains(mouse_pos) {
///     // clicked!
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// x coordinate of top-left corner
    pub x: f32,
    /// y coordinate of top-left corner
    pub y: f32,
    /// width
    pub w: f32,
    /// height
    pub h: f32,
}

impl Rect {
    /// create a new rectangle from top-left corner and size.
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// create a rectangle from center point and half-size extents.
    #[must_use]
    pub fn from_center(center: Vec2, half_size: Vec2) -> Self {
        Self {
            x: center.x - half_size.x,
            y: center.y - half_size.y,
            w: half_size.x * 2.0,
            h: half_size.y * 2.0,
        }
    }

    /// check if a point lies inside this rectangle.
    ///
    /// inclusive of all edges (touching counts as inside). contrast with [`intersects`](Self::intersects),
    /// which is exclusive (touching edges do not count as overlap).
    #[must_use]
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.w
            && point.y >= self.y
            && point.y <= self.y + self.h
    }

    /// check if this rectangle overlaps another.
    ///
    /// exclusive of touching edges (two rects that share only an edge do not overlap). contrast with
    /// [`contains`](Self::contains), which is inclusive.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }

    /// get the center point of this rectangle.
    #[must_use]
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// get the top-left corner position.
    #[must_use]
    pub const fn top_left(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// get the bottom-right corner position.
    #[must_use]
    pub const fn bottom_right(&self) -> Vec2 {
        Vec2::new(self.x + self.w, self.y + self.h)
    }

    /// expand or shrink the rect by the given deltas on all sides.
    pub fn inflate(&mut self, dx: f32, dy: f32) {
        self.x -= dx;
        self.y -= dy;
        self.w = dx.mul_add(2.0, self.w);
        self.h = dy.mul_add(2.0, self.h);
    }

    /// constrain this rect to lie fully within another rect.
    /// clamps both position and size so the right/bottom edges don't exceed the boundary.
    pub fn clamp(&mut self, within: &Self) {
        let x2 = (self.x + self.w).min(within.x + within.w);
        let y2 = (self.y + self.h).min(within.y + within.h);
        self.x = self.x.max(within.x);
        self.y = self.y.max(within.y);
        self.w = (x2 - self.x).max(0.0);
        self.h = (y2 - self.y).max(0.0);
    }

    /// alias for [`Rect::contains`] — point collision check.
    #[must_use]
    pub fn collide_point(&self, point: Vec2) -> bool {
        self.contains(point)
    }

    /// alias for [`Rect::intersects`] — rect collision check.
    #[must_use]
    pub fn collide_rect(&self, other: &Self) -> bool {
        self.intersects(other)
    }

    /// return the smallest rect that contains both this rect and another.
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.w).max(other.x + other.w);
        let bottom = (self.y + self.h).max(other.y + other.h);
        Self {
            x,
            y,
            w: right - x,
            h: bottom - y,
        }
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn rgb_constructs_with_full_opacity() {
        let c = Color::rgb(0.5, 0.3, 0.8);
        assert_eq!(c.r, 0.5);
        assert_eq!(c.g, 0.3);
        assert_eq!(c.b, 0.8);
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn rgba_constructs_with_explicit_alpha() {
        let c = Color::rgba(1.0, 0.0, 0.0, 0.5);
        assert_eq!(c.a, 0.5);
    }

    #[test]
    fn constants_have_expected_values() {
        assert_eq!(Color::BLACK, Color::rgb(0.0, 0.0, 0.0));
        assert_eq!(Color::WHITE, Color::rgb(1.0, 1.0, 1.0));
        assert_eq!(Color::RED, Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(Color::GREEN, Color::rgb(0.0, 1.0, 0.0));
        assert_eq!(Color::BLUE, Color::rgb(0.0, 0.0, 1.0));
        assert_eq!(Color::TRANSPARENT, Color::rgba(0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn default_is_white() {
        assert_eq!(Color::default(), Color::WHITE);
    }
}

#[cfg(test)]
mod rect_tests {
    use super::*;

    #[test]
    fn new_creates_rect() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 20.0);
        assert_eq!(r.w, 100.0);
        assert_eq!(r.h, 50.0);
    }

    #[test]
    fn from_center_derives_correct_corners() {
        let r = Rect::from_center(Vec2::new(50.0, 30.0), Vec2::new(40.0, 20.0));
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 10.0);
        assert_eq!(r.w, 80.0);
        assert_eq!(r.h, 40.0);
    }

    #[test]
    fn contains_point_inside() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(r.contains(Vec2::new(50.0, 50.0)));
        assert!(r.contains(Vec2::new(0.0, 0.0)));
        assert!(r.contains(Vec2::new(100.0, 100.0)));
    }

    #[test]
    fn contains_point_outside() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(!r.contains(Vec2::new(-1.0, 50.0)));
        assert!(!r.contains(Vec2::new(50.0, 101.0)));
    }

    #[test]
    fn intersects_overlapping() {
        let a = Rect::new(0.0, 0.0, 50.0, 50.0);
        let b = Rect::new(25.0, 25.0, 50.0, 50.0);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    #[test]
    fn intersects_non_overlapping() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn center_is_midpoint() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let c = r.center();
        assert_eq!(c.x, 60.0);
        assert_eq!(c.y, 45.0);
    }

    #[test]
    fn inflate_expands_on_all_sides() {
        let mut r = Rect::new(10.0, 10.0, 20.0, 20.0);
        r.inflate(5.0, 10.0);
        assert_eq!(r.x, 5.0);
        assert_eq!(r.y, 0.0);
        assert_eq!(r.w, 30.0);
        assert_eq!(r.h, 40.0);
    }

    #[test]
    fn clamp_constrains_within_boundary() {
        let mut r = Rect::new(-10.0, -10.0, 100.0, 100.0);
        let boundary = Rect::new(0.0, 0.0, 50.0, 50.0);
        r.clamp(&boundary);
        assert_eq!(r.x, 0.0);
        assert_eq!(r.y, 0.0);
        assert_eq!(r.w, 50.0);
        assert_eq!(r.h, 50.0);
    }

    #[test]
    fn clamp_does_not_expand() {
        let mut r = Rect::new(10.0, 10.0, 20.0, 20.0);
        let boundary = Rect::new(0.0, 0.0, 100.0, 100.0);
        r.clamp(&boundary);
        assert_eq!(r, Rect::new(10.0, 10.0, 20.0, 20.0));
    }

    #[test]
    fn collide_point_delegates_to_contains() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.collide_point(Vec2::new(5.0, 5.0)));
        assert!(!r.collide_point(Vec2::new(15.0, 5.0)));
    }

    #[test]
    fn collide_rect_delegates_to_intersects() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(a.collide_rect(&Rect::new(5.0, 5.0, 10.0, 10.0)));
        assert!(!a.collide_rect(&Rect::new(20.0, 20.0, 10.0, 10.0)));
    }

    #[test]
    fn union_encloses_both_rects() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        let u = a.union(&b);
        assert_eq!(u, Rect::new(0.0, 0.0, 30.0, 30.0));
    }

    #[test]
    fn union_with_contained_rect() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert_eq!(a.union(&b), a);
    }

    #[test]
    fn top_left_returns_corner() {
        let r = Rect::new(5.0, 10.0, 50.0, 30.0);
        assert_eq!(r.top_left(), Vec2::new(5.0, 10.0));
    }

    #[test]
    fn bottom_right_returns_corner() {
        let r = Rect::new(5.0, 10.0, 50.0, 30.0);
        assert_eq!(r.bottom_right(), Vec2::new(55.0, 40.0));
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn default_transform_is_at_origin() {
        let t = Transform::default();
        assert_eq!(t.translation, Vec2::ZERO);
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.scale, Vec2::ONE);
    }

    #[test]
    fn from_translation_sets_position() {
        let t = Transform::from_translation(Vec2::new(100.0, 200.0));
        assert_eq!(t.translation.x, 100.0);
        assert_eq!(t.translation.y, 200.0);
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.scale, Vec2::ONE);
    }

    #[test]
    fn from_xy_shorthand() {
        let t = Transform::from_xy(50.0, 75.0);
        assert_eq!(t.translation.x, 50.0);
        assert_eq!(t.translation.y, 75.0);
    }

    #[test]
    fn with_rotation_chain() {
        let t = Transform::from_xy(0.0, 0.0).with_rotation(1.5);
        assert_eq!(t.rotation, 1.5);
    }

    #[test]
    fn with_scale_chain() {
        let t = Transform::from_xy(0.0, 0.0).with_scale(Vec2::new(2.0, 3.0));
        assert_eq!(t.scale, Vec2::new(2.0, 3.0));
    }

    #[test]
    fn builder_chaining() {
        let t = Transform::from_xy(10.0, 20.0)
            .with_rotation(0.5)
            .with_scale(Vec2::splat(2.0));
        assert_eq!(t.translation.x, 10.0);
        assert_eq!(t.translation.y, 20.0);
        assert_eq!(t.rotation, 0.5);
        assert_eq!(t.scale, Vec2::splat(2.0));
    }

    #[test]
    fn local_transform_default() {
        let t = LocalTransform::default();
        assert_eq!(t.translation, Vec2::ZERO);
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.scale, Vec2::ONE);
    }

    #[test]
    fn world_transform_new_is_at_origin() {
        let t = WorldTransform::new();
        assert_eq!(t.translation, Vec2::ZERO);
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.scale, Vec2::ONE);
    }

    #[test]
    fn world_transform_from_xy() {
        let t = WorldTransform::from_xy(30.0, 40.0);
        assert_eq!(t.translation.x, 30.0);
        assert_eq!(t.translation.y, 40.0);
    }
}
