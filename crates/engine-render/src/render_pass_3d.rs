//! 3D render pass for future compatibility.
//!
//! this module provides the scaffolding for a 3D render pass
//! that will run alongside the existing 2D render pass.
//! the actual 3D rendering pipeline is not yet implemented.

use wgpu::util::DeviceExt;

use crate::mesh::{Light, LightType, Mesh};

/// uniform data for the 3D camera.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Camera3DUniform {
    pub view_matrix: [f32; 16],
    pub projection_matrix: [f32; 16],
    pub camera_position: [f32; 3],
    pub _padding: f32,
}

/// uniform data for a 3D light.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightUniform {
    pub light_type: u32,
    pub color: [f32; 3],
    pub intensity: f32,
    pub direction: [f32; 3],
    pub position: [f32; 3],
    pub range: f32,
    pub spot_angles: [f32; 2],
}

/// a 3D render pass that can be executed alongside the 2D pass.
pub struct RenderPass3D {
    pub pipeline: Option<wgpu::RenderPipeline>,
    pub camera_buffer: wgpu::Buffer,
    pub light_buffer: wgpu::Buffer,
    pub max_lights: usize,
}

impl RenderPass3D {
    /// create a new 3D render pass.
    pub fn new(
        device: &wgpu::Device,
        _config: &wgpu::SurfaceConfiguration,
        max_lights: usize,
    ) -> Self {
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3d camera buffer"),
            contents: bytemuck::cast_slice(&[Camera3DUniform {
                view_matrix: [0.0; 16],
                projection_matrix: [0.0; 16],
                camera_position: [0.0; 3],
                _padding: 0.0,
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let light_uniform_size = (std::mem::size_of::<LightUniform>() * max_lights) as u64;
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("3d light buffer"),
            contents: vec![0u8; light_uniform_size as usize].leak(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        // pipeline is None until 3D shaders are implemented
        RenderPass3D {
            pipeline: None,
            camera_buffer,
            light_buffer,
            max_lights,
        }
    }

    /// update the camera uniform buffer.
    pub fn update_camera(&self, queue: &wgpu::Queue, camera: &Camera3DUniform) {
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[*camera]));
    }

    /// update the light uniform buffer with an array of lights.
    pub fn update_lights(&self, queue: &wgpu::Queue, lights: &[Light]) {
        let light_uniforms: Vec<LightUniform> = lights
            .iter()
            .take(self.max_lights)
            .map(|light| LightUniform {
                light_type: match light.light_type {
                    LightType::Directional => 0,
                    LightType::Point => 1,
                    LightType::Spot => 2,
                },
                color: light.color,
                intensity: light.intensity,
                direction: light.direction,
                position: [0.0; 3],
                range: light.range,
                spot_angles: [light.spot_inner, light.spot_outer],
            })
            .collect();

        let bytes = bytemuck::cast_slice(&light_uniforms);
        queue.write_buffer(&self.light_buffer, 0, bytes);
    }

    /// execute the 3D render pass.
    /// this is a stub — it clears the depth buffer but does not render meshes yet.
    pub fn execute(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _view: &wgpu::TextureView,
        _meshes: &[(&Mesh, [f32; 16])],
    ) {
        // stub: 3D rendering not yet implemented
        // when ready, this will:
        // 1. bind the 3D pipeline
        // 2. set camera/light uniforms
        // 3. iterate meshes and draw with their world matrices
    }
}

/// create a perspective projection matrix.
pub fn perspective_projection(fov: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov / 2.0).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        (far + near) * nf,
        -1.0,
        0.0,
        0.0,
        2.0 * far * near * nf,
        0.0,
    ]
}

/// create a look-at view matrix.
pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let zx = eye[0] - target[0];
    let zy = eye[1] - target[1];
    let zz = eye[2] - target[2];
    let len = (zx * zx + zy * zy + zz * zz).sqrt();
    let z = [zx / len, zy / len, zz / len];

    let xx = up[1] * z[2] - up[2] * z[1];
    let xy = up[2] * z[0] - up[0] * z[2];
    let xz = up[0] * z[1] - up[1] * z[0];
    let len2 = (xx * xx + xy * xy + xz * xz).sqrt();
    let x = [xx / len2, xy / len2, xz / len2];

    let y = [
        z[1] * x[2] - z[2] * x[1],
        z[2] * x[0] - z[0] * x[2],
        z[0] * x[1] - z[1] * x[0],
    ];

    [
        x[0],
        y[0],
        z[0],
        0.0,
        x[1],
        y[1],
        z[1],
        0.0,
        x[2],
        y[2],
        z[2],
        0.0,
        -(x[0] * eye[0] + x[1] * eye[1] + x[2] * eye[2]),
        -(y[0] * eye[0] + y[1] * eye[1] + y[2] * eye[2]),
        -(z[0] * eye[0] + z[1] * eye[1] + z[2] * eye[2]),
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_projection_returns_16_elements() {
        let m = perspective_projection(1.0, 1.0, 0.1, 100.0);
        assert_eq!(m.len(), 16);
    }

    #[test]
    fn look_at_returns_16_elements() {
        let m = look_at([0.0, 0.0, 5.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(m.len(), 16);
    }

    #[test]
    fn light_uniform_size() {
        // 56 bytes due to alignment: u32 + [f32;3] + f32 + [f32;3] + [f32;3] + f32 + [f32;2]
        assert_eq!(std::mem::size_of::<LightUniform>(), 56);
    }

    #[test]
    fn camera_uniform_size() {
        assert_eq!(std::mem::size_of::<Camera3DUniform>(), 144);
    }
}
