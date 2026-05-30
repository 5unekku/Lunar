//! precomputed flow-field pathfinding via Dijkstra cost maps.
//!
//! bake a [`FlowField`] from a goal position at level load time. any number of agents
//! then query their next step in O(1) per agent by reading the flow vector at their tile.
//! ideal for many enemies all moving toward the same target (player, base, etc.).
//!
//! # usage
//!
//! ```ignore
//! use lunar_pathfinding_pre::{FlowField, FlowFieldOptions};
//! use lunar_pathfinding_rt::NavGrid;
//!
//! let grid = NavGrid::new(20, 20);
//! let flow = FlowField::bake(&grid, [10, 10], FlowFieldOptions::default());
//!
//! // each agent queries its next step
//! if let Some([dx, dy]) = flow.step_toward_goal([3, 3]) {
//!     agent.tile_x = (agent.tile_x as i32 + dx) as u32;
//!     agent.tile_y = (agent.tile_y as i32 + dy) as u32;
//! }
//! ```

use std::collections::BinaryHeap;
use std::cmp::Reverse;

use bevy_ecs::prelude::Resource;

/// heap entry that orders by cost using total_cmp so NaN sorts last.
#[derive(Clone, Copy, PartialEq)]
struct HeapEntry {
    cost: f32,
    node: u32,
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cost.total_cmp(&other.cost).then(self.node.cmp(&other.node))
    }
}

// re-export NavGrid so users only depend on this crate
pub use lunar_pathfinding_rt::NavGrid;

/// options for flow field baking.
#[derive(Default)]
pub struct FlowFieldOptions {
    /// allow diagonal movement (8-directional).
    pub diagonal: bool,
}


/// precomputed flow field for a single goal position.
///
/// bake once at level load (or whenever the nav grid changes) via [`FlowField::bake`].
/// multiple agents then call [`FlowField::step_toward_goal`] every frame at O(1) cost.
///
/// out-of-range or unreachable tiles return `None` from `step_toward_goal`.
#[derive(Resource)]
pub struct FlowField {
    width: u32,
    height: u32,
    /// per-tile Dijkstra cost from the tile to the goal. `f32::MAX` = unreachable.
    cost_map: Vec<f32>,
    /// per-tile best direction: `[dx, dy]` in `{-1, 0, 1}`. `[0, 0]` = goal or unreachable.
    flow: Vec<[i8; 2]>,
    goal: [u32; 2],
}

impl FlowField {
    /// bake a flow field for `goal` on `grid`.
    ///
    /// runs a full Dijkstra from the goal outward, computing the cheapest cost
    /// to reach the goal from every walkable tile. then derives the steepest-descent
    /// direction per tile.
    ///
    /// O(N log N) where N = width × height.
    #[must_use]
    pub fn bake(grid: &NavGrid, goal: [u32; 2], options: FlowFieldOptions) -> Self {
        let n = (grid.width * grid.height) as usize;
        let mut cost_map = vec![f32::MAX; n];
        let mut flow = vec![[0i8; 2]; n];

        if goal[0] >= grid.width || goal[1] >= grid.height {
            return Self { width: grid.width, height: grid.height, cost_map, flow, goal };
        }
        let goal_idx = pack(grid.width, goal[0], goal[1]);
        cost_map[goal_idx as usize] = 0.0;

        // Dijkstra from goal outward
        let mut heap: BinaryHeap<Reverse<HeapEntry>> = BinaryHeap::new();
        heap.push(Reverse(HeapEntry { cost: 0.0, node: goal_idx }));

        while let Some(Reverse(HeapEntry { node, .. })) = heap.pop() {
            let [nx, ny] = unpack(grid.width, node);
            let current_cost = cost_map[node as usize];

            for (neighbor, step_cost) in dijkstra_neighbors(grid, nx, ny, options.diagonal) {
                let new_cost = current_cost + step_cost * grid.tile_cost(
                    neighbor % grid.width,
                    neighbor / grid.width,
                );
                if new_cost < cost_map[neighbor as usize] {
                    cost_map[neighbor as usize] = new_cost;
                    heap.push(Reverse(HeapEntry { cost: new_cost, node: neighbor }));
                }
            }
        }

        // derive flow directions: each tile points toward its cheapest neighbor
        for idx in 0..n {
            let [x, y] = unpack(grid.width, idx as u32);
            if cost_map[idx] == f32::MAX {
                continue; // unreachable
            }
            if idx as u32 == goal_idx {
                continue; // goal tile: no direction needed
            }
            let mut best_cost = cost_map[idx];
            let mut best_dir = [0i8; 2];
            for (neighbor, _) in dijkstra_neighbors(grid, x, y, options.diagonal) {
                let neighbor_cost = cost_map[neighbor as usize];
                if neighbor_cost < best_cost {
                    best_cost = neighbor_cost;
                    let [nx, ny] = unpack(grid.width, neighbor);
                    best_dir = [(nx as i32 - x as i32) as i8, (ny as i32 - y as i32) as i8];
                }
            }
            flow[idx] = best_dir;
        }

        Self { width: grid.width, height: grid.height, cost_map, flow, goal }
    }

    /// the goal tile this flow field was baked for.
    #[must_use]
    pub fn goal(&self) -> [u32; 2] {
        self.goal
    }

