//! CPU lightmap baker using hemisphere ambient occlusion.
//!
//! for each lightmap texel:
//! 1. find the world-space surface point via UV2 → triangle lookup
//! 2. cast `samples` cosine-weighted rays in the hemisphere above the surface normal
//! 3. unblocked fraction × ambient_color × directional contribution → lightmap texel
//!
//! parallelised over texel rows with rayon.
//!
//! the output is a raw RGBA8 linear image that can be loaded directly into the asset
//! server and attached to a `Lightmap` component.

use rayon::prelude::*;
use lunar_math::{Mat3, Vec2, Vec3};
use lunar_3d::{IndexBuffer, MeshData};

/// directional light descriptor for lightmap baking.
#[derive(Debug, Clone, Copy)]
pub struct BakeDirectional {
    /// normalized direction the light shines FROM (i.e. pointing toward the light)
    pub direction: Vec3,
    /// linear RGB color × illuminance
    pub color: Vec3,
}

/// result of a lightmap bake: raw RGBA8 linear image data.
#[derive(Debug)]
pub struct BakeResult {
    pub width: u32,
    pub height: u32,
    /// RGBA8 linear color data, row-major, top-left origin
    pub pixels: Vec<u8>,
}

impl BakeResult {
    /// save to a PNG file (for offline workflows / editor preview).
    ///
    /// # Errors
    ///
    /// returns an error string if the file cannot be written.
    pub fn save_png(&self, path: &str) -> Result<(), String> {
        use std::io::BufWriter;
        use std::fs::File;
        let file = File::create(path).map_err(|e| format!("failed to create '{path}': {e}"))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| format!("png header error: {e}"))?;
        writer.write_image_data(&self.pixels).map_err(|e| format!("png write error: {e}"))
    }
}

/// CPU lightmap baker.
pub struct LightmapBaker {
    resolution: u32,
    samples: u32,
    directional: Option<BakeDirectional>,
    ambient: Vec3,
}

impl Default for LightmapBaker {
    fn default() -> Self {
        Self {
            resolution: 256,
            samples: 64,
            directional: None,
            ambient: Vec3::splat(0.1),
        }
    }
}

impl LightmapBaker {
    #[must_use]
    pub fn new() -> Self { Self::default() }

    #[must_use]
    pub fn with_resolution(mut self, res: u32) -> Self { self.resolution = res; self }

    #[must_use]
    pub fn with_samples(mut self, n: u32) -> Self { self.samples = n; self }

    #[must_use]
    pub fn with_directional(mut self, dir: BakeDirectional) -> Self {
        self.directional = Some(dir);
        self
    }

    #[must_use]
    pub fn with_ambient(mut self, ambient: Vec3) -> Self { self.ambient = ambient; self }

    /// bake a lightmap for a mesh.
    ///
    /// the mesh must have UV2 coords (`uv_lightmap`) in [0,1] with no overlap
    /// (standard lightmap UV layout). normals must be precomputed.
    #[must_use]
    pub fn bake(&self, mesh: &MeshData) -> BakeResult {
        let w = self.resolution;
        let h = self.resolution;

        // build triangle list with UV2 + world-space data
        let tris = build_triangles(mesh);

        let samples = self.samples;
        let ambient = self.ambient;
        let directional = self.directional;

        // parallel bake over rows
        let row_data: Vec<Vec<u8>> = (0..h).into_par_iter().map(|row| {
            let mut row_pixels = vec![0u8; w as usize * 4];
            for col in 0..w {
                let uv = Vec2::new(
                    (col as f32 + 0.5) / w as f32,
                    (row as f32 + 0.5) / h as f32,
                );
                if let Some(surface) = find_surface(&tris, uv) {
                    let color = shade_texel(
                        surface.pos, surface.normal,
                        samples, ambient, directional, &tris,
                    );
                    let idx = (col as usize) * 4;
                    row_pixels[idx]     = (color.x.clamp(0.0, 1.0) * 255.0) as u8;
                    row_pixels[idx + 1] = (color.y.clamp(0.0, 1.0) * 255.0) as u8;
                    row_pixels[idx + 2] = (color.z.clamp(0.0, 1.0) * 255.0) as u8;
                    row_pixels[idx + 3] = 255;
                } else {
                    // outside all triangles: transparent black (will be ignored in shader)
                    // leave as 0
                }
            }
            row_pixels
        }).collect();

        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for row in row_data { pixels.extend_from_slice(&row); }
        BakeResult { width: w, height: h, pixels }
    }
}

struct BakedSurface {
    pos: Vec3,
    normal: Vec3,
}

struct BakeTri {
    // uv2 coords
    uv0: Vec2,
    uv1: Vec2,
    uv2: Vec2,
    // world positions (assumed object-space here; multiply by entity transform in caller)
    p0: Vec3,
    p1: Vec3,
    p2: Vec3,
    // averaged normal
    n0: Vec3,
    n1: Vec3,
    n2: Vec3,
}

fn build_triangles(mesh: &MeshData) -> Vec<BakeTri> {
    let indices: Vec<usize> = match &mesh.indices {
        IndexBuffer::U16(v) => v.iter().map(|&i| i as usize).collect(),
        IndexBuffer::U32(v) => v.iter().map(|&i| i as usize).collect(),
    };
    let verts = &mesh.vertices;
    indices.chunks_exact(3).map(|tri| {
        let v0 = &verts[tri[0]];
        let v1 = &verts[tri[1]];
        let v2 = &verts[tri[2]];
        BakeTri {
            uv0: v0.uv_lightmap,
            uv1: v1.uv_lightmap,
            uv2: v2.uv_lightmap,
            p0: v0.position,
            p1: v1.position,
            p2: v2.position,
            n0: v0.normal,
            n1: v1.normal,
            n2: v2.normal,
        }
    }).collect()
}

