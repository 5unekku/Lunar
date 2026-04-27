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
    /// create a transform from a 2D translation
    pub fn from_translation(translation: Vec2) -> Self {
        Self {
            translation: Vec3::new(translation.x, translation.y, 0.0),
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }

    /// create a transform from x, y coordinates
    pub fn from_xy(x: f32, y: f32) -> Self {
        Self::from_translation(Vec2::new(x, y))
    }

    /// set the rotation in radians
    pub fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }

    /// set the scale
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
    /// pure black
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// pure white
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// pure red
    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    /// pure green
    pub const GREEN: Self = Self::rgb(0.0, 1.0, 0.0);
    /// pure blue
    pub const BLUE: Self = Self::rgb(0.0, 0.0, 1.0);
    /// transparent
    pub const TRANSPARENT: Self = Self::rgba(0.0, 0.0, 0.0, 0.0);

    /// create an RGB color (alpha defaults to 1.0)
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// create an RGBA color
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
    /// create a new rectangle
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// create a rectangle from center point and half-size
    pub fn from_center(center: Vec2, half_size: Vec2) -> Self {
        Self {
            x: center.x - half_size.x,
            y: center.y - half_size.y,
            w: half_size.x * 2.0,
            h: half_size.y * 2.0,
        }
    }

    /// check if a point is inside this rectangle
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.w
            && point.y >= self.y
            && point.y <= self.y + self.h
    }

    /// check if this rectangle intersects another
    pub fn intersects(&self, other: &Self) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }

    /// get the center point of this rectangle
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// get the top-left corner
    pub fn top_left(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// get the bottom-right corner
    pub fn bottom_right(&self) -> Vec2 {
        Vec2::new(self.x + self.w, self.y + self.h)
    }
}
