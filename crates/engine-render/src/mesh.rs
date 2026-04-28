//! 3D mesh and light components for future compatibility.
//!
//! these types are placeholders to allow game code to define 3D entities
//! even though the 3D render pass is not yet implemented.

/// a vertex with position, normal, and UV coordinates.
///
/// used by the 3D render pass for mesh geometry.
#[derive(Clone, Copy, Debug)]
pub struct MeshVertex {
    /// position in model space (x, y, z).
    pub position: [f32; 3],
    /// surface normal for lighting calculations (x, y, z).
    pub normal: [f32; 3],
    /// texture coordinates (u, v).
    pub uv: [f32; 2],
}

/// a mesh resource containing vertex and index data.
///
/// stores geometry as a list of [`MeshVertex`] elements and
/// an index buffer for triangle definitions.
#[derive(Clone)]
pub struct Mesh {
    /// vertex data for the mesh.
    pub vertices: Vec<MeshVertex>,
    /// triangle indices (groups of 3).
    pub indices: Vec<u32>,
}

impl Mesh {
    /// create a new mesh from vertex and index data.
    pub fn new(vertices: Vec<MeshVertex>, indices: Vec<u32>) -> Self {
        Mesh { vertices, indices }
    }

    /// create a unit cube mesh centered at the origin with side length 1.
    /// generates 24 vertices (4 per face, with separate normals)
    /// and 36 indices (6 faces x 2 triangles x 3 vertices).
    pub fn unit_cube() -> Self {
        let vertices = vec![
            // front face
            MeshVertex {
                position: [-0.5, -0.5, 0.5],
                normal: [0.0, 0.0, 1.0],
                uv: [0.0, 0.0],
            },
            MeshVertex {
                position: [0.5, -0.5, 0.5],
                normal: [0.0, 0.0, 1.0],
                uv: [1.0, 0.0],
            },
            MeshVertex {
                position: [0.5, 0.5, 0.5],
                normal: [0.0, 0.0, 1.0],
                uv: [1.0, 1.0],
            },
            MeshVertex {
                position: [-0.5, 0.5, 0.5],
                normal: [0.0, 0.0, 1.0],
                uv: [0.0, 1.0],
            },
            // back face
            MeshVertex {
                position: [-0.5, -0.5, -0.5],
                normal: [0.0, 0.0, -1.0],
                uv: [0.0, 0.0],
            },
            MeshVertex {
                position: [-0.5, 0.5, -0.5],
                normal: [0.0, 0.0, -1.0],
                uv: [1.0, 0.0],
            },
            MeshVertex {
                position: [0.5, 0.5, -0.5],
                normal: [0.0, 0.0, -1.0],
                uv: [1.0, 1.0],
            },
            MeshVertex {
                position: [0.5, -0.5, -0.5],
                normal: [0.0, 0.0, -1.0],
                uv: [0.0, 1.0],
            },
            // top face
            MeshVertex {
                position: [-0.5, 0.5, -0.5],
                normal: [0.0, 1.0, 0.0],
                uv: [0.0, 0.0],
            },
            MeshVertex {
                position: [-0.5, 0.5, 0.5],
                normal: [0.0, 1.0, 0.0],
                uv: [1.0, 0.0],
            },
            MeshVertex {
                position: [0.5, 0.5, 0.5],
                normal: [0.0, 1.0, 0.0],
                uv: [1.0, 1.0],
            },
            MeshVertex {
                position: [0.5, 0.5, -0.5],
                normal: [0.0, 1.0, 0.0],
                uv: [0.0, 1.0],
            },
            // bottom face
            MeshVertex {
                position: [-0.5, -0.5, -0.5],
                normal: [0.0, -1.0, 0.0],
                uv: [0.0, 0.0],
            },
            MeshVertex {
                position: [0.5, -0.5, -0.5],
                normal: [0.0, -1.0, 0.0],
                uv: [1.0, 0.0],
            },
            MeshVertex {
                position: [0.5, -0.5, 0.5],
                normal: [0.0, -1.0, 0.0],
                uv: [1.0, 1.0],
            },
            MeshVertex {
                position: [-0.5, -0.5, 0.5],
                normal: [0.0, -1.0, 0.0],
                uv: [0.0, 1.0],
            },
            // right face
            MeshVertex {
                position: [0.5, -0.5, -0.5],
                normal: [1.0, 0.0, 0.0],
                uv: [0.0, 0.0],
            },
            MeshVertex {
                position: [0.5, 0.5, -0.5],
                normal: [1.0, 0.0, 0.0],
                uv: [1.0, 0.0],
            },
            MeshVertex {
                position: [0.5, 0.5, 0.5],
                normal: [1.0, 0.0, 0.0],
                uv: [1.0, 1.0],
            },
            MeshVertex {
                position: [0.5, -0.5, 0.5],
                normal: [1.0, 0.0, 0.0],
                uv: [0.0, 1.0],
            },
            // left face
            MeshVertex {
                position: [-0.5, -0.5, -0.5],
                normal: [-1.0, 0.0, 0.0],
                uv: [0.0, 0.0],
            },
            MeshVertex {
                position: [-0.5, -0.5, 0.5],
                normal: [-1.0, 0.0, 0.0],
                uv: [1.0, 0.0],
            },
            MeshVertex {
                position: [-0.5, 0.5, 0.5],
                normal: [-1.0, 0.0, 0.0],
                uv: [1.0, 1.0],
            },
            MeshVertex {
                position: [-0.5, 0.5, -0.5],
                normal: [-1.0, 0.0, 0.0],
                uv: [0.0, 1.0],
            },
        ];
        let indices = vec![
            0, 1, 2, 2, 3, 0, // front
            4, 5, 6, 6, 7, 4, // back
            8, 9, 10, 10, 11, 8, // top
            12, 13, 14, 14, 15, 12, // bottom
            16, 17, 18, 18, 19, 16, // right
            20, 21, 22, 22, 23, 20, // left
        ];
        Mesh::new(vertices, indices)
    }
}

