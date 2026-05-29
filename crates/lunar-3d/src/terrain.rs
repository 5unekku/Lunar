use bevy_ecs::prelude::*;
use lunar_math::Color;

/// heightmap terrain rendered with geometry clipmaps (Losasso/Hoppe 2004).
///
/// attach to an entity. position the entity to set the world-space origin of
/// the terrain. the heightmap covers [0, heightmap_size_world] on X and Z.
/// world Y = sample * height_scale.
///
/// on mid+ tier: up to `clipmap_rings` nested LOD rings around the camera,
/// each 2× coarser than the previous. cracks between rings are hidden by a
/// one-row skirt. on low tier: single full-res patch with distance fade.
#[derive(Component, Clone)]
pub struct Terrain {
    /// R16Float heightmap stored row-major, width × height samples.
    pub heightmap: Vec<u8>,
    /// width of the heightmap in samples.
    pub heightmap_width: u32,
    /// height of the heightmap in samples.
    pub heightmap_height: u32,
    /// world-space size of the entire heightmap on XZ.
    pub world_size: f32,
    /// maps normalised [0,1] height samples to world-space Y units.
    pub height_scale: f32,
    /// number of clipmap LOD rings (1 = no LOD, just center patch; max 8).
    pub clipmap_rings: u32,
    /// number of quads along one side of a clipmap ring segment.
    pub ring_resolution: u32,
    /// terrain tint colour (multiplied with the heightmap-derived surface colour).
    pub tint: Color,
    /// if true the heightmap texture needs to be re-uploaded to the GPU.
    pub dirty: bool,
}

impl Default for Terrain {
    fn default() -> Self {
        Self {
            heightmap: Vec::new(),
            heightmap_width: 0,
            heightmap_height: 0,
            world_size: 1024.0,
            height_scale: 64.0,
            clipmap_rings: 5,
            ring_resolution: 32,
            tint: Color::rgba(0.55, 0.5, 0.4, 1.0),
            dirty: true,
        }
    }
}
