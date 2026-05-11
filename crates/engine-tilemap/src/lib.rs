//! tile-based level rendering.
//!
//! game code defines a tile atlas (spritesheet) and a 2d grid of tile IDs;
//! the engine renders it efficiently with frustum culling.
//!
//! # usage
//!
//! ```ignore
//! use engine_tilemap::{TileAtlas, TileMap, TileMapPlugin};
//!
//! fn setup(mut commands: Commands, mut asset_server: ResMut<AssetServer>) {
//!     let texture = asset_server.load_texture("tileset.png");
//!     let atlas = TileAtlas::new(texture, 16, 16);
//!
//!     let mut map = TileMap::new(atlas, 20, 15, 0);
//!     map.set(0, 0, Some(0));  // tile id 0 at column 0, row 0
//!     map.set(1, 0, Some(1));  // tile id 1 at column 1, row 0
//!
//!     commands.spawn((Transform::from_xy(0.0, 0.0), map));
//! }
//! ```

use bevy_ecs::prelude::*;
use engine_assets::{Handle, Texture};
use engine_core::{App, GamePlugin};
use engine_math::{Transform, Vec2};
use engine_render::{Camera, RenderInfo, RenderQueue};

/// a tile spritesheet — uniform grid of tiles packed into a single texture.
///
/// tile ids are row-major: id 0 is the top-left tile, id 1 is one to the right, etc.
#[derive(Debug, Clone)]
pub struct TileAtlas {
    pub texture: Handle<Texture>,
    /// pixel width of each tile.
    pub tile_width: u32,
    /// pixel height of each tile.
    pub tile_height: u32,
    /// total texture width in pixels (set after texture is loaded via set_texture_size).
    texture_width: u32,
}

impl TileAtlas {
    /// create an atlas. `texture_width` is filled in when you call [`TileAtlas::set_texture_size`]
    /// after the texture is confirmed loaded; pass 0 to defer.
    #[must_use]
    pub fn new(texture: Handle<Texture>, tile_width: u32, tile_height: u32) -> Self {
        Self {
            texture,
            tile_width,
            tile_height,
            texture_width: 0,
        }
    }

    /// set the pixel width of the backing texture. required before the tilemap renders.
    /// game code calls this once the texture is ready (e.g. in a startup system after
    /// `asset_server.wait_for_all()`).
    pub fn set_texture_size(&mut self, texture_width: u32) {
        self.texture_width = texture_width;
    }

    /// compute the (top-left pixel position, pixel size) for a tile id.
    ///
    /// returns `None` if the texture width hasn't been set yet.
    #[must_use]
    pub fn source_rect(&self, tile_id: u32) -> Option<(Vec2, Vec2)> {
        if self.texture_width == 0 {
            return None;
        }
        let tiles_per_row = self.texture_width / self.tile_width;
        if tiles_per_row == 0 {
            return None;
        }
        let col = tile_id % tiles_per_row;
        let row = tile_id / tiles_per_row;
        Some((
            Vec2::new(
                (col * self.tile_width) as f32,
                (row * self.tile_height) as f32,
            ),
            Vec2::new(self.tile_width as f32, self.tile_height as f32),
        ))
    }
}

/// component that holds a 2d grid of tile ids and renders them via [`RenderQueue`].
///
/// tiles are `Option<u32>`: `None` = empty/transparent, `Some(id)` = tile from the atlas.
/// attach alongside a [`Transform`] to set the map's world-space origin (top-left corner).
#[derive(Debug, Clone, Component)]
pub struct TileMap {
    pub atlas: TileAtlas,
    /// tiles[row][col]. row 0 is the top of the map.
    tiles: Vec<Vec<Option<u32>>>,
    pub columns: usize,
    pub rows: usize,
    /// render layer (default: `layers::GAME`).
    pub layer: i32,
}

impl TileMap {
    /// create a map of `columns × rows` tiles, all initialized to `None`.
    #[must_use]
    pub fn new(atlas: TileAtlas, columns: usize, rows: usize, layer: i32) -> Self {
        Self {
            atlas,
            tiles: vec![vec![None; columns]; rows],
            columns,
            rows,
            layer,
        }
    }

    /// get the tile id at (col, row). returns `None` for out-of-bounds.
    #[must_use]
    pub fn get(&self, col: usize, row: usize) -> Option<u32> {
        self.tiles.get(row)?.get(col).copied().flatten()
    }

    /// set the tile id at (col, row). out-of-bounds is silently ignored.
    pub fn set(&mut self, col: usize, row: usize, tile_id: Option<u32>) {
        if let Some(row_vec) = self.tiles.get_mut(row) {
            if let Some(cell) = row_vec.get_mut(col) {
                *cell = tile_id;
            }
        }
    }

    /// convert a world-space position to (col, row) tile coordinates.
    /// the transform passed in is the map's origin (top-left corner).
    #[must_use]
    pub fn world_to_tile(&self, world_pos: Vec2, map_origin: Vec2) -> (i32, i32) {
        let tile_w = self.atlas.tile_width as f32;
        let tile_h = self.atlas.tile_height as f32;
        let relative = world_pos - map_origin;
        (
            (relative.x / tile_w).floor() as i32,
            (relative.y / tile_h).floor() as i32,
        )
    }

    /// convert a (col, row) tile coordinate to world-space position (top-left of that tile).
    #[must_use]
    pub fn tile_to_world(&self, col: i32, row: i32, map_origin: Vec2) -> Vec2 {
        Vec2::new(
            map_origin.x + col as f32 * self.atlas.tile_width as f32,
            map_origin.y + row as f32 * self.atlas.tile_height as f32,
        )
    }
}

