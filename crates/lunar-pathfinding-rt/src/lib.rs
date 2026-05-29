//! realtime per-query A* pathfinding on a uniform-cost grid.
//!
//! call [`find_path`] whenever an agent needs a new path. results are
//! tile-coordinate sequences that game code converts to world positions.
//!
//! # usage
//!
//! ```ignore
//! use lunar_pathfinding_rt::{NavGrid, PathOptions, find_path};
//!
//! let mut grid = NavGrid::new(20, 20);
//! grid.set_walkable(5, 5, false); // place a wall
//!
//! let path = find_path(&grid, [0, 0], [10, 10], PathOptions::default());
//! if let Some(nodes) = path {
//!     for [x, y] in nodes {
//!         println!("step ({x}, {y})");
//!     }
//! }
//! ```

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use bevy_ecs::prelude::Resource;

/// uniform-cost walkability grid.
///
/// tiles are identified by `[x, y]` with x in `[0, width)` and y in `[0, height)`.
/// out-of-range tiles are implicitly unwalkable.
///
/// insert as a [`Resource`] and update it when the level changes.
#[derive(Resource)]
pub struct NavGrid {
    /// number of columns.
    pub width: u32,
    /// number of rows.
    pub height: u32,
    walkable: Vec<bool>,
    /// per-tile movement cost multiplier (default 1.0). higher = more expensive.
    cost: Vec<f32>,
}

impl NavGrid {
    /// create a fully walkable grid of size `width × height`.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let n = (width * height) as usize;
        Self {
            width,
            height,
            walkable: vec![true; n],
            cost: vec![1.0; n],
        }
    }

    /// mark a tile walkable or not.
    pub fn set_walkable(&mut self, x: u32, y: u32, walkable: bool) {
        if let Some(idx) = self.idx(x, y) {
            self.walkable[idx] = walkable;
        }
    }

    /// true if the tile is within bounds and walkable.
    #[must_use]
    pub fn is_walkable(&self, x: u32, y: u32) -> bool {
        self.idx(x, y).map(|i| self.walkable[i]).unwrap_or(false)
    }

    /// set the movement cost multiplier for a tile. must be positive.
    pub fn set_cost(&mut self, x: u32, y: u32, cost: f32) {
        if let Some(idx) = self.idx(x, y) {
            self.cost[idx] = cost.max(0.001);
        }
    }

    /// movement cost for the given tile (default 1.0).
    #[must_use]
    pub fn tile_cost(&self, x: u32, y: u32) -> f32 {
        self.idx(x, y).map(|i| self.cost[i]).unwrap_or(1.0)
    }

    fn idx(&self, x: u32, y: u32) -> Option<usize> {
        if x < self.width && y < self.height {
            Some((y * self.width + x) as usize)
        } else {
            None
        }
    }

    fn pack(&self, x: u32, y: u32) -> u32 {
        y * self.width + x
    }

    fn unpack(&self, node: u32) -> [u32; 2] {
        [node % self.width, node / self.width]
    }
}

/// options that control the pathfinding search.
pub struct PathOptions {
    /// allow diagonal movement (8-directional). if false, only cardinal (4-dir).
    pub diagonal: bool,
    /// maximum number of nodes to expand before giving up. prevents runaway searches.
    /// default 65536.
    pub max_nodes: usize,
}

impl Default for PathOptions {
    fn default() -> Self {
        Self { diagonal: false, max_nodes: 65536 }
    }
}

/// find a path from `start` to `goal` using A* on the given grid.
///
/// returns the path as a sequence of `[x, y]` tile coordinates from `start` to `goal`
/// (inclusive of both endpoints), or `None` if no path exists within `options.max_nodes`.
///
/// uses Manhattan heuristic for 4-directional movement, Chebyshev for 8-directional.
#[must_use]
pub fn find_path(
    grid: &NavGrid,
    start: [u32; 2],
    goal: [u32; 2],
    options: PathOptions,
) -> Option<Vec<[u32; 2]>> {
    if !grid.is_walkable(start[0], start[1]) || !grid.is_walkable(goal[0], goal[1]) {
        return None;
    }

    let start_node = grid.pack(start[0], start[1]);
    let goal_node = grid.pack(goal[0], goal[1]);

    if start_node == goal_node {
        return Some(vec![start]);
    }

    // open set: Reverse so smallest f-score is popped first
    // (f_score_bits, node_id)
    let mut open: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    // g_score and parent per node
    let mut g_score: HashMap<u32, f32> = HashMap::new();
    let mut parent: HashMap<u32, u32> = HashMap::new();
    let mut expanded = 0usize;

    g_score.insert(start_node, 0.0);
    let h = heuristic(start, goal, options.diagonal);
    open.push(Reverse((f32::to_bits(h), start_node)));

    while let Some(Reverse((_, current))) = open.pop() {
        if current == goal_node {
            return Some(reconstruct_path(grid, &parent, goal_node));
        }

        expanded += 1;
        if expanded > options.max_nodes {
            return None;
        }

        let current_g = *g_score.get(&current).unwrap_or(&f32::MAX);
        let [cx, cy] = grid.unpack(current);

        for (nx, ny, step_cost) in neighbors(grid, cx, cy, options.diagonal) {
            let neighbor = grid.pack(nx, ny);
            let tentative_g = current_g + step_cost * grid.tile_cost(nx, ny);
            let prev_g = *g_score.get(&neighbor).unwrap_or(&f32::MAX);
            if tentative_g < prev_g {
                g_score.insert(neighbor, tentative_g);
                parent.insert(neighbor, current);
                let f = tentative_g + heuristic([nx, ny], goal, options.diagonal);
                open.push(Reverse((f32::to_bits(f), neighbor)));
            }
        }
    }

    None
}