/// type of light source.
///
/// determines how the light affects the scene and how it attenuates.
#[derive(Clone, Copy, Debug)]
pub enum LightType {
    /// directional light: infinite distance, parallel rays, no attenuation.
    /// useful for sunlight or moonlight.
    Directional,
    /// point light: omnidirectional, attenuates with distance.
    /// useful for lamps, explosions, etc.
    Point,
    /// spot light: cone-shaped, directional with inner/outer angle falloff.
    /// useful for flashlights, stage lights, etc.
    Spot,
}

/// a light component for 3D scenes.
///
/// stores the type, color, intensity, and geometric parameters
/// for a single light source.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    /// the type of light (directional, point, spot).
    pub light_type: LightType,
    /// RGB color of the light (0.0 - 1.0 per channel).
    pub color: [f32; 3],
    /// brightness multiplier.
    pub intensity: f32,
    /// direction vector for directional and spot lights.
    pub direction: [f32; 3],
    /// maximum reach for point and spot lights (0 = infinite for directional).
    pub range: f32,
    /// inner cone angle in radians for spot lights (full brightness inside).
    pub spot_inner: f32,
    /// outer cone angle in radians for spot lights (falloff boundary).
    pub spot_outer: f32,
}

impl Light {
    /// create a new directional light (e.g. sunlight).
    ///
    /// the direction vector points from the light toward the scene.
    pub fn directional(color: [f32; 3], intensity: f32, direction: [f32; 3]) -> Self {
        Light {
            light_type: LightType::Directional,
            color,
            intensity,
            direction,
            range: 0.0,
            spot_inner: 0.0,
            spot_outer: 0.0,
        }
    }

    /// create a new point light (omnidirectional).
    ///
    /// the range determines how far the light reaches before fading to zero.
    pub fn point(color: [f32; 3], intensity: f32, range: f32) -> Self {
        Light {
            light_type: LightType::Point,
            color,
            intensity,
            direction: [0.0, 0.0, 0.0],
            range,
            spot_inner: 0.0,
            spot_outer: 0.0,
        }
    }

    /// create a new spot light (cone-shaped beam).
    ///
    /// `spot_inner` and `spot_outer` define the cone angles in radians,
    /// controlling the sharpness of the edge falloff.
    pub fn spot(
        color: [f32; 3],
        intensity: f32,
        direction: [f32; 3],
        range: f32,
        spot_inner: f32,
        spot_outer: f32,
    ) -> Self {
        Light {
            light_type: LightType::Spot,
            color,
            intensity,
            direction,
            range,
            spot_inner,
            spot_outer,
        }
    }
}

impl Default for Light {
    fn default() -> Self {
        Light::directional([1.0, 1.0, 1.0], 1.0, [0.0, -1.0, 0.0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_cube_has_vertices_and_indices() {
        let cube = Mesh::unit_cube();
        assert_eq!(cube.vertices.len(), 24);
        assert_eq!(cube.indices.len(), 36);
    }

    #[test]
    fn light_directional_default() {
        let light = Light::default();
        assert!(matches!(light.light_type, LightType::Directional));
        assert_eq!(light.color, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn light_point_creation() {
        let light = Light::point([1.0, 0.5, 0.0], 2.0, 10.0);
        assert!(matches!(light.light_type, LightType::Point));
        assert_eq!(light.intensity, 2.0);
        assert_eq!(light.range, 10.0);
    }

    #[test]
    fn light_spot_creation() {
        let light = Light::spot([0.0, 1.0, 0.0], 1.5, [0.0, -1.0, 0.0], 20.0, 0.3, 0.5);
        assert!(matches!(light.light_type, LightType::Spot));
        assert_eq!(light.spot_inner, 0.3);
        assert_eq!(light.spot_outer, 0.5);
    }
}