/// system that renders all TileMap components via RenderQueue.
///
/// frustum-culls tiles outside the camera view so only visible tiles are drawn.
pub fn render_tilemaps(
    query: Query<(&Transform, &TileMap)>,
    camera: Option<Res<Camera>>,
    render_info: Res<RenderInfo>,
    mut render_queue: ResMut<RenderQueue>,
) {
    let tile_size = Vec2::new(1.0, 1.0); // placeholder if source_rect fails

    // compute the visible world-space rect for culling
    let (camera_pos, half_view) = if let Some(camera) = &camera {
        let zoom = camera.zoom.max(0.001);
        let viewport = camera
            .viewport
            .unwrap_or((render_info.window_width, render_info.window_height));
        let half_w = viewport.0 as f32 / (2.0 * zoom);
        let half_h = viewport.1 as f32 / (2.0 * zoom);
        (camera.position, Vec2::new(half_w, half_h))
    } else {
        let half_w = render_info.window_width as f32 / 2.0;
        let half_h = render_info.window_height as f32 / 2.0;
        (Vec2::ZERO, Vec2::new(half_w, half_h))
    };

    for (transform, tilemap) in &query {
        let origin = transform.translation;
        let tile_w = tilemap.atlas.tile_width as f32;
        let tile_h = tilemap.atlas.tile_height as f32;

        // compute tile-index range that could be visible
        let view_min = camera_pos - half_view;
        let view_max = camera_pos + half_view;

        let col_start = ((view_min.x - origin.x) / tile_w).floor() as i32;
        let col_end = ((view_max.x - origin.x) / tile_w).ceil() as i32;
        let row_start = ((view_min.y - origin.y) / tile_h).floor() as i32;
        let row_end = ((view_max.y - origin.y) / tile_h).ceil() as i32;

        let col_start = col_start.max(0) as usize;
        let col_end = (col_end as usize).min(tilemap.columns);
        let row_start = row_start.max(0) as usize;
        let row_end = (row_end as usize).min(tilemap.rows);

        for row in row_start..row_end {
            for col in col_start..col_end {
                let Some(tile_id) = tilemap.get(col, row) else {
                    continue;
                };
                let world_pos = Vec2::new(
                    origin.x + col as f32 * tile_w,
                    origin.y + row as f32 * tile_h,
                );
                let draw_size = Vec2::new(tile_w, tile_h);
                if let Some(region) = tilemap.atlas.source_rect(tile_id) {
                    render_queue.draw_sprite_atlas_on_layer(
                        &tilemap.atlas.texture,
                        world_pos,
                        draw_size,
                        region,
                        tilemap.layer,
                    );
                } else {
                    // texture size not set yet — draw a colored rect as placeholder
                    render_queue.draw_rect_on_layer(
                        world_pos,
                        tile_size,
                        engine_math::Color::WHITE,
                        tilemap.layer,
                    );
                }
            }
        }
    }
}

/// plugin that registers the tilemap render system.
pub struct TileMapPlugin;

impl GamePlugin for TileMapPlugin {
    fn name(&self) -> &'static str {
        "tilemap"
    }

    fn build(&mut self, app: &mut App) {
        app.add_system_to_stage(engine_core::UpdateStage::Render, render_tilemaps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_assets::Handle;

    fn dummy_handle() -> Handle<Texture> {
        Handle::new(0, 0)
    }

    fn make_atlas(texture_width: u32) -> TileAtlas {
        let mut atlas = TileAtlas::new(dummy_handle(), 16, 16);
        atlas.set_texture_size(texture_width);
        atlas
    }

    #[test]
    fn source_rect_row_major() {
        let atlas = make_atlas(64); // 4 tiles per row
        let (pos, size) = atlas.source_rect(5).unwrap();
        // tile 5: row 1, col 1 → x=16, y=16
        assert_eq!(pos, Vec2::new(16.0, 16.0));
        assert_eq!(size, Vec2::new(16.0, 16.0));
    }

    #[test]
    fn source_rect_first_tile() {
        let atlas = make_atlas(64);
        let (pos, size) = atlas.source_rect(0).unwrap();
        assert_eq!(pos, Vec2::ZERO);
        assert_eq!(size, Vec2::new(16.0, 16.0));
    }

    #[test]
    fn source_rect_none_when_no_texture_size() {
        let atlas = TileAtlas::new(dummy_handle(), 16, 16); // texture_width = 0
        assert!(atlas.source_rect(0).is_none());
    }

    #[test]
    fn tilemap_get_set() {
        let atlas = make_atlas(64);
        let mut map = TileMap::new(atlas, 10, 10, 0);
        map.set(3, 4, Some(7));
        assert_eq!(map.get(3, 4), Some(7));
        assert_eq!(map.get(0, 0), None);
    }

    #[test]
    fn tilemap_out_of_bounds_ignored() {
        let atlas = make_atlas(64);
        let mut map = TileMap::new(atlas, 5, 5, 0);
        map.set(99, 99, Some(0)); // should not panic
        assert_eq!(map.get(99, 99), None);
    }

    #[test]
    fn world_to_tile_and_back() {
        let atlas = make_atlas(64);
        let map = TileMap::new(atlas, 10, 10, 0);
        let origin = Vec2::new(100.0, 200.0);
        let (col, row) = map.world_to_tile(Vec2::new(132.0, 216.0), origin);
        assert_eq!(col, 2);
        assert_eq!(row, 1);
        let world = map.tile_to_world(col, row, origin);
        assert_eq!(world, Vec2::new(132.0, 216.0));
    }
}