fn heuristic(from: [u32; 2], to: [u32; 2], diagonal: bool) -> f32 {
    let dx = from[0].abs_diff(to[0]) as f32;
    let dy = from[1].abs_diff(to[1]) as f32;
    if diagonal {
        // Chebyshev distance
        dx.max(dy)
    } else {
        // Manhattan distance
        dx + dy
    }
}

fn neighbors(grid: &NavGrid, x: u32, y: u32, diagonal: bool) -> impl Iterator<Item = (u32, u32, f32)> {
    let mut buf = [(0u32, 0u32, 0.0f32); 8];
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
    for &(dx, dy, cost) in dirs {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx >= 0 && ny >= 0 {
            let (nx, ny) = (nx as u32, ny as u32);
            if grid.is_walkable(nx, ny) {
                // for diagonal movement, also require both cardinal neighbors to be walkable
                // to prevent cutting through wall corners
                if cost > 1.0 {
                    let clear_x = grid.is_walkable((x as i32 + dx) as u32, y);
                    let clear_y = grid.is_walkable(x, (y as i32 + dy) as u32);
                    if !clear_x || !clear_y {
                        continue;
                    }
                }
                buf[len] = (nx, ny, cost);
                len += 1;
            }
        }
    }
    buf.into_iter().take(len)
}

fn reconstruct_path(grid: &NavGrid, parent: &HashMap<u32, u32>, mut current: u32) -> Vec<[u32; 2]> {
    let mut path = vec![grid.unpack(current)];
    while let Some(&prev) = parent.get(&current) {
        current = prev;
        path.push(grid.unpack(current));
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_path_no_obstacles() {
        let grid = NavGrid::new(10, 10);
        let path = find_path(&grid, [0, 0], [5, 0], PathOptions::default()).unwrap();
        assert_eq!(path.first(), Some(&[0u32, 0u32]));
        assert_eq!(path.last(), Some(&[5u32, 0u32]));
        assert_eq!(path.len(), 6);
    }

    #[test]
    fn path_around_wall() {
        let mut grid = NavGrid::new(5, 3);
        // block the whole middle column
        for y in 0..3 {
            grid.set_walkable(2, y, false);
        }
        // open a gap at y=2
        grid.set_walkable(2, 2, true);
        let path = find_path(&grid, [0, 1], [4, 1], PathOptions::default()).unwrap();
        assert_eq!(path.first(), Some(&[0u32, 1u32]));
        assert_eq!(path.last(), Some(&[4u32, 1u32]));
        // path must pass through the gap
        assert!(path.iter().any(|&[x, y]| x == 2 && y == 2));
    }

    #[test]
    fn no_path_when_blocked() {
        let mut grid = NavGrid::new(5, 5);
        for y in 0..5 {
            grid.set_walkable(2, y, false);
        }
        assert!(find_path(&grid, [0, 2], [4, 2], PathOptions::default()).is_none());
    }

    #[test]
    fn start_equals_goal() {
        let grid = NavGrid::new(5, 5);
        let path = find_path(&grid, [2, 2], [2, 2], PathOptions::default()).unwrap();
        assert_eq!(path, vec![[2u32, 2u32]]);
    }

    #[test]
    fn diagonal_movement_shorter() {
        let grid = NavGrid::new(5, 5);
        let opts_4dir = PathOptions::default();
        let opts_8dir = PathOptions { diagonal: true, ..Default::default() };
        let path_4 = find_path(&grid, [0, 0], [3, 3], opts_4dir).unwrap();
        let path_8 = find_path(&grid, [0, 0], [3, 3], opts_8dir).unwrap();
        // diagonal path should be shorter (4 vs 7 steps)
        assert!(path_8.len() < path_4.len());
    }

    #[test]
    fn high_cost_tile_avoided() {
        let mut grid = NavGrid::new(5, 1);
        grid.set_cost(2, 0, 100.0);
        // with a very high cost tile in the middle of a 1-row grid there is no
        // alternate route, so the path must go through it — but find_path should
        // still succeed
        let path = find_path(&grid, [0, 0], [4, 0], PathOptions::default());
        assert!(path.is_some());
        assert!(path.unwrap().iter().any(|&[x, _y]| x == 2));
    }

    #[test]
    fn max_nodes_limit_returns_none() {
        let grid = NavGrid::new(100, 100);
        let opts = PathOptions { diagonal: false, max_nodes: 2 };
        // path is long enough that 2 nodes won't be enough
        assert!(find_path(&grid, [0, 0], [99, 99], opts).is_none());
    }
}
