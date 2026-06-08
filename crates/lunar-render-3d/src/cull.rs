//! `RenderEngine3d` — frustum/hzb culling and per-frame draw-list assembly.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

// mapped BufferView is not guaranteed u32-aligned, so read without cast
fn mapped_u32s(bytes: &[u8]) -> Vec<u32> {
	bytes
		.chunks_exact(4)
		.map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
		.collect()
}

impl RenderEngine3d {
	/// frustum + HZB occlusion culling for this frame. high tier reads the
	/// previous frame's GPU compute result (no stall) and dispatches this
	/// frame's; mid/low tier does a CPU AABB test. populates `self.frustum_visible`.
	pub(crate) fn cull_entities(&mut self, world: &mut World, view_proj: Mat4, cam_pos: Vec3) {
		// ── frustum cull ─────────────────────────────────────────────────
		// high tier: 1-frame pipelined GPU compute cull.
		//   frame N: read previous frame's staging result (no stall), dispatch this frame's compute.
		//   frame N+1: read frame N's result.
		//   first frame: no prior result — fall through to CPU cull as bootstrap.
		// mid/low tier: CPU test over contiguous CullSoa arrays.
		self.frustum_visible.clear();
		if self.gpu_cull_enabled {
			let (entity_count, frustum_planes) = {
				let frustum = *world.resource::<Frustum>();
				let soa = world.resource::<CullSoa>();
				(soa.entities.len(), frustum.planes)
			};

			// read previous frame's LOD staging result (1-frame pipelined, same as cull)
			if self.lod_staging_pending
				&& entity_count > 0
				&& self.lod_staging_ready.load(Ordering::Acquire)
			{
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

			// read previous frame's staging result — non-blocking, uses AtomicBool set by map_async callback
			if self.cull_staging_pending && entity_count > 0 {
				let _ = self.device.poll(wgpu::PollType::Poll); // fire any completed callbacks
				if self.cull_staging_ready.load(Ordering::Acquire) {
					let prev_count = self.cull_pending_entity_count;
					if let Some(staging_buf) = self.cull_flags_staging.as_ref() {
						{
							let staging_slice = staging_buf.slice(0..(prev_count * 4) as u64);
							let data = staging_slice.get_mapped_range();
							let flags = mapped_u32s(&data);
							let soa = world.resource::<CullSoa>();
							for (i, &entity) in soa.entities.iter().take(prev_count).enumerate() {
								if i < flags.len() && flags[i] != 0 {
									self.frustum_visible.insert(entity);
								}
							}
							self.gpu_cull_flags.clear();
							self.gpu_cull_flags
								.extend_from_slice(&flags[..prev_count.min(flags.len())]);
						}
						staging_buf.unmap();
					}
					self.cull_staging_ready.store(false, Ordering::Release);
					self.cull_staging_pending = false;
				} else {
					// gpu not done yet — use stale gpu_cull_flags from last frame, no stall
					let soa = world.resource::<CullSoa>();
					for (i, &entity) in soa.entities.iter().enumerate() {
						if i < self.gpu_cull_flags.len() && self.gpu_cull_flags[i] != 0 {
							self.frustum_visible.insert(entity);
						}
					}
					self.cull_staging_pending = false;
				}
			}

			// if no prior result yet (first frame), fall back to CPU cull
			if self.frustum_visible.is_empty() && entity_count > 0 {
				let frustum = *world.resource::<Frustum>();
				let soa = world.resource::<CullSoa>();
				for (i, &entity) in soa.entities.iter().enumerate() {
					if frustum.intersects_aabb(soa.centers[i], soa.half_extents[i]) {
						self.frustum_visible.insert(entity);
					}
				}
			}

			// dispatch this frame's GPU cull (result used next frame)
			if entity_count > 0 {
				self.ensure_gpu_cull_resources(entity_count);

				// build per-entity AABB upload data once; the HZB cull below reuses it
				self.cull_aabb_scratch.clear();
				{
					let soa = world.resource::<CullSoa>();
					for i in 0..entity_count {
						let c = soa.centers[i];
						let e = soa.half_extents[i];
						self.cull_aabb_scratch
							.extend_from_slice(&[c.x, c.y, c.z, 0.0, e.x, e.y, e.z, 0.0]);
					}
				}
				let mut frustum_data = [0f32; 32];
				for (p, plane) in frustum_planes.iter().enumerate() {
					frustum_data[p * 4] = plane.x;
					frustum_data[p * 4 + 1] = plane.y;
					frustum_data[p * 4 + 2] = plane.z;
					frustum_data[p * 4 + 3] = plane.w;
				}
				frustum_data[24] = f32::from_bits(entity_count as u32);

				// ensure LOD buffers before borrowing aabb_buf (borrow checker requirement)
				self.ensure_lod_select_resources(entity_count);

				// (re)build the cull + LOD bind groups only when their backing buffers regrew;
				// the ensure_* paths reset these to None on growth. done before the local buffer
				// borrows below so the mutable self writes don't clash with them.
				if self.cull_bg.is_none() {
					self.cull_bg =
						Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
							label: Some("[cull] bg"),
							layout: self.cull_bgl.as_ref().unwrap(),
							entries: &[
								wgpu::BindGroupEntry {
									binding: 0,
									resource:
										self.cull_aabb_buf.as_ref().unwrap().as_entire_binding(),
								},
								wgpu::BindGroupEntry {
									binding: 1,
									resource:
										self.cull_frustum_buf.as_ref().unwrap().as_entire_binding(),
								},
								wgpu::BindGroupEntry {
									binding: 2,
									resource:
										self.cull_flags_buf.as_ref().unwrap().as_entire_binding(),
								},
							],
						}));
				}
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

				let aabb_buf = self.cull_aabb_buf.as_ref().unwrap();
				let frustum_buf = self.cull_frustum_buf.as_ref().unwrap();
				let flags_buf = self.cull_flags_buf.as_ref().unwrap();
				let staging_buf = self.cull_flags_staging.as_ref().unwrap();
				let bg = self.cull_bg.as_ref().unwrap();

				self.queue
					.write_buffer(aabb_buf, 0, bytemuck::cast_slice(&self.cull_aabb_scratch));
				self.queue
					.write_buffer(frustum_buf, 0, bytemuck::cast_slice(&frustum_data));
				let mut cull_enc =
					self.device
						.create_command_encoder(&wgpu::CommandEncoderDescriptor {
							label: Some("[cull] encoder"),
						});
				{
					let mut cpass = cull_enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
						label: Some("[cull] pass"),
						timestamp_writes: None,
					});
					cpass.set_pipeline(self.cull_pipeline.as_ref().unwrap());
					cpass.set_bind_group(0, bg, &[]);
					cpass.dispatch_workgroups((entity_count as u32).div_ceil(64), 1, 1);
				}
				// also dispatch LOD selection in the same encoder (reuses the cached bind group)
				if let (Some(lod_pipeline), Some(lod_params_buf), Some(lod_buf), Some(lod_bg)) = (
					self.lod_select_pipeline.as_ref(),
					self.lod_params_buf.as_ref(),
					self.lod_indices_buf.as_ref(),
					self.lod_select_bg.as_ref(),
				) {
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
					self.queue.write_buffer(
						lod_params_buf,
						0,
						bytemuck::cast_slice(&lod_params_data),
					);
					{
						let mut lpass = cull_enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
							label: Some("[lod select] pass"),
							timestamp_writes: None,
						});
						lpass.set_pipeline(lod_pipeline);
						lpass.set_bind_group(0, lod_bg, &[]);
						lpass.dispatch_workgroups((entity_count as u32).div_ceil(64), 1, 1);
					}
					if let Some(lod_staging) = self.lod_indices_staging.as_ref() {
						cull_enc.copy_buffer_to_buffer(
							lod_buf,
							0,
							lod_staging,
							0,
							(entity_count * 4) as u64,
						);
					}
				}

				cull_enc.copy_buffer_to_buffer(
					flags_buf,
					0,
					staging_buf,
					0,
					(entity_count * 4) as u64,
				);
				self.queue.submit([cull_enc.finish()]);
				// register map_async for next frame — callback fires when GPU finishes, no CPU stall
				let ready = self.cull_staging_ready.clone();
				ready.store(false, Ordering::Release);
				staging_buf.slice(0..(entity_count * 4) as u64).map_async(
					wgpu::MapMode::Read,
					move |result| {
						if result.is_ok() {
							ready.store(true, Ordering::Release);
						}
					},
				);
				self.cull_staging_pending = true;
				self.cull_pending_entity_count = entity_count;

				// register LOD staging map_async for next frame
				if let Some(lod_staging) = self.lod_indices_staging.as_ref() {
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
		} else {
			let frustum = *world.resource::<Frustum>();
			let soa = world.resource::<CullSoa>();
			#[cfg(not(target_arch = "wasm32"))]
			{
				use rayon::prelude::*;
				let n = soa.entities.len();
				let visible: Vec<Entity> = (0..n)
					.into_par_iter()
					.filter_map(|i| {
						if frustum.intersects_aabb(soa.centers[i], soa.half_extents[i]) {
							Some(soa.entities[i])
						} else {
							None
						}
					})
					.collect();
				self.frustum_visible.extend(visible);
			}
			#[cfg(target_arch = "wasm32")]
			for (i, &entity) in soa.entities.iter().enumerate() {
				if frustum.intersects_aabb(soa.centers[i], soa.half_extents[i]) {
					self.frustum_visible.insert(entity);
				}
			}
		}

		// ── HZB occlusion cull (high tier, 1-frame pipelined) ────────────
		// applies previous frame's occlusion result to frustum_visible, then
		// dispatches this frame's occlusion compute for next frame's use.
		// no CPU stall — the previous frame's compute completed while we were
		// building the draw list.
		if self.hzb_enabled && self.hzb_texture.is_some() {
			let entity_count = {
				let soa = world.resource::<CullSoa>();
				soa.entities.len()
			};
			if entity_count > 0 {
				self.ensure_hzb_cull_buffers(entity_count);

				// read previous frame's occlusion result — non-blocking
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

				// dispatch this frame's HZB occlusion compute
				if !self.gpu_cull_flags.is_empty() {
					// reuse the AABB data built above for the frustum cull (same CullSoa order)
					let vp_array = view_proj.to_cols_array();
					let mut params_data = [0f32; 24];
					params_data[..16].copy_from_slice(&vp_array);
					params_data[16] = self.surface_config.width as f32;
					params_data[17] = self.surface_config.height as f32;
					params_data[18] = f32::from_bits(self.hzb_mip_count);
					params_data[19] = f32::from_bits(entity_count as u32);

					let n = entity_count.min(self.gpu_cull_flags.len());
					self.queue.write_buffer(
						self.hzb_occ_buf.as_ref().unwrap(),
						0,
						bytemuck::cast_slice(&self.gpu_cull_flags[..n]),
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
			let visible_leaves = level.visible_leaves(leaf);
			let area_map = level.area_map();
			for leaf_idx in &visible_leaves {
				if let Ok(pos) = area_map.binary_search_by_key(&(*leaf_idx as u32), |&(li, _)| li) {
					self.bsp_visible_scratch.insert(area_map[pos].1);
				}
			}
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
			let mut q = world.query::<(
				Entity,
				&Mesh3d,
				&Material3d,
				&WorldTransform3d,
				&ComputedVisibility,
				Option<&Aabb3d>,
				Option<&MeshLod>,
				Option<&MeshImpostor>,
				Option<&Area>,
				Option<&Lightmap>,
				Option<&DirectionalLightmap>,
				Option<&PrevWorldTransform3d>,
			)>();
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
						// SIMD distance² (Vec3A) — this runs per visible renderable, hot path
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

						// normal mesh draw — GPU LOD index (1-frame pipelined) or CPU dist fallback
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
			self.static_entities_scratch.clear();
			let mut q = world.query::<(Entity, &StaticMesh)>();
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

		self.draw_scratch.clear();
		{
			let registry = world.resource::<MeshRegistry>();
			for &(entity, mesh_id, mat_id, model, lm_id, dir_lm_id) in &self.raw_scratch {
				let (color, metallic, roughness, alpha, mat_flags) = registry
					.get_material(lunar_assets::Handle::new(mat_id, 0))
					.map(|m| {
						let mut color = m.base_color;
						color.a = m.alpha;
						let flags = if m.shading == lunar_3d::ShadingModel::Unlit {
							1u32
						} else {
							0u32
						};
						(color, m.metallic, m.roughness, m.alpha, flags)
					})
					.unwrap_or((Color::WHITE, 0.0, 0.5, 1.0, 0u32));
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
