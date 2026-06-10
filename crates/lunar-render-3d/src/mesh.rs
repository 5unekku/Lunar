//! `RenderEngine3d` — mesh upload, forsyth optimization, terrain/heightmap/clipmap, uniform packing.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

impl RenderEngine3d {
	pub(crate) fn upload_mesh_data(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		data: &MeshData,
	) -> GpuMesh {
		let qn = |f: f32| -> i8 { (f * 127.0).round().clamp(-127.0, 127.0) as i8 };
		let qu = |f: f32| -> u16 { (f.clamp(0.0, 1.0) * 65535.0).round() as u16 };
		#[cfg(not(target_arch = "wasm32"))]
		let gpu_verts: Vec<GpuVertex3d> = {
			use rayon::prelude::*;
			data.vertices
				.par_iter()
				.map(|v| GpuVertex3d {
					position: [v.position.x, v.position.y, v.position.z],
					normal: [qn(v.normal.x), qn(v.normal.y), qn(v.normal.z), 0],
					tangent: [
						qn(v.tangent[0]),
						qn(v.tangent[1]),
						qn(v.tangent[2]),
						qn(v.tangent[3]),
					],
					uv: [v.uv.x, v.uv.y],
					uv_lightmap: [qu(v.uv_lightmap.x), qu(v.uv_lightmap.y)],
					color: v.color,
				})
				.collect()
		};
		#[cfg(target_arch = "wasm32")]
		let gpu_verts: Vec<GpuVertex3d> = data
			.vertices
			.iter()
			.map(|v| GpuVertex3d {
				position: [v.position.x, v.position.y, v.position.z],
				normal: [qn(v.normal.x), qn(v.normal.y), qn(v.normal.z), 0],
				tangent: [
					qn(v.tangent[0]),
					qn(v.tangent[1]),
					qn(v.tangent[2]),
					qn(v.tangent[3]),
				],
				uv: [v.uv.x, v.uv.y],
				uv_lightmap: [qu(v.uv_lightmap.x), qu(v.uv_lightmap.y)],
				color: v.color,
			})
			.collect();
		let vdata = bytemuck::cast_slice(&gpu_verts);
		let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[mesh] vbuf"),
			size: vdata.len() as u64,
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		queue.write_buffer(&vbuf, 0, vdata);

		// position-only buffer for shadow and z-prepass passes (12 bytes/vertex vs 32)
		#[cfg(not(target_arch = "wasm32"))]
		let positions: Vec<[f32; 3]> = {
			use rayon::prelude::*;
			data.vertices
				.par_iter()
				.map(|v| [v.position.x, v.position.y, v.position.z])
				.collect()
		};
		#[cfg(target_arch = "wasm32")]
		let positions: Vec<[f32; 3]> = data
			.vertices
			.iter()
			.map(|v| [v.position.x, v.position.y, v.position.z])
			.collect();
		let pos_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[mesh] pos buf"),
			size: (positions.len() * 12) as u64,
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		queue.write_buffer(&pos_buf, 0, bytemuck::cast_slice(&positions));

		match &data.indices {
			IndexBuffer::U16(v) => {
				#[cfg(not(target_arch = "wasm32"))]
				let u32_indices: Vec<u32> = {
					use rayon::prelude::*;
					v.par_iter().map(|&i| i as u32).collect()
				};
				#[cfg(target_arch = "wasm32")]
				let u32_indices: Vec<u32> = v.iter().map(|&i| i as u32).collect();
				let optimized = Self::forsyth_optimize(&u32_indices, data.vertices.len());
				let u16_opt: Vec<u16> = optimized.iter().map(|&i| i as u16).collect();
				let ibuf = device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("[mesh] ibuf"),
					size: (u16_opt.len() * 2) as u64,
					usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(u16_opt.as_slice()));
				GpuMesh {
					vbuf,
					pos_buf,
					ibuf,
					index_count: u16_opt.len() as u32,
					index_fmt: wgpu::IndexFormat::Uint16,
				}
			}
			IndexBuffer::U32(v) => {
				let optimized = Self::forsyth_optimize(v, data.vertices.len());
				let ibuf = device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("[mesh] ibuf"),
					size: (optimized.len() * 4) as u64,
					usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(optimized.as_slice()));
				GpuMesh {
					vbuf,
					pos_buf,
					ibuf,
					index_count: optimized.len() as u32,
					index_fmt: wgpu::IndexFormat::Uint32,
				}
			}
		}
	}
	/// reorder triangle indices to maximize GPU vertex cache utilization (Forsyth 2006).
	/// improves post-transform cache hit rate from ~50% to ~90% for typical meshes.
	/// runs once per mesh at upload time; the original index buffer is not modified.
	pub(crate) fn forsyth_optimize(indices: &[u32], vertex_count: usize) -> Vec<u32> {
		const CACHE_SIZE: usize = 32;
		let tri_count = indices.len() / 3;
		if tri_count == 0 || vertex_count == 0 {
			return indices.to_vec();
		}

		// per-vertex: remaining triangle count + list of triangle indices
		let mut vert_tris: Vec<Vec<u32>> = vec![Vec::new(); vertex_count];
		for (ti, chunk) in indices.chunks_exact(3).enumerate() {
			for &vi in chunk {
				if (vi as usize) < vertex_count {
					vert_tris[vi as usize].push(ti as u32);
				}
			}
		}
		let mut vert_remaining: Vec<u32> = vert_tris.iter().map(|v| v.len() as u32).collect();

		// vertex score: cache position → score
		let cache_score = |pos: usize| -> f32 {
			if pos >= CACHE_SIZE {
				return 0.0;
			}
			if pos < 3 {
				return 0.75;
			} // just used
			((1.0 - (pos - 3) as f32 / (CACHE_SIZE - 3) as f32).powi(3)) * 0.5
		};
		let valence_score = |remaining: u32| -> f32 {
			if remaining == 0 {
				return 0.0;
			}
			2.0 * (remaining as f32).sqrt().recip()
		};

		let mut vert_score: Vec<f32> = (0..vertex_count)
			.map(|v| valence_score(vert_remaining[v]) + cache_score(CACHE_SIZE))
			.collect();

		// per-triangle: sum of vertex scores; u32::MAX = already emitted
		let mut tri_score: Vec<f32> = (0..tri_count)
			.map(|ti| {
				indices[ti * 3..ti * 3 + 3]
					.iter()
					.map(|&vi| vert_score[vi as usize])
					.sum()
			})
			.collect();
		let mut tri_emitted: Vec<bool> = vec![false; tri_count];

		let mut out = Vec::with_capacity(indices.len());
		let mut cache: Vec<i32> = vec![-1i32; CACHE_SIZE]; // -1 = empty slot

		let mut best_tri = (0..tri_count)
			.max_by(|&a, &b| {
				tri_score[a]
					.partial_cmp(&tri_score[b])
					.unwrap_or(std::cmp::Ordering::Equal)
			})
			.unwrap_or(0);

		while out.len() < indices.len() {
			if tri_emitted[best_tri] {
				// find next unemitted triangle with highest score
				best_tri = (0..tri_count)
					.filter(|&t| !tri_emitted[t])
					.max_by(|&a, &b| {
						tri_score[a]
							.partial_cmp(&tri_score[b])
							.unwrap_or(std::cmp::Ordering::Equal)
					})
					.unwrap_or_else(|| (0..tri_count).find(|&t| !tri_emitted[t]).unwrap_or(0));
			}
			tri_emitted[best_tri] = true;
			let v0 = indices[best_tri * 3] as usize;
			let v1 = indices[best_tri * 3 + 1] as usize;
			let v2 = indices[best_tri * 3 + 2] as usize;
			out.push(v0 as u32);
			out.push(v1 as u32);
			out.push(v2 as u32);

			// update cache: insert v0, v1, v2 at front, shift others
			let new_verts = [v0, v1, v2];
			let mut new_cache: Vec<i32> = new_verts.iter().map(|&v| v as i32).collect();
			for &slot in &cache {
				if slot >= 0 && !new_verts.contains(&(slot as usize)) {
					new_cache.push(slot);
					if new_cache.len() >= CACHE_SIZE {
						break;
					}
				}
			}
			while new_cache.len() < CACHE_SIZE {
				new_cache.push(-1);
			}
			cache.copy_from_slice(&new_cache[..CACHE_SIZE]);

			// recompute vertex scores for vertices now in cache
			let mut verts_to_update: Vec<usize> = new_verts.to_vec();
			for &slot in &cache {
				if slot >= 0 {
					verts_to_update.push(slot as usize);
				}
			}
			verts_to_update.sort_unstable();
			verts_to_update.dedup();

			for &vi in &verts_to_update {
				if vi >= vertex_count {
					continue;
				}
				let cache_pos = cache
					.iter()
					.position(|&s| s == vi as i32)
					.unwrap_or(CACHE_SIZE);
				vert_remaining[vi] = vert_tris[vi]
					.iter()
					.filter(|&&ti| !tri_emitted[ti as usize])
					.count() as u32;
				vert_score[vi] = valence_score(vert_remaining[vi]) + cache_score(cache_pos);
			}

			// update triangle scores for triangles adjacent to updated vertices
			let mut tris_to_update: Vec<usize> = Vec::new();
			for &vi in &verts_to_update {
				if vi >= vertex_count {
					continue;
				}
				for &ti in &vert_tris[vi] {
					if !tri_emitted[ti as usize] {
						tris_to_update.push(ti as usize);
					}
				}
			}
			tris_to_update.sort_unstable();
			tris_to_update.dedup();

			let mut best_score = f32::NEG_INFINITY;
			let mut best_in_cache: usize = usize::MAX;
			for &ti in &tris_to_update {
				tri_score[ti] = indices[ti * 3..ti * 3 + 3]
					.iter()
					.map(|&vi| vert_score[vi as usize])
					.sum();
				if tri_score[ti] > best_score {
					best_score = tri_score[ti];
					best_in_cache = ti;
				}
			}
			best_tri = if best_in_cache != usize::MAX {
				best_in_cache
			} else {
				0
			};
		}
		out
	}
	/// build a flat NxN quad grid for one clipmap ring.
	/// vertices carry grid coords in position.xz (0..=resolution), position.y = 0.
	/// the vertex shader reads the heightmap to displace Y.
	pub(crate) fn build_clipmap_patch(resolution: u32) -> MeshData {
		let n = (resolution + 1) as usize;
		let mut vertices = Vec::with_capacity(n * n);
		for row in 0..=resolution {
			for col in 0..=resolution {
				let x = col as f32;
				let z = row as f32;
				let uv = Vec2::new(x / resolution as f32, z / resolution as f32);
				vertices.push(Vertex3d::new(
					Vec3::new(x, 0.0, z),
					Vec3::Y,
					[1.0, 0.0, 0.0, 1.0],
					uv,
				));
			}
		}
		let mut indices: Vec<u32> =
			Vec::with_capacity(resolution as usize * resolution as usize * 6);
		for row in 0..resolution {
			for col in 0..resolution {
				let tl = row * (resolution + 1) + col;
				let tr = tl + 1;
				let bl = tl + (resolution + 1);
				let br = bl + 1;
				indices.extend_from_slice(&[tl, bl, tr, tr, bl, br]);
			}
		}
		MeshData::new(vertices, IndexBuffer::U32(indices))
	}
	/// upload a R16Float heightmap to the GPU.
	pub(crate) fn upload_heightmap(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		data: &[u8],
		width: u32,
		height: u32,
	) -> (wgpu::Texture, wgpu::TextureView) {
		let tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[terrain] heightmap"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::R16Float,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		if !data.is_empty() {
			queue.write_texture(
				tex.as_image_copy(),
				data,
				wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(width * 2), // R16Float = 2 bytes per sample
					rows_per_image: None,
				},
				wgpu::Extent3d {
					width,
					height,
					depth_or_array_layers: 1,
				},
			);
		}
		let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
		(tex, view)
	}
	/// initialise GPU resources for one terrain entity.
	pub(crate) fn build_terrain_gpu(
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		params_bgl: &wgpu::BindGroupLayout,
		terrain: &Terrain,
	) -> TerrainGpu {
		// build ring meshes: center patch + (clipmap_rings - 1) outer rings
		let rings = terrain.clipmap_rings.clamp(1, 8);
		let resolution = terrain.ring_resolution.clamp(4, 256);
		let mut ring_meshes = Vec::with_capacity(rings as usize);
		for _ in 0..rings {
			let mesh = Self::build_clipmap_patch(resolution);
			ring_meshes.push(Self::upload_mesh_data(device, queue, &mesh));
		}

		let (w, h) = (
			terrain.heightmap_width.max(1),
			terrain.heightmap_height.max(1),
		);
		let (heightmap_tex, heightmap_view) =
			Self::upload_heightmap(device, queue, &terrain.heightmap, w, h);

		let hmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[terrain] heightmap sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});

		let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[terrain] params buffer"),
			size: TERRAIN_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let params_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[terrain] params bg"),
			layout: params_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&heightmap_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&hmap_sampler),
				},
			],
		});

		TerrainGpu {
			heightmap_tex,
			heightmap_view,
			ring_meshes,
			params_buf,
			params_bg,
			hmap_sampler,
		}
	}
	pub(crate) fn pack_mesh_uniforms(staging: &mut [u8], slot: usize, model: Mat4) {
		let offset = slot * UNIFORM_STRIDE as usize;
		Self::pack_mesh_uniforms_at(&mut staging[offset..offset + 112], model);
	}
	/// write the 112-byte mesh block (model matrix + normal matrix) at the start of
	/// `slot_buf`. the slot-relative form lets the per-entity packing loop run over
	/// disjoint `par_chunks_mut` slices without each task needing the whole buffer.
	pub(crate) fn pack_mesh_uniforms_at(slot_buf: &mut [u8], model: Mat4) {
		// model matrix (64 bytes)
		let model_cols = model.to_cols_array();
		slot_buf[0..64].copy_from_slice(bytemuck::cast_slice(&model_cols));
		// normal matrix = transpose(inverse(mat3(model))), packed as 3×vec4 (48 bytes)
		let normal_mat = Mat3::from_mat4(model).inverse().transpose();
		let cols = normal_mat.to_cols_array();
		let normal_packed: [f32; 12] = [
			cols[0], cols[1], cols[2], 0.0, cols[3], cols[4], cols[5], 0.0, cols[6], cols[7],
			cols[8], 0.0,
		];
		slot_buf[64..112].copy_from_slice(bytemuck::cast_slice(&normal_packed));
	}
	/// write 9 L2 SH coefficients (the 144-byte block at offset 112 within `slot_buf`,
	/// a full slot stride). `coeffs[i] = [R, G, B]`; flag=1.0 in `[0].w` marks per-entity
	/// probe data present.
	pub(crate) fn pack_sh_uniforms_at(slot_buf: &mut [u8], coeffs: &[[f32; 3]; 9]) {
		let mut data = [0f32; 36];
		for (i, c) in coeffs.iter().enumerate() {
			data[i * 4] = c[0];
			data[i * 4 + 1] = c[1];
			data[i * 4 + 2] = c[2];
			data[i * 4 + 3] = if i == 0 { 1.0 } else { 0.0 }; // flag only in [0].w
		}
		slot_buf[112..256].copy_from_slice(bytemuck::cast_slice(&data));
	}
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn pack_material_uniforms(
		staging: &mut [u8],
		slot: usize,
		color: Color,
		metallic: f32,
		roughness: f32,
		flags: u32,
		has_lightmap: u32,
		lm_uv_offset: [f32; 2],
		lm_uv_scale: [f32; 2],
	) {
		let offset = slot * MATERIAL_UNIFORMS_SIZE as usize;
		Self::pack_material_uniforms_at(
			&mut staging[offset..offset + MATERIAL_UNIFORMS_SIZE as usize],
			color,
			metallic,
			roughness,
			flags,
			has_lightmap,
			lm_uv_offset,
			lm_uv_scale,
		);
	}
	/// write the 48-byte material block at the start of `slot_buf` (slot-relative form
	/// for the parallel per-entity packing loop).
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn pack_material_uniforms_at(
		slot_buf: &mut [u8],
		color: Color,
		metallic: f32,
		roughness: f32,
		flags: u32,
		has_lightmap: u32,
		lm_uv_offset: [f32; 2],
		lm_uv_scale: [f32; 2],
	) {
		// base_color(16) + metallic(4) + roughness(4) + flags(4) + has_lightmap(4) = 32 bytes
		let data: [f32; 7] = [
			color.r,
			color.g,
			color.b,
			color.a,
			metallic,
			roughness,
			f32::from_bits(flags),
		];
		slot_buf[0..28].copy_from_slice(bytemuck::cast_slice(&data));
		slot_buf[28..32].copy_from_slice(&has_lightmap.to_le_bytes());
		// lm_uv_offset(8) + lm_uv_scale(8) = 16 bytes at offset 32
		slot_buf[32..40].copy_from_slice(bytemuck::cast_slice(&lm_uv_offset));
		slot_buf[40..48].copy_from_slice(bytemuck::cast_slice(&lm_uv_scale));
	}

	// ── public surface management ──────────────────────────────────────────
}