    /// Dijkstra cost from tile `[x, y]` to the goal. `f32::MAX` if unreachable.
    #[must_use]
    pub fn cost_at(&self, x: u32, y: u32) -> f32 {
        self.idx(x, y).map(|i| self.cost_map[i]).unwrap_or(f32::MAX)
    }

    /// best direction to step from `[x, y]` toward the goal.
    ///
    /// returns `Some([dx, dy])` where each component is in `{-1, 0, 1}`, or
    /// `None` if the tile is out of range, unreachable, or is the goal itself.
    #[must_use]
    pub fn step_toward_goal(&self, tile: [u32; 2]) -> Option<[i32; 2]> {
        let [x, y] = tile;
        let i = self.idx(x, y)?;
        if self.cost_map[i] == f32::MAX {
            return None;
        }
        let [dx, dy] = self.flow[i];
        if dx == 0 && dy == 0 {
            return None; // already at goal
        }
        Some([dx as i32, dy as i32])
    }

    fn idx(&self, x: u32, y: u32) -> Option<usize> {
        if x < self.width && y < self.height {
            Some((y * self.width + x) as usize)
        } else {
            None
        }
    }
}

fn pack(width: u32, x: u32, y: u32) -> u32 {
    y * width + x
}

fn unpack(width: u32, node: u32) -> [u32; 2] {
    [node % width, node / width]
}

fn dijkstra_neighbors(
    grid: &NavGrid,
    x: u32,
    y: u32,
    diagonal: bool,
) -> impl Iterator<Item = (u32, f32)> {
    let width = grid.width;
    let mut buf = [(0u32, 0.0f32); 8];
    let mut len = 0usize;
    let dirs: &[(i32, i32, f32)] = if diagonal {
        &[
            (-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
            (-1, -1, std::f32::consts::SQRT_2),
            (1, -1, std::f32::consts::SQRT_2),
            (-1, 1, std::f32::consts::SQRT_2),
            (1, 1, std::f32::consts::SQRT_2),
        ]
    } else {
        &[(-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0)]
    };
    for &(dx, dy, step_cost) in dirs {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx >= 0 && ny >= 0 {
            let (nx, ny) = (nx as u32, ny as u32);
            if grid.is_walkable(nx, ny) {
                if step_cost > 1.0 {
                    let clear_x = grid.is_walkable((x as i32 + dx) as u32, y);
                    let clear_y = grid.is_walkable(x, (y as i32 + dy) as u32);
                    if !clear_x || !clear_y {
                        continue;
                    }
                }
                buf[len] = (pack(width, nx, ny), step_cost);
                len += 1;
            }
        }
    }
    buf.into_iter().take(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_leads_toward_goal_flat_grid() {
        let grid = NavGrid::new(5, 5);
        let flow = FlowField::bake(&grid, [4, 4], FlowFieldOptions::default());

        // tile [0, 0] should have a direction leading toward [4, 4]
        let step = flow.step_toward_goal([0, 0]).unwrap();
        assert!(step[0] >= 0 && step[1] >= 0, "should move toward +x/+y");
    }

    #[test]
    fn goal_tile_returns_none() {
        let grid = NavGrid::new(5, 5);
        let flow = FlowField::bake(&grid, [2, 2], FlowFieldOptions::default());
        assert!(flow.step_toward_goal([2, 2]).is_none());
    }

    #[test]
    fn unreachable_tile_returns_none() {
        let mut grid = NavGrid::new(5, 5);
        // surround goal with walls
        for x in 1..4 {
            grid.set_walkable(x, 1, false);
            grid.set_walkable(x, 3, false);
        }
        for y in 1..4 {
            grid.set_walkable(1, y, false);
            grid.set_walkable(3, y, false);
        }
        let flow = FlowField::bake(&grid, [2, 2], FlowFieldOptions::default());
        // [0, 0] is outside the wall ring, so it cannot reach the goal
        assert!(flow.step_toward_goal([0, 0]).is_none());
    }

    #[test]
    fn cost_at_goal_is_zero() {
        let grid = NavGrid::new(5, 5);
        let flow = FlowField::bake(&grid, [3, 3], FlowFieldOptions::default());
        assert_eq!(flow.cost_at(3, 3), 0.0);
    }

    #[test]
    fn cost_decreases_toward_goal() {
        let grid = NavGrid::new(10, 1);
        let flow = FlowField::bake(&grid, [9, 0], FlowFieldOptions::default());
        // cost should decrease monotonically from left to right
        let costs: Vec<f32> = (0..10).map(|x| flow.cost_at(x, 0)).collect();
        for window in costs.windows(2) {
            assert!(window[0] > window[1], "cost should decrease toward goal");
        }
    }

    #[test]
    fn many_agents_same_field() {
        let grid = NavGrid::new(20, 20);
        let flow = FlowField::bake(&grid, [19, 19], FlowFieldOptions::default());
        // simulate 100 agents all querying the same flow field
        for ax in 0..10u32 {
            for ay in 0..10u32 {
                let step = flow.step_toward_goal([ax, ay]);
                assert!(step.is_some(), "all agents should have a valid path");
            }
        }
    }
}
