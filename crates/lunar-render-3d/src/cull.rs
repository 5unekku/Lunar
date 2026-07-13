//! `RenderEngine3d`: frustum/hzb culling and per-frame draw-list assembly.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules: all share the struct's private fields).

use super::*;

// mapped BufferView is not guaranteed u32-aligned, so read without cast
fn mapped_u32s(bytes: &[u8]) -> Vec<u32> {
	bytes
		.chunks_exact(4)
		.map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
		.collect()
}

impl RenderEngine3d {
	/// frustum + HZB occlusion culling for this frame. the frustum test is a
	/// fresh CPU SIMD sweep every frame on every tier; high tier additionally
	/// applies the previous frame's HZB occlusion result (1-frame pipelined,
	/// tested against the view_proj snapshot the HZB was built with) and
	/// dispatches this frame's occlusion + LOD compute. populates `self.frustum_visible`.
	pub(crate) fn cull_entities(&mut self, world: &mut World, cam_pos: Vec3) {
		// ── frustum cull: CPU SIMD sweep, every tier ─────────────────────
		// always this-frame correct. the old high-tier path gated visibility on
		// a 1-frame-stale gpu readback: rotating the camera popped geometry at
		// the screen edges for a frame, and slow-gpu frames applied flags from
		// an even older camera. the sweep is 8 boxes/iter on AVX2 and costs
		// microseconds at real entity counts; the late gpu cull in frame.rs
		// still feeds the one-call indirect path gpu-side with this-frame data.
		self.frustum_visible.clear();
		let frustum = *world.resource::<Frustum>();
		{
			let soa = world.resource::<CullSoa>();
			let n = soa.entities.len();
			self.frustum_flags_scratch.clear();
			self.frustum_flags_scratch.resize(n, 0);
			if n > 0 {
				let planes = &frustum.planes;
				let flags = &mut self.frustum_flags_scratch;
				#[cfg(not(target_arch = "wasm32"))]
				{
					use rayon::prelude::*;
					// fan the 8-wide sweep across cores only once there's enough work to
					// amortise the rayon hand-off; each chunk owns a disjoint flag slice.
					const PARALLEL_CHUNK: usize = 4096;
					if n >= PARALLEL_CHUNK * 2 {
						flags
							.par_chunks_mut(PARALLEL_CHUNK)
							.enumerate()
							.for_each(|(chunk_idx, flags_chunk)| {
								let start = chunk_idx * PARALLEL_CHUNK;
								let end = start + flags_chunk.len();
								lunar_3d::cull_aabbs_soa(
									planes,
									&soa.center_x[start..end],
									&soa.center_y[start..end],
									&soa.center_z[start..end],
									&soa.half_x[start..end],
									&soa.half_y[start..end],
									&soa.half_z[start..end],
									flags_chunk,
								);
							});
					} else {
						lunar_3d::cull_aabbs_soa(
							planes,
							&soa.center_x,
							&soa.center_y,
							&soa.center_z,
							&soa.half_x,
							&soa.half_y,
							&soa.half_z,
							flags,
						);
					}
				}
				#[cfg(target_arch = "wasm32")]
				lunar_3d::cull_aabbs_soa(
					planes,
					&soa.center_x,
					&soa.center_y,
					&soa.center_z,
					&soa.half_x,
					&soa.half_y,
					&soa.half_z,
					flags,
				);
			}
			for (i, &entity) in soa.entities.iter().enumerate() {
				if self.frustum_flags_scratch[i] != 0 {
					self.frustum_visible.insert(entity);
				}
			}
		}

		let entity_count = world.resource::<CullSoa>().entities.len();

		// per-entity AABB upload data (CullSoa order): built once, shared by the
		// gpu LOD select and the HZB occlusion dispatch below
		let hzb_active = self.hzb_enabled && self.hzb_texture.is_some();
		if (self.gpu_cull_enabled || hzb_active) && entity_count > 0 {
			self.cull_aabb_scratch.clear();
			let soa = world.resource::<CullSoa>();
			for i in 0..entity_count {
				self.cull_aabb_scratch.extend_from_slice(&[
					soa.center_x[i],
					soa.center_y[i],
					soa.center_z[i],
					0.0,
					soa.half_x[i],
					soa.half_y[i],
					soa.half_z[i],
					0.0,
				]);
			}
		}

		// ── gpu LOD selection (high tier, 1-frame pipelined) ─────────────
		if self.gpu_cull_enabled && entity_count > 0 {
			let _ = self.device.poll(wgpu::PollType::Poll); // fire completed map_async callbacks

			// read previous frame's LOD staging result
			if self.lod_staging_pending && self.lod_staging_ready.load(Ordering::Acquire) {
				let prev_count = self.lod_pending_entity_count;
				if let Some(staging) = self.lod_indices_staging.as_ref() {
					{
						let slice = staging.slice(0..(prev_count * 4) as u64);
						let data = slice.get_mapped_range();
						let indices = mapped_u32s(&data);
						let soa = world.resource::<CullSoa>();
						self.gpu_lod_indices.clear();
						for (i, &entity) in soa.entities.iter().take(prev_count).enumerate() {
							if i < indices.len() {
								self.gpu_lod_indices.insert(entity, indices[i]);
							}
						}
					}
					staging.unmap();
				}
				self.lod_staging_ready.store(false, Ordering::Release);
				self.lod_staging_pending = false;
			}

			self.ensure_gpu_cull_resources(entity_count);
			self.ensure_lod_select_resources(entity_count);

			// a staging buffer is only reusable once its previous map_async has been
			// drained (pending cleared by the read block above or a buffer rebuild)
			let lod_staging_free = !self.lod_staging_pending;

			// (re)build the LOD bind group only when its backing buffers regrew;
			// the ensure_* paths reset it to None on growth
			if self.lod_select_bg.is_none()
				&& let (Some(lod_bgl), Some(lod_params_buf), Some(lod_buf), Some(aabb_for_lod)) = (
					self.lod_select_bgl.as_ref(),
					self.lod_params_buf.as_ref(),
					self.lod_indices_buf.as_ref(),
					self.cull_aabb_buf.as_ref(),
				) {
				self.lod_select_bg =
					Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
						label: Some("[lod select] bg"),
						layout: lod_bgl,
						entries: &[
							wgpu::BindGroupEntry {
								binding: 0,
								resource: lod_params_buf.as_entire_binding(),
							},
							wgpu::BindGroupEntry {
								binding: 1,
								resource: aabb_for_lod.as_entire_binding(),
							},
							wgpu::BindGroupEntry {
								binding: 2,
								resource: lod_buf.as_entire_binding(),
							},
						],
					}));
			}

			if let (Some(lod_pipeline), Some(lod_params_buf), Some(lod_buf), Some(lod_bg), Some(aabb_buf)) = (
				self.lod_select_pipeline.as_ref(),
				self.lod_params_buf.as_ref(),
				self.lod_indices_buf.as_ref(),
				self.lod_select_bg.as_ref(),
				self.cull_aabb_buf.as_ref(),
			) {
				self.queue
					.write_buffer(aabb_buf, 0, bytemuck::cast_slice(&self.cull_aabb_scratch));
				let mut lod_params_data = [0u32; 8];
				lod_params_data[0] = cam_pos.x.to_bits();
				lod_params_data[1] = cam_pos.y.to_bits();
				lod_params_data[2] = cam_pos.z.to_bits();
				lod_params_data[3] = entity_count as u32;
				// squared distance thresholds: [15²=225, 50²=2500, 150²=22500, 400²=160000]
				lod_params_data[4] = 225.0f32.to_bits();
				lod_params_data[5] = 2500.0f32.to_bits();
				lod_params_data[6] = 22500.0f32.to_bits();
				lod_params_data[7] = 160000.0f32.to_bits();
				self.queue
					.write_buffer(lod_params_buf, 0, bytemuck::cast_slice(&lod_params_data));
				let mut lod_enc =
					self.device
						.create_command_encoder(&wgpu::CommandEncoderDescriptor {
							label: Some("[lod select] encoder"),
						});
				{
					let mut lpass = lod_enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
						label: Some("[lod select] pass"),
						timestamp_writes: None,
					});
					lpass.set_pipeline(lod_pipeline);
					lpass.set_bind_group(0, lod_bg, &[]);
					lpass.dispatch_workgroups((entity_count as u32).div_ceil(64), 1, 1);
				}
				// copy fresh indices out only while the staging buffer is free: a buffer
				// with an outstanding map_async must not appear in a submit
				if lod_staging_free && let Some(lod_staging) = self.lod_indices_staging.as_ref() {
					lod_enc.copy_buffer_to_buffer(
						lod_buf,
						0,
						lod_staging,
						0,
						(entity_count * 4) as u64,
					);
				}
				self.queue.submit([lod_enc.finish()]);

				// register LOD staging map_async for next frame
				if lod_staging_free && let Some(lod_staging) = self.lod_indices_staging.as_ref() {
					let lod_ready = self.lod_staging_ready.clone();
					lod_ready.store(false, Ordering::Release);
					lod_staging.slice(0..(entity_count * 4) as u64).map_async(
						wgpu::MapMode::Read,
						move |result| {
							if result.is_ok() {
								lod_ready.store(true, Ordering::Release);
							}
						},
					);
					self.lod_staging_pending = true;
					self.lod_pending_entity_count = entity_count;
				}
			}
		}

		// ── HZB occlusion cull (high tier, 1-frame pipelined) ────────────
		// applies previous frame's occlusion result to frustum_visible, then
		// dispatches this frame's occlusion compute for next frame's use.
		// no CPU stall: the previous frame's compute completed while we were
		// building the draw list.
		if hzb_active {
			if entity_count > 0 {
				self.ensure_hzb_cull_buffers(entity_count);

				// read previous frame's occlusion result: non-blocking
				if self.hzb_staging_pending {
					let _ = self.device.poll(wgpu::PollType::Poll);
					if self.hzb_staging_ready.load(Ordering::Acquire) {
						let prev = self.hzb_pending_entity_count;
						if let Some(occ_staging) = self.hzb_occ_staging.as_ref() {
							{
								let slice = occ_staging.slice(0..(prev * 4) as u64);
								let data = slice.get_mapped_range();
								let flags = mapped_u32s(&data);
								let soa = world.resource::<CullSoa>();
								for (i, &entity) in soa.entities.iter().take(prev).enumerate() {
									if i < flags.len() && flags[i] == 0 {
										self.frustum_visible.remove(&entity);
									}
								}
							}
							occ_staging.unmap();
						}
						self.hzb_staging_ready.store(false, Ordering::Release);
						self.hzb_staging_pending = false;
					}
					// if not ready: skip hzb cull for this frame (frustum_visible unchanged)
				}

				// dispatch this frame's HZB occlusion compute. skipped while the previous
				// readback is still in flight (a buffer with an outstanding map_async must
				// not appear in a submit) and until the first HZB has actually been built,
				// so the test never runs against a cleared depth pyramid
				if !self.hzb_staging_pending && self.hzb_built {
					// test against the view_proj snapshot taken when the HZB depth was
					// drawn: testing last frame's depth with this frame's matrix falsely
					// culled still-visible geometry whenever the camera moved
					let vp_array = self.hzb_view_proj.to_cols_array();
					let mut params_data = [0f32; 24];
					params_data[..16].copy_from_slice(&vp_array);
					// footprint-to-mip selection happens in HZB texel space, so the
					// viewport is the HZB mip 0 size (not the display surface size)
					params_data[16] = self.hzb_width as f32;
					params_data[17] = self.hzb_height as f32;
					params_data[18] = f32::from_bits(self.hzb_mip_count);
					params_data[19] = f32::from_bits(entity_count as u32);

					// seed occlusion flags from this frame's fresh CPU frustum result
					self.hzb_seed_scratch.clear();
					self.hzb_seed_scratch
						.extend(self.frustum_flags_scratch.iter().map(|&flag| flag as u32));
					self.queue.write_buffer(
						self.hzb_occ_buf.as_ref().unwrap(),
						0,
						bytemuck::cast_slice(&self.hzb_seed_scratch),
					);
					self.queue.write_buffer(
						self.hzb_cull_aabb_buf.as_ref().unwrap(),
						0,
						bytemuck::cast_slice(&self.cull_aabb_scratch),
					);
					self.queue.write_buffer(
						self.hzb_cull_params_buf.as_ref().unwrap(),
						0,
						bytemuck::cast_slice(&params_data),
					);

					// (re)build the hzb-cull bind group only when its buffers regrew (reset to None
					// in ensure_hzb_cull_buffers); the hzb src view is fixed-size so it never changes.
					if self.hzb_cull_bg.is_none() {
						self.hzb_cull_bg = Some(
							self.device.create_bind_group(&wgpu::BindGroupDescriptor {
								label: Some("[hzb] cull bg"),
								layout: self.hzb_cull_bgl.as_ref().unwrap(),
								entries: &[
									wgpu::BindGroupEntry {
										binding: 0,
										resource: self
											.hzb_cull_aabb_buf
											.as_ref()
											.unwrap()
											.as_entire_binding(),
									},
									wgpu::BindGroupEntry {
										binding: 1,
										resource: self
											.hzb_cull_params_buf
											.as_ref()
											.unwrap()
											.as_entire_binding(),
									},
									wgpu::BindGroupEntry {
										binding: 2,
										resource: self
											.hzb_occ_buf
											.as_ref()
											.unwrap()
											.as_entire_binding(),
									},
									wgpu::BindGroupEntry {
										binding: 3,
										resource: wgpu::BindingResource::TextureView(
											self.hzb_src_view.as_ref().unwrap(),
										),
									},
								],
							}),
						);
					}

					let occ_buf = self.hzb_occ_buf.as_ref().unwrap();
					let occ_staging = self.hzb_occ_staging.as_ref().unwrap();
					let hzb_cull_bg = self.hzb_cull_bg.as_ref().unwrap();

					let mut hzb_enc =
						self.device
							.create_command_encoder(&wgpu::CommandEncoderDescriptor {
								label: Some("[hzb] cull encoder"),
							});
					{
						let mut cpass = hzb_enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
							label: Some("[hzb] cull pass"),
							timestamp_writes: None,
						});
						cpass.set_pipeline(self.hzb_cull_pipeline.as_ref().unwrap());
						cpass.set_bind_group(0, hzb_cull_bg, &[]);
						cpass.dispatch_workgroups((entity_count as u32).div_ceil(64), 1, 1);
					}
					hzb_enc.copy_buffer_to_buffer(
						occ_buf,
						0,
						occ_staging,
						0,
						(entity_count * 4) as u64,
					);
					self.queue.submit([hzb_enc.finish()]);
					let hzb_ready = self.hzb_staging_ready.clone();
					hzb_ready.store(false, Ordering::Release);
					occ_staging.slice(0..(entity_count * 4) as u64).map_async(
						wgpu::MapMode::Read,
						move |result| {
							if result.is_ok() {
								hzb_ready.store(true, Ordering::Release);
							}
						},
					);
					self.hzb_staging_pending = true;
					self.hzb_pending_entity_count = entity_count;
				}
			}
		}
	}
	/// build the per-frame draw list: BSP/portal area visibility, then a world
	/// query filtered by visibility + frustum, written into draw_scratch /
	/// raw_scratch / impostor_scratch (with prev-transform interpolation).
	pub(crate) fn gather_draw_list(&mut self, world: &mut World, cam_pos: Vec3) {
		// ── gather draw list ──────────────────────────────────────────────

		// build area visibility from BspLevel PVS if loaded; fall through to VisibleAreas otherwise.
		// reuses bsp_visible_scratch; `active` mirrors the old `Option::is_some`.
		self.bsp_visible_scratch.clear();
		self.bsp_visible_active = false;
		if let Some(level) = world
			.get_resource::<BspLevel>()
			.filter(|level| level.is_loaded())
		{
			let leaf = level.camera_leaf(cam_pos);
			let area_map = level.area_map();
			let bsp_visible = &mut self.bsp_visible_scratch;
			level.for_each_visible_leaf(leaf, |leaf_idx| {
				if let Ok(pos) = area_map.binary_search_by_key(&(leaf_idx as u32), |&(li, _)| li) {
					bsp_visible.insert(area_map[pos].1);
				}
			});
			self.bsp_visible_active = true;
		}

		// write visible areas back so game code (AI LOS queries etc.) reads a correct set
		if self.bsp_visible_active
			&& let Some(mut vis_areas) = world.get_resource_mut::<VisibleAreas>()
		{
			vis_areas.area_ids.clear();
			vis_areas
				.area_ids
				.extend(self.bsp_visible_scratch.iter().copied());
			vis_areas.active = true;
		}

		// snapshot portal visible areas before the mutable query borrow (reuses portal_visible_scratch)
		self.portal_visible_scratch.clear();
		self.portal_visible_active = false;
		if let Some(pv) = world.get_resource::<VisibleAreas>().filter(|pv| pv.active) {
			self.portal_visible_scratch
				.extend(pv.area_ids.iter().copied());
			self.portal_visible_active = true;
		}

		let interp_alpha = world
			.get_resource::<lunar_core::Time>()
			.map(|t| t.interp_alpha())
			.unwrap_or(1.0);

		self.raw_scratch.clear();
		self.impostor_scratch.clear();
		// reserve capacity equal to current peak so steady-state frames never reallocate
		let prev_raw = self.raw_scratch.capacity();
		if prev_raw == 0 {
			self.raw_scratch.reserve(64);
		}
		let prev_draw = self.draw_scratch.capacity();
		if prev_draw == 0 {
			self.draw_scratch.reserve(64);
		}
		{
			let q = &mut self.queries.as_mut().unwrap().cullables;
			q.iter(world)
				.filter(|(entity, _, _, _, vis, aabb, _, _, area, _, _, _)| {
					if !vis.0 {
						return false;
					}
					// BSP PVS area culling (takes priority over portal traversal)
					if self.bsp_visible_active {
						if let Some(a) = area
							&& !self.bsp_visible_scratch.contains(&a.0)
						{
							return false;
						}
					} else if self.portal_visible_active
						&& let Some(a) = area
						&& !self.portal_visible_scratch.contains(&a.0)
					{
						return false;
					}
					aabb.is_none() || self.frustum_visible.contains(entity)
				})
				.for_each(
					|(
						entity,
						mesh,
						mat,
						wt,
						_,
						_,
						lod,
						impostor,
						_,
						lightmap,
						dir_lightmap,
						prev_wt,
					)| {
						let render_wt = prev_wt
							.map(|prev| prev.0.lerp(wt, interp_alpha))
							.unwrap_or(*wt);
						// SIMD distance² (Vec3A): this runs per visible renderable, hot path
						let dist_sq = (Vec3A::from(render_wt.translation) - Vec3A::from(cam_pos))
							.length_squared();

						// check if entity should use impostor billboard
						if let Some(imp) = impostor
							&& dist_sq >= imp.min_dist_sq
						{
							// compute view azimuth angle around Y for atlas selection
							let to_entity = Vec3::from(render_wt.translation) - cam_pos;
							let view_angle = to_entity.z.atan2(to_entity.x);
							let (u_min, u_max, _, _) = imp.atlas.uv_rect(view_angle);
							self.impostor_scratch.push((
								Vec3::from(render_wt.translation),
								imp.half_width,
								imp.half_height,
								imp.atlas.texture.id(),
								u_min,
								u_max,
							));
							return; // skip mesh draw
						}

						// normal mesh draw: GPU LOD index (1-frame pipelined) or CPU dist fallback
						let mesh_id = if let Some(&gpu_lod) = self.gpu_lod_indices.get(&entity) {
							lod.and_then(|l| {
								if gpu_lod == 0 {
									None
								} else {
									l.levels.get((gpu_lod - 1) as usize).map(|(_, h)| *h)
								}
							})
							.unwrap_or(mesh.0)
						} else {
							lod.and_then(|l| l.select(dist_sq)).unwrap_or(mesh.0)
						}
						.id();
						let lm_id = lightmap
							.map(|lm| lm.texture.id())
							.or_else(|| dir_lightmap.map(|dlm| dlm.irradiance.id()))
							.unwrap_or(u32::MAX);
						let dir_lm_id = dir_lightmap
							.map(|dlm| dlm.direction.id())
							.unwrap_or(u32::MAX);
						self.raw_scratch.push((
							entity,
							mesh_id,
							mat.0.id(),
							render_wt.to_matrix(),
							lm_id,
							dir_lm_id,
						));
					},
				);
		}

		// collect static entities and assign stable slot ids (reuses static_entities_scratch)
		{
			let q = &mut self.queries.as_mut().unwrap().static_meshes;
			// fast unchanged-set check: one pass over StaticMesh. if every entity already owns a
			// slot and the counts match, the set is identical to last frame (subset of equal size
			// ⇒ equal set), so skip the hashset rebuild + retain + max-scan + slot assignment.
			let mut count = 0usize;
			let mut all_known = true;
			for (e, _) in q.iter(world) {
				count += 1;
				if !self.static_entity_slots.contains_key(&e) {
					all_known = false;
					break;
				}
			}
			if !(all_known && count == self.static_entity_slots.len()) {
				// set changed (add / despawn / component removal): full rebuild
				self.static_entities_scratch.clear();
				for (e, _) in q.iter(world) {
					self.static_entities_scratch.insert(e);
				}
				// remove slots for entities that are no longer in the world
				self.static_entity_slots
					.retain(|e, _| self.static_entities_scratch.contains(e));
				// assign slots to new static entities (append after existing)
				let mut next_slot = self
					.static_entity_slots
					.values()
					.copied()
					.max()
					.map(|m| m + 1)
					.unwrap_or(0);
				for entity in &self.static_entities_scratch {
					if !self.static_entity_slots.contains_key(entity) {
						self.static_entity_slots.insert(*entity, next_slot);
						next_slot += 1;
					}
				}
				self.static_entity_count = next_slot;
			}
		}

		self.draw_scratch.clear();
		{
			let registry = world.resource::<MeshRegistry>();
			for &(entity, mesh_id, mat_id, model, lm_id, dir_lm_id) in &self.raw_scratch {
				let (color, metallic, roughness, alpha, mat_flags, texset) = registry
					.get_material(lunar_assets::Handle::new(mat_id, 0))
					.map(|m| {
						let mut color = m.base_color;
						color.a = m.alpha;
						let mut flags = if m.shading == lunar_3d::ShadingModel::Unlit {
							1u32
						} else {
							0u32
						};
						// bit 2: material has a normal map (shader gates the
						// tangent-space perturb on it)
						if m.normal_map.is_some() {
							flags |= 4;
						}
						// bit 3: alpha-test cutout. the threshold rides bits 24..31 of the
						// same flags word (quantized to 8 bits), and the entity keeps
						// alpha 1.0 so it routes to the opaque pass with depth writes.
						let alpha = match m.alpha_cutoff {
							Some(cutoff) => {
								flags |= 8
									| (((cutoff.clamp(0.0, 1.0) * 255.0).round() as u32) << 24);
								1.0
							}
							None => m.alpha,
						};
						let texset = [
							m.diffuse.map(|h| h.id()).unwrap_or(u32::MAX),
							m.normal_map.map(|h| h.id()).unwrap_or(u32::MAX),
							m.specular.map(|h| h.id()).unwrap_or(u32::MAX),
						];
						(color, m.metallic, m.roughness, alpha, flags, texset)
					})
					.unwrap_or((Color::WHITE, 0.0, 0.5, 1.0, 0u32, [u32::MAX; 3]));
				if texset != [u32::MAX; 3] && !self.any_material_textures {
					self.any_material_textures = true;
					if self.bindless_supported() {
						log::info!(
							"material textures in use: bindless one-call gpu multi-draw path"
						);
					} else {
						log::info!(
							"material textures in use: one-call gpu multi-draw disabled, \
							 per-batch indirect path active"
						);
					}
				}
				self.mat_texsets.insert(mat_id, texset);
				self.draw_scratch.push((
					entity, mesh_id, mat_id, color, metallic, roughness, model, alpha, mat_flags,
					lm_id, dir_lm_id,
				));
			}
		}
		// sort opaque entities by (mesh_id, mat_id, lm_id, dir_lm_id) so consecutive entities
		// can share VBO/IBO and bind groups, batched into a single draw_indexed call.
		// transparents are sorted separately by depth after this.
		//
		// sort a small (key, source_index) array rather than draw_scratch in place: that moves
		// 24-byte keys through sort_unstable instead of the ~128-byte draw tuples, then gathers
		// each tuple exactly once. draw_scratch ends up identically ordered, so every downstream
		// consumer is unchanged. the keys/gather bufs are reused, so there's no per-frame alloc.
		self.draw_sort_keys.clear();
		self.draw_sort_keys.extend(self.draw_scratch.iter().enumerate().map(
			|(i, &(_, mesh_id, mat_id, _, _, _, _, alpha, _, lm_id, dir_lm_id))| {
				let transparent = if alpha < 1.0 { 1u8 } else { 0u8 };
				(transparent, mesh_id, mat_id, lm_id, dir_lm_id, i as u32)
			},
		));
		self.draw_sort_keys.sort_unstable();
		self.draw_sorted_scratch.clear();
		self.draw_sorted_scratch.extend(
			self.draw_sort_keys
				.iter()
				.map(|&(_, _, _, _, _, i)| self.draw_scratch[i as usize]),
		);
		std::mem::swap(&mut self.draw_scratch, &mut self.draw_sorted_scratch);
	}
}