/// find the surface point corresponding to a UV2 texel position.
fn find_surface(tris: &[BakeTri], uv: Vec2) -> Option<BakedSurface> {
    for tri in tris {
        if let Some((u, v)) = barycentric_uv(uv, tri.uv0, tri.uv1, tri.uv2) {
            let w = 1.0 - u - v;
            let pos = tri.p0 * w + tri.p1 * u + tri.p2 * v;
            let normal = (tri.n0 * w + tri.n1 * u + tri.n2 * v).normalize_or_zero();
            return Some(BakedSurface { pos, normal });
        }
    }
    None
}

/// compute barycentric coords (u, v) for a point inside a UV triangle.
/// returns None if outside.
fn barycentric_uv(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> Option<(f32, f32)> {
    let v0 = b - a;
    let v1 = c - a;
    let v2 = p - a;
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d11 = v1.dot(v1);
    let d20 = v2.dot(v0);
    let d21 = v2.dot(v1);
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1e-10 { return None; }
    let u = (d11 * d20 - d01 * d21) / denom;
    let v = (d00 * d21 - d01 * d20) / denom;
    let w = 1.0 - u - v;
    if u >= 0.0 && v >= 0.0 && w >= 0.0 { Some((u, v)) } else { None }
}

/// compute lighting at a surface point using hemisphere AO + directional.
fn shade_texel(
    pos: Vec3,
    normal: Vec3,
    samples: u32,
    ambient: Vec3,
    directional: Option<BakeDirectional>,
    tris: &[BakeTri],
) -> Vec3 {
    let tangent_space = build_tangent_basis(normal);

    // hemisphere AO: cast samples, count unblocked
    let mut ao_weight = 0.0f32;
    let inv_samples = 1.0 / samples as f32;
    for i in 0..samples {
        // cosine-weighted hemisphere sample using Halton sequence
        let (xi1, xi2) = halton2(i);
        let cos_theta = (1.0 - xi1).sqrt();
        let sin_theta = xi1.sqrt();
        let phi = 2.0 * std::f32::consts::PI * xi2;
        let dir_local = Vec3::new(sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta);
        let dir_world = tangent_space * dir_local;

        if !ray_blocked(pos + normal * 1e-3, dir_world, tris) {
            ao_weight += cos_theta * inv_samples;
        }
    }
    // AO weight is in [0, 1/pi * pi] = [0, 1] after cosine weighting
    // clamp to [0, 1]
    let ao = ao_weight.clamp(0.0, 1.0);

    let ambient_contrib = ambient * ao;

    // directional light contribution
    let dir_contrib = if let Some(dir) = directional {
        let ndotl = normal.dot(dir.direction).max(0.0);
        if ndotl > 0.0 && !ray_blocked(pos + normal * 1e-3, dir.direction, tris) {
            dir.color * ndotl
        } else {
            Vec3::ZERO
        }
    } else {
        Vec3::ZERO
    };

    ambient_contrib + dir_contrib
}

/// build an orthonormal basis where Z = normal (tangent space → world space matrix).
fn build_tangent_basis(n: Vec3) -> Mat3 {
    let t = if n.x.abs() > 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let bitangent = n.cross(t).normalize_or_zero();
    let tangent = bitangent.cross(n);
    Mat3::from_cols(tangent, bitangent, n)
}

/// Halton low-discrepancy sequence, base 2 and 3.
fn halton2(i: u32) -> (f32, f32) {
    let h2 = {
        let mut f = 1.0f32;
        let mut r = 0.0f32;
        let mut n = i + 1;
        while n > 0 {
            f /= 2.0;
            r += f * (n % 2) as f32;
            n /= 2;
        }
        r
    };
    let h3 = {
        let mut f = 1.0f32;
        let mut r = 0.0f32;
        let mut n = i + 1;
        while n > 0 {
            f /= 3.0;
            r += f * (n % 3) as f32;
            n /= 3;
        }
        r
    };
    (h2, h3)
}

/// simple ray-triangle intersection: returns true if any triangle blocks the ray.
fn ray_blocked(origin: Vec3, direction: Vec3, tris: &[BakeTri]) -> bool {
    const T_MAX: f32 = 100.0;
    for tri in tris {
        if let Some(t) = moller_trumbore(origin, direction, tri.p0, tri.p1, tri.p2) {
            if t > 1e-4 && t < T_MAX {
                return true;
            }
        }
    }
    false
}

/// Möller–Trumbore ray-triangle intersection test.
fn moller_trumbore(origin: Vec3, direction: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
    const EPSILON: f32 = 1e-7;
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let h = direction.cross(e2);
    let det = e1.dot(h);
    if det.abs() < EPSILON { return None; }
    let inv_det = 1.0 / det;
    let s = origin - v0;
    let u = inv_det * s.dot(h);
    if !(0.0..=1.0).contains(&u) { return None; }
    let q = s.cross(e1);
    let v = inv_det * direction.dot(q);
    if v < 0.0 || u + v > 1.0 { return None; }
    let t = inv_det * e2.dot(q);
    if t < EPSILON { return None; }
    Some(t)
}
