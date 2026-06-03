use lunar::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Facing {
	Down,
	Up,
	Left,
	Right,
}

impl Facing {
	pub fn delta(self) -> (i32, i32) {
		match self {
			Facing::Up => (0, -1),
			Facing::Down => (0, 1),
			Facing::Left => (-1, 0),
			Facing::Right => (1, 0),
		}
	}
}

#[derive(Debug, Clone, Copy, Component)]
pub struct GridPos {
	pub col: i32,
	pub row: i32,
}

#[derive(Debug, Component)]
pub struct Player;

#[derive(Debug, Component)]
pub struct Npc(pub usize);

#[derive(Component)]
pub struct PlayerMoveAnimation {
	pub source: Vec2,
	pub target: Vec2,
	pub elapsed: f32,
}
