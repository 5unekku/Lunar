use lunar::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
#[allow(dead_code)]
pub enum Facing {
    Down,
    Up,
    Left,
    Right,
}

#[derive(Debug, Component)]
pub struct Player;

#[derive(Debug, Component)]
pub struct Npc(pub usize);
