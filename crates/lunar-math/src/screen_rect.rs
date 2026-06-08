/// integer pixel-space rectangle, used for scissor regions and screen-space bounds.
///
/// origin is top-left. x1 < x2, y1 < y2. the rect is inclusive on all edges.
///
/// # usage
///
/// ```ignore
/// let screen = ScreenRect::full(window_w, window_h);
/// let clipped = screen.intersect(ScreenRect::new(10, 10, 200, 150));
/// if !clipped.is_empty() {
///     // region is on screen
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
	pub x1: i16,
	pub y1: i16,
	pub x2: i16,
	pub y2: i16,
}

impl ScreenRect {
	/// an empty rect that contains no pixels.
	pub const EMPTY: Self = Self {
		x1: i16::MAX,
		y1: i16::MAX,
		x2: i16::MIN,
		y2: i16::MIN,
	};

	/// the full screen at the given dimensions.
	#[must_use]
	pub const fn full(width: u16, height: u16) -> Self {
		Self {
			x1: 0,
			y1: 0,
			x2: width as i16 - 1,
			y2: height as i16 - 1,
		}
	}

	/// rect from explicit (inclusive) corner coordinates.
	#[must_use]
	pub const fn new(x1: i16, y1: i16, x2: i16, y2: i16) -> Self {
		Self { x1, y1, x2, y2 }
	}

	/// true if the rect has no area.
	#[must_use]
	pub const fn is_empty(self) -> bool {
		self.x1 > self.x2 || self.y1 > self.y2
	}

	/// expand the rect to include a screen-space pixel point.
	pub fn add_point(&mut self, x: i16, y: i16) {
		if x < self.x1 {
			self.x1 = x;
		}
		if x > self.x2 {
			self.x2 = x;
		}
		if y < self.y1 {
			self.y1 = y;
		}
		if y > self.y2 {
			self.y2 = y;
		}
	}

	/// smallest rect containing both self and other.
	#[must_use]
	pub const fn union(self, other: Self) -> Self {
		Self {
			x1: if self.x1 < other.x1 {
				self.x1
			} else {
				other.x1
			},
			y1: if self.y1 < other.y1 {
				self.y1
			} else {
				other.y1
			},
			x2: if self.x2 > other.x2 {
				self.x2
			} else {
				other.x2
			},
			y2: if self.y2 > other.y2 {
				self.y2
			} else {
				other.y2
			},
		}
	}

	/// largest rect contained within both self and other (intersection).
	#[must_use]
	pub const fn intersect(self, other: Self) -> Self {
		Self {
			x1: if self.x1 > other.x1 {
				self.x1
			} else {
				other.x1
			},
			y1: if self.y1 > other.y1 {
				self.y1
			} else {
				other.y1
			},
			x2: if self.x2 < other.x2 {
				self.x2
			} else {
				other.x2
			},
			y2: if self.y2 < other.y2 {
				self.y2
			} else {
				other.y2
			},
		}
	}

	/// width in pixels (0 if empty).
	#[must_use]
	pub const fn width(self) -> u16 {
		if self.x2 >= self.x1 {
			(self.x2 - self.x1 + 1) as u16
		} else {
			0
		}
	}

	/// height in pixels (0 if empty).
	#[must_use]
	pub const fn height(self) -> u16 {
		if self.y2 >= self.y1 {
			(self.y2 - self.y1 + 1) as u16
		} else {
			0
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn empty_has_no_area() {
		assert!(ScreenRect::EMPTY.is_empty());
	}

	#[test]
	fn full_covers_screen() {
		let r = ScreenRect::full(1920, 1080);
		assert_eq!(r.width(), 1920);
		assert_eq!(r.height(), 1080);
		assert!(!r.is_empty());
	}

	#[test]
	fn add_point_expands() {
		let mut r = ScreenRect::EMPTY;
		r.add_point(10, 20);
		r.add_point(50, 80);
		assert_eq!(r.x1, 10);
		assert_eq!(r.y1, 20);
		assert_eq!(r.x2, 50);
		assert_eq!(r.y2, 80);
	}

	#[test]
	fn union_encloses_both() {
		let a = ScreenRect::new(0, 0, 10, 10);
		let b = ScreenRect::new(5, 5, 20, 20);
		let u = a.union(b);
		assert_eq!(u, ScreenRect::new(0, 0, 20, 20));
	}

	#[test]
	fn intersect_clips_to_overlap() {
		let a = ScreenRect::new(0, 0, 20, 20);
		let b = ScreenRect::new(10, 10, 30, 30);
		let i = a.intersect(b);
		assert_eq!(i, ScreenRect::new(10, 10, 20, 20));
	}

	#[test]
	fn intersect_non_overlapping_is_empty() {
		let a = ScreenRect::new(0, 0, 5, 5);
		let b = ScreenRect::new(10, 10, 20, 20);
		assert!(a.intersect(b).is_empty());
	}
}
