//! built-in engine types: transform, color, rect
//!
//! these are the common types used across all 2D game code.

use crate::{Vec2, Vec3};
use bevy_ecs::prelude::Component;

/// 2D transform component: position, rotation, scale.
///
/// this is the primary way to represent an entity's placement in the world.
/// it supports translation (x, y, z), rotation (radians), and scale (x, y).
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
    /// x, y position + z layer for depth sorting
    pub translation: Vec3,
    /// rotation in radians
    pub rotation: f32,
    /// x, y scale
    pub scale: Vec2,
}

impl Transform {
    /// create a transform from a 2D translation.
    /// sets z to 0.0, rotation to 0, and scale to (1, 1).
    pub fn from_translation(translation: Vec2) -> Self {
        Self {
            translation: Vec3::new(translation.x, translation.y, 0.0),
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }

    /// create a transform from x, y coordinates.
    /// shorthand for [`from_translation`](Transform::from_translation).
    pub fn from_xy(x: f32, y: f32) -> Self {
        Self::from_translation(Vec2::new(x, y))
    }

    /// set the rotation in radians.
    /// returns self for builder-style chaining.
    pub fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }

    /// set the scale.
    /// returns self for builder-style chaining.
    pub fn with_scale(mut self, scale: Vec2) -> Self {
        self.scale = scale;
        self
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
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
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// create an RGBA color with explicit alpha.
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
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// create a rectangle from center point and half-size extents.
    pub fn from_center(center: Vec2, half_size: Vec2) -> Self {
        Self {
            x: center.x - half_size.x,
            y: center.y - half_size.y,
            w: half_size.x * 2.0,
            h: half_size.y * 2.0,
        }
    }

    /// check if a point lies inside this rectangle (inclusive of edges).
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.w
            && point.y >= self.y
            && point.y <= self.y + self.h
    }

    /// check if this rectangle overlaps another (exclusive of touching edges).
    pub fn intersects(&self, other: &Self) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }

    /// get the center point of this rectangle.
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// get the top-left corner position.
    pub fn top_left(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// get the bottom-right corner position.
    pub fn bottom_right(&self) -> Vec2 {
        Vec2::new(self.x + self.w, self.y + self.h)
    }

    /// expand or shrink the rect by the given deltas on all sides.
    pub fn inflate(&mut self, dx: f32, dy: f32) {
        self.x -= dx;
        self.y -= dy;
        self.w += dx * 2.0;
        self.h += dy * 2.0;
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
    pub fn collide_point(&self, point: Vec2) -> bool {
        self.contains(point)
    }

    /// alias for [`Rect::intersects`] — rect collision check.
    pub fn collide_rect(&self, other: &Self) -> bool {
        self.intersects(other)
    }

    /// return the smallest rect that contains both this rect and another.
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