#[cfg(test)]
mod packing_tests {
	use super::*;
	use lunar_math::{Mat4, Quat, Vec3};
	use rayon::prelude::*;

	/// deterministic per-entity model with rotation + non-uniform scale, so the
	/// normal matrix (inverse-transpose) and every packed byte are non-trivial.
	fn model_for(i: usize) -> Mat4 {
		let f = i as f32;
		Mat4::from_scale_rotation_translation(
			Vec3::new(1.0 + (f * 0.1) % 3.0, 0.5 + (f * 0.2) % 2.0, 1.0 + (f * 0.05) % 1.5),
			Quat::from_rotation_y(f * 0.137) * Quat::from_rotation_x(f * 0.071),
			Vec3::new(f, -f * 0.5, f * 2.0),
		)
	}

	/// the parallel per-entity packing (`par_chunks_mut` + the `*_at` writers) must produce
	/// byte-identical staging to the serial slot-based packing the GPU previously consumed.
	#[test]
	fn parallel_packing_matches_serial() {
		let n = 1000usize;
		let base = ENTITY_SLOT_START;
		let stride = UNIFORM_STRIDE as usize;
		let mat_size = MATERIAL_UNIFORMS_SIZE as usize;
		let total_slots = base + n;

		let models: Vec<Mat4> = (0..n).map(model_for).collect();
		let sh: Vec<[[f32; 3]; 9]> = (0..n)
			.map(|i| core::array::from_fn(|k| [i as f32 + k as f32, k as f32, (i % 7) as f32]))
			.collect();
		// alternate "has SH" so the None branch (which leaves prior bytes) is exercised the same.
		let has_sh: Vec<bool> = (0..n).map(|i| i % 3 != 0).collect();
		let colors: Vec<Color> = (0..n)
			.map(|i| Color::rgba((i % 5) as f32 / 5.0, 0.3, 0.7, 1.0))
			.collect();

		// 0xAB sentinel so the "untouched on None-SH" tail bytes are well-defined and compared.
		let mut serial_u = vec![0xABu8; total_slots * stride];
		let mut serial_m = vec![0u8; total_slots * mat_size];
		let mut parallel_u = serial_u.clone();
		let mut parallel_m = serial_m.clone();

		// serial reference: the original slot-based packing.
		for i in 0..n {
			let slot = base + i;
			RenderEngine3d::pack_mesh_uniforms(&mut serial_u, slot, models[i]);
			if has_sh[i] {
				RenderEngine3d::pack_sh_uniforms_at(
					&mut serial_u[slot * stride..slot * stride + stride],
					&sh[i],
				);
			}
			RenderEngine3d::pack_material_uniforms(
				&mut serial_m,
				slot,
				colors[i],
				0.2,
				0.6,
				(i % 4) as u32,
				(i % 2) as u32,
				[0.1, 0.2],
				[0.9, 0.8],
			);
		}

		// parallel path: chunked `*_at` writers over disjoint slot slices.
		parallel_u[base * stride..(base + n) * stride]
			.par_chunks_mut(stride)
			.zip(parallel_m[base * mat_size..(base + n) * mat_size].par_chunks_mut(mat_size))
			.enumerate()
			.for_each(|(i, (uniform_slot, material_slot))| {
				RenderEngine3d::pack_mesh_uniforms_at(uniform_slot, models[i]);
				if has_sh[i] {
					RenderEngine3d::pack_sh_uniforms_at(uniform_slot, &sh[i]);
				}
				RenderEngine3d::pack_material_uniforms_at(
					material_slot,
					colors[i],
					0.2,
					0.6,
					(i % 4) as u32,
					(i % 2) as u32,
					[0.1, 0.2],
					[0.9, 0.8],
				);
			});

		assert_eq!(serial_u, parallel_u, "uniform staging diverged between serial and parallel");
		assert_eq!(serial_m, parallel_m, "material staging diverged between serial and parallel");
	}
}
