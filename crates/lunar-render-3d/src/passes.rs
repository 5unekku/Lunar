//! `RenderEngine3d` — scene + shadow pass recording.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

impl RenderEngine3d {
    /// build the detail-sprite compute bind group. resolves the density map view
    /// (1×1 fallback until the texture uploads). returns `None` if the layout isn't ready.
    fn build_detail_compute_bg(&self, params_buf: &wgpu::Buffer, inst_buf: &wgpu::Buffer, count_buf: &wgpu::Buffer, density_id: u32) -> Option<wgpu::BindGroup> {
        let compute_bgl = self.detail_sprite_compute_bgl.as_ref()?;
        let density_view = self.surface_tex_cache.get(&density_id).map(|(_, v)| v).unwrap_or(&self.contact_shadow_fallback_view);
        Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[detail sprite] compute bg"),
            layout: compute_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(density_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: inst_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: count_buf.as_entire_binding() },
            ],
        }))
    }

    /// build the detail-sprite render bind group. resolves the atlas view
    /// (1×1 fallback until the texture uploads). returns `None` if the layout isn't ready.
    fn build_detail_render_bg(&self, inst_buf: &wgpu::Buffer, atlas_id: u32) -> Option<wgpu::BindGroup> {
        let render_bgl = self.detail_sprite_bgl.as_ref()?;
        let atlas_view = self.surface_tex_cache.get(&atlas_id).map(|(_, v)| v as &wgpu::TextureView).unwrap_or(&self.contact_shadow_fallback_view);
        Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[detail sprite] render bg"),
            layout: render_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(atlas_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: inst_buf.as_entire_binding() },
            ],
        }))
    }

    /// records the main color pass and the scene passes (surface shaders, terrain,
    /// water, decals, particles, detail sprites) into the HDR target; returns draw count.
    pub(crate) fn record_scene_passes(&mut self, fc: &FrameContext, world: &mut World, encoder: &mut wgpu::CommandEncoder) -> u32 {
        let &FrameContext { view_proj, cam_pos, sky, sky_color, dir_direction, dir_illuminance, dir_enabled, vp_x, vp_y, vp_w, vp_h, .. } = fc;
        let mut draw_calls = 0u32;
        // ── main color pass → HDR texture ───��─────────────────────────────
        // MSAA resolves into the non-MSAA HDR texture; no MSAA renders direct to HDR.
        // composite pass reads the HDR texture and writes to swapchain.

        // ── static RenderBundle recording ─────────────────────────────────
        // rebuild when the static entity set changes or hdr_format/msaa_samples change.
        // the comparison stays per-frame on purpose: the bundle bakes `ENTITY_SLOT_START + i`
        // (draw_scratch index), and those indices shift whenever culling reorders draw_scratch,
        // so a static-set-only dirty flag would leave stale slot bindings. we only avoid the
        // per-frame heap churn here — build into a reused scratch vec and swap instead of clone.
        {
            self.static_list_scratch.clear();
            for (i, entry) in self.draw_scratch.iter().enumerate() {
                if self.static_entity_slots.contains_key(&entry.0) {
                    self.static_list_scratch.push((entry.1, entry.2, entry.9, entry.10, i));
                }
            }
            self.static_list_scratch.sort_unstable();
            let format_changed = self.static_bundle_params != (self.hdr_format, self.msaa_samples);
            let list_changed = self.static_list_scratch != self.static_draw_list;
            if (list_changed || format_changed) && !self.static_list_scratch.is_empty() {
                self.static_bundle_params = (self.hdr_format, self.msaa_samples);
                // adopt the freshly built list; scratch keeps the old vec's capacity for reuse
                std::mem::swap(&mut self.static_draw_list, &mut self.static_list_scratch);
                let new_static_list = &self.static_draw_list;
                let mut benc = self.device.create_render_bundle_encoder(
                    &wgpu::RenderBundleEncoderDescriptor {
                        label: Some("[static] bundle encoder"),
                        color_formats: &[Some(self.hdr_format)],
                        depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                            format: wgpu::TextureFormat::Depth32Float,
                            depth_read_only: false,
                            stencil_read_only: false,
                        }),
                        sample_count: self.msaa_samples,
                        multiview: None,
                    }
                );
                benc.set_bind_group(0, &self.globals_bg, &[]);
                benc.set_bind_group(1, &self.material_bg, &[]);
                benc.set_bind_group(2, &self.entity_bg, &[]);
                benc.set_bind_group(3, &self.lights_bg, &[]);
                benc.set_bind_group(5, &self.cluster_bg_render, &[]);
                let mut last_mesh = u32::MAX;
                let mut last_mat = u32::MAX;
                let mut last_lm = u32::MAX;
                let mut last_dir_lm = u32::MAX;
                let mut group_start_j = 0usize;
                let sn = new_static_list.len();
                let mut j = 0;
                while j <= sn {
                    let (cur_mesh, cur_mat, cur_lm, cur_dir_lm) = if j == sn { (u32::MAX, u32::MAX, u32::MAX, u32::MAX) }
                        else { let (m, mt, lm, dlm, _) = new_static_list[j]; (m, mt, lm, dlm) };
                    let grp_changed = cur_mesh != last_mesh || cur_mat != last_mat || cur_lm != last_lm || cur_dir_lm != last_dir_lm;
                    if grp_changed && j > group_start_j {
                        let slot_i = new_static_list[group_start_j].4;
                        if let Some(gpu) = self.mesh_gpu.get(&last_mesh) {
                            let base = (ENTITY_SLOT_START + slot_i) as u32;
                            benc.draw_indexed(0..gpu.index_count, 0, base..base + (j - group_start_j) as u32);
                        }
                    }
                    if j == sn { break; }
                    if grp_changed
                        && let Some(gpu) = self.mesh_gpu.get(&cur_mesh) {
                            let lm_bg = if cur_lm != u32::MAX {
                                self.lightmap_bg_cache.get(&(cur_lm, cur_dir_lm)).unwrap_or(&self.lightmap_fallback_bg)
                            } else {
                                &self.lightmap_fallback_bg
                            };
                            benc.set_bind_group(4, lm_bg, &[]);
                            benc.set_vertex_buffer(0, gpu.vbuf.slice(..));
                            benc.set_index_buffer(gpu.ibuf.slice(..), gpu.index_fmt);
                            last_mesh = cur_mesh; last_mat = cur_mat; last_lm = cur_lm; last_dir_lm = cur_dir_lm;
                            group_start_j = j;
                        }
                    j += 1;
                }
                self.static_bundle = Some(benc.finish(&wgpu::RenderBundleDescriptor {
                    label: Some("[static] bundle"),
                }));
            } else if self.static_list_scratch.is_empty() {
                self.static_bundle = None;
                self.static_draw_list.clear();
            }
        }

        {
            let (color_target, resolve_target) = match &self.msaa_color_view {
                Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                None => (&self.hdr_view as &wgpu::TextureView, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[frame] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_target,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky_color.r as f64,
                            g: sky_color.g as f64,
                            b: sky_color.b as f64,
                            a: 1.0,
                        }),
                        store: if self.msaa_color_view.is_some() {
                            wgpu::StoreOp::Discard  // MSAA tile memory, not needed after resolve
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // load z-prepass depth on mid/high; clear on low (no prepass)
                        load: if self.render_tier != RenderTier::LowGles {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(1.0)
                        },
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // apply viewport + scissor so this camera only renders to its screen rect.
            // for full-screen cameras (ViewportRect::FULL), these are no-ops with max extents.
            pass.set_viewport(vp_x as f32, vp_y as f32, vp_w as f32, vp_h as f32, 0.0, 1.0);
            pass.set_scissor_rect(vp_x, vp_y, vp_w, vp_h);

            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_bind_group(1, &self.material_bg, &[]);
            pass.set_bind_group(3, &self.lights_bg, &[]);
            // group 4 fallback — sky/sun are unlit and never sample the lightmap, but pipeline requires it bound
            pass.set_bind_group(4, &self.lightmap_fallback_bg, &[]);
            // group 5: clustered lights (same for entire pass)
            pass.set_bind_group(5, &self.cluster_bg_render, &[]);

            // sky pass — unlit, dome always drawn; sun only when sky resource present.
            // entity_bg is set once for the whole pass (covers all slots in storage buffer).
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(2, &self.entity_bg, &[]);
            pass.set_vertex_buffer(0, self.dome_mesh.vbuf.slice(..));
            pass.set_index_buffer(self.dome_mesh.ibuf.slice(..), self.dome_mesh.index_fmt);
            pass.draw_indexed(0..self.dome_mesh.index_count, 0, SLOT_DOME as u32..SLOT_DOME as u32 + 1);
            draw_calls += 1;

            if sky.is_some_and(|s| s.show_sun) {
                pass.set_vertex_buffer(0, self.sun_mesh.vbuf.slice(..));
                pass.set_index_buffer(self.sun_mesh.ibuf.slice(..), self.sun_mesh.index_fmt);
                pass.draw_indexed(0..self.sun_mesh.index_count, 0, SLOT_SUN as u32..SLOT_SUN as u32 + 1);
                draw_calls += 1;
            }

            // static geometry via RenderBundle — near-zero CPU cost per frame
            if let Some(ref bundle) = self.static_bundle {
                pass.execute_bundles(std::iter::once(bundle));
            }

            // opaque PBR pass — entity_bg set once; instance_index selects transform + material.
            pass.set_pipeline(&self.opaque_pipeline);
            pass.set_bind_group(2, &self.entity_bg, &[]);
            if self.gpu_indirect_active() {
                // phase 4: GPU cull wrote draw commands to indirect_buf.
                // bind atlas once (all lightmaps packed into it), bind mega-VBO/IBO, one call.
                // phase 5: render path doesn't use frustum_visible — GPU handles culling entirely.
                let atlas_bg = self.atlas_bg.as_ref().unwrap_or(&self.lightmap_fallback_bg);
                pass.set_bind_group(4, atlas_bg, &[]);
                let mega_vbuf = self.mega_vbuf.as_ref().unwrap();
                let mega_ibuf = self.mega_ibuf.as_ref().unwrap();
                let indirect_buf = self.indirect_buf.as_ref().unwrap();
                let count_buf = self.cull_indirect_count_buf.as_ref().unwrap();
                pass.set_vertex_buffer(0, mega_vbuf.slice(..));
                pass.set_index_buffer(mega_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                let max_draws = self.draw_scratch.len() as u32;
                pass.multi_draw_indexed_indirect_count(indirect_buf, 0, count_buf, 0, max_draws);
                draw_calls += 1; // one logical draw (multi-draw)
            } else {
                // phase 2 / non-GPU-driven: per-batch draw_indexed or draw_indexed_indirect
                let mut last_mesh: u32 = u32::MAX;
                let mut last_mat: u32 = u32::MAX;
                let mut last_lm: u32 = u32::MAX;
                let mut last_dir_lm: u32 = u32::MAX;
                let mut group_start: usize = 0;
                let mut opaque_batch_idx: u64 = 0;
                let n = self.draw_scratch.len();
                let mut i = 0;
                while i <= n {
                    let flush = i == n || self.draw_scratch[i].7 < 1.0;
                    let (cur_mesh, cur_mat, cur_lm, cur_dir_lm) = if flush || i == n {
                        (u32::MAX, u32::MAX, u32::MAX, u32::MAX)
                    } else {
                        (self.draw_scratch[i].1, self.draw_scratch[i].2, self.draw_scratch[i].9, self.draw_scratch[i].10)
                    };
                    let group_changed = cur_mesh != last_mesh || cur_mat != last_mat || cur_lm != last_lm || cur_dir_lm != last_dir_lm;
                    if group_changed && i > group_start {
                        let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) else { group_start = i; i += 1; continue; };
                        if self.has_indirect {
                            if let Some(indirect_buf) = self.indirect_buf.as_ref() {
                                pass.draw_indexed_indirect(indirect_buf, opaque_batch_idx * 20);
                            }
                            opaque_batch_idx += 1;
                        } else {
                            let base = (ENTITY_SLOT_START + group_start) as u32;
                            let count = (i - group_start) as u32;
                            pass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + count);
                        }
                        draw_calls += 1;
                    }
                    if flush { break; }
                    if cur_mesh != last_mesh || cur_mat != last_mat || cur_lm != last_lm || cur_dir_lm != last_dir_lm {
                        let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) else { i += 1; continue; };
                        let lm_bg = if cur_lm != u32::MAX {
                            self.lightmap_bg_cache.get(&(cur_lm, cur_dir_lm)).unwrap_or(&self.lightmap_fallback_bg)
                        } else {
                            &self.lightmap_fallback_bg
                        };
                        pass.set_bind_group(4, lm_bg, &[]);
                        pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                        pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                        last_mesh = cur_mesh; last_mat = cur_mat; last_lm = cur_lm; last_dir_lm = cur_dir_lm; group_start = i;
                    }
                    i += 1;
                }
            }

            // transparent pass — back-to-front sorted, no depth write, alpha blend.
            // transparents are few so no batching needed; entity_bg already set.
            if !self.transparent_scratch.is_empty() {
                pass.set_pipeline(&self.transparent_pipeline);
                for &i in &self.transparent_scratch {
                    let mesh_id = self.draw_scratch[i].1;
                    let lm_id = self.draw_scratch[i].9;
                    let dir_lm_id = self.draw_scratch[i].10;
                    let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                    let lm_bg = if lm_id != u32::MAX {
                        self.lightmap_bg_cache.get(&(lm_id, dir_lm_id)).unwrap_or(&self.lightmap_fallback_bg)
                    } else {
                        &self.lightmap_fallback_bg
                    };
                    pass.set_bind_group(4, lm_bg, &[]);
                    pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                    pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                    let base = (ENTITY_SLOT_START + i) as u32;
                    pass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + 1);
                    draw_calls += 1;
                }
            }
        }

        // ── surface shader pass (q3-style multi-stage surfaces) ─────────
        if !self.surface_scratch.is_empty() {
            let (color_target, resolve_target) = match &self.msaa_color_view {
                Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                None => (&self.hdr_view as &wgpu::TextureView, None),
            };
            let mut surf_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[surface] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_target, resolve_target,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            surf_pass.set_pipeline(&self.surface_pipeline);
            surf_pass.set_bind_group(0, &self.globals_bg, &[]);
            surf_pass.set_bind_group(1, &self.entity_bg, &[]);
            let draw_base_slot = ENTITY_SLOT_START + self.draw_scratch.len();
            for &(entity, slot, tex_ids, _) in &self.surface_scratch {
                let Some(bg) = self.surface_bg_cache.get(&tex_ids) else { continue; };
                let surf_offset = ((slot - draw_base_slot) as u64 * UNIFORM_STRIDE) as u32;
                let Some(mesh_comp) = world.get::<Mesh3d>(entity) else { continue; };
                let mesh_id = mesh_comp.0.id();
                let Some(gpu) = self.mesh_gpu.get(&mesh_id) else { continue; };
                surf_pass.set_bind_group(2, bg, &[surf_offset]);
                surf_pass.set_vertex_buffer(0, gpu.vbuf.slice(..));
                surf_pass.set_index_buffer(gpu.ibuf.slice(..), gpu.index_fmt);
                surf_pass.draw_indexed(0..gpu.index_count, 0, slot as u32..slot as u32 + 1);
            }
        }

        // ── terrain pass — geometry clipmap heightmap rendering ─────────
        {
            let mut terrain_query = world.query::<(Entity, &mut Terrain, &WorldTransform3d)>();
            let terrain_entities: Vec<(Entity, Terrain, WorldTransform3d)> = terrain_query
                .iter_mut(world)
                .map(|(e, t, wt)| (e, t.clone(), *wt))
                .collect();

            for (entity, terrain_comp, wt) in &terrain_entities {
                // lazy-init GPU resources on first encounter or if dirty
                let needs_rebuild = {
                    let entry = self.terrain_gpu.get(entity);
                    entry.is_none() || terrain_comp.dirty
                };
                if needs_rebuild {
                    let gpu = Self::build_terrain_gpu(
                        &self.device,
                        &self.queue,
                        &self.terrain_params_bgl,
                        terrain_comp,
                    );
                    self.terrain_gpu.insert(*entity, gpu);
                    // mark clean on the actual component
                    if let Some(mut t) = world.get_mut::<Terrain>(*entity) {
                        t.dirty = false;
                    }
                }
                let Some(gpu) = self.terrain_gpu.get(entity) else { continue; };

                let terrain_origin = wt.translation;
                let world_size = terrain_comp.world_size;
                let rings = terrain_comp.clipmap_rings.clamp(1, 8);
                let resolution = terrain_comp.ring_resolution.clamp(4, 256) as f32;

                // on low tier render a single LOD-0 patch covering the whole terrain
                let effective_rings = if self.render_tier == RenderTier::LowGles { 1 } else { rings };

                for ring in 0..effective_rings as usize {
                    let Some(ring_mesh) = gpu.ring_meshes.get(ring) else { continue; };

                    // each ring is 2× coarser than the previous
                    let base_cell = world_size / (resolution * (1 << rings) as f32);
                    let lod_cell_size = base_cell * (1u32 << ring) as f32;

                    // snap ring origin to cell grid around camera
                    let ring_half = resolution * lod_cell_size * 0.5;
                    let ring_origin_x = (cam_pos.x / lod_cell_size).floor() * lod_cell_size - ring_half;
                    let ring_origin_z = (cam_pos.z / lod_cell_size).floor() * lod_cell_size - ring_half;

                    // sun direction from directional light (default to overhead if none)
                    let sun_d = if dir_enabled != 0 { dir_direction } else { Vec3::Y };
                    let (sun_dx, sun_dy, sun_dz, sun_int) = (sun_d.x, sun_d.y, sun_d.z, dir_illuminance.max(1.0));

                    let tint = [terrain_comp.tint.r, terrain_comp.tint.g, terrain_comp.tint.b, terrain_comp.tint.a];

                    let mut data = [0u8; TERRAIN_PARAMS_SIZE as usize];
                    // ring_origin (vec4)
                    let ro: [f32; 4] = [ring_origin_x, 0.0, ring_origin_z, 0.0];
                    data[0..16].copy_from_slice(bytemuck::cast_slice(&ro));
                    // terrain_origin (vec4)
                    let to_arr: [f32; 4] = [terrain_origin.x, terrain_origin.y, terrain_origin.z, 0.0];
                    data[16..32].copy_from_slice(bytemuck::cast_slice(&to_arr));
                    // misc: lod_cell_size, world_size, height_scale, ring_resolution
                    let misc: [f32; 4] = [lod_cell_size, world_size, terrain_comp.height_scale, resolution];
                    data[32..48].copy_from_slice(bytemuck::cast_slice(&misc));
                    // tint (vec4)
                    data[48..64].copy_from_slice(bytemuck::cast_slice(&tint));
                    // sun_dir (vec4)
                    let sun: [f32; 4] = [sun_dx, sun_dy, sun_dz, sun_int];
                    data[64..80].copy_from_slice(bytemuck::cast_slice(&sun));
                    // ambient + pad
                    let amb: [f32; 4] = [0.15, 0.0, 0.0, 0.0];
                    data[80..96].copy_from_slice(bytemuck::cast_slice(&amb));
                    self.queue.write_buffer(&gpu.params_buf, 0, &data);

                    let (color_target, resolve_target) = match &self.msaa_color_view {
                        Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                        None => (&self.hdr_view as &wgpu::TextureView, None),
                    };
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("[terrain] pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: color_target,
                            resolve_target,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.depth_view,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&self.terrain_pipeline);
                    pass.set_bind_group(0, &self.terrain_globals_bg, &[]);
                    pass.set_bind_group(1, &gpu.params_bg, &[]);
                    pass.set_vertex_buffer(0, ring_mesh.vbuf.slice(..));
                    pass.set_index_buffer(ring_mesh.ibuf.slice(..), ring_mesh.index_fmt);
                    pass.draw_indexed(0..ring_mesh.index_count, 0, 0..1);
                    draw_calls += 1;
                }
            }
        }

        // ── water pass — Gerstner wave displacement + refraction (mid+) ──
        if self.render_tier != RenderTier::LowGles {
            let width  = self.surface_config.width as f32;
            let height = self.surface_config.height as f32;
            let mut water_query = world.query::<(&Water, &Mesh3d, &WorldTransform3d)>();
            let water_entities: Vec<(Water, u32, WorldTransform3d)> = water_query
                .iter(world)
                .map(|(w, m, t)| (*w, m.0.id(), *t))
                .collect();

            for (water_comp, mesh_id, wt) in &water_entities {
                let Some(gpu_mesh) = self.mesh_gpu.get(mesh_id) else { continue; };

                let model_cols = wt.to_matrix().to_cols_array();
                // default 4-wave setup: two crossing ocean swells + two small chop waves
                let waves: [[f32; 4]; 4] = [
                    [1.0, 0.0, 12.0, 0.3],   // direction.x, direction.z, wavelength, amplitude
                    [0.7, 0.7, 8.0,  0.2],
                    [0.0, 1.0, 5.0,  0.1],
                    [-0.5, 0.8, 3.0, 0.05],
                ];
                let water_color = [water_comp.water_color.r, water_comp.water_color.g, water_comp.water_color.b, water_comp.water_color.a];
                let deep_color  = [water_comp.deep_color.r, water_comp.deep_color.g, water_comp.deep_color.b, water_comp.deep_color.a];

                let mut data = [0u8; WATER_PARAMS_SIZE as usize];
                for (i, w) in waves.iter().enumerate() {
                    data[i*16..i*16+16].copy_from_slice(bytemuck::cast_slice(w));
                }
                data[64..128].copy_from_slice(bytemuck::cast_slice(&model_cols));
                data[128..144].copy_from_slice(bytemuck::cast_slice(&water_color));
                data[144..160].copy_from_slice(bytemuck::cast_slice(&deep_color));
                let misc: [f32; 8] = [
                    water_comp.refract_strength,
                    water_comp.wave_speed,
                    water_comp.fresnel_power,
                    width, height,
                    0.0, 0.0, 0.0,
                ];
                data[160..192].copy_from_slice(bytemuck::cast_slice(&misc));
                self.queue.write_buffer(&self.water_params_buf, 0, &data);

                let (color_target, resolve_target) = match &self.msaa_color_view {
                    Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                    None => (&self.hdr_view as &wgpu::TextureView, None),
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[water] pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color_target,
                        resolve_target,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.water_pipeline);
                pass.set_bind_group(0, &self.water_bg0, &[]);
                pass.set_bind_group(1, &self.water_bg1, &[]);
                pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                draw_calls += 1;
            }
        }

        // ── decal pass — box-projected over scene depth ───────────────────
        {
            let width  = self.surface_config.width as f32;
            let height = self.surface_config.height as f32;
            let inv_vp = view_proj.inverse();
            let inv_vp_cols = inv_vp.to_cols_array();
            let vp_cols = view_proj.to_cols_array();

            let mut decal_query = world.query::<(&Decal, &WorldTransform3d)>();
            let decals: Vec<(Decal, WorldTransform3d)> = decal_query
                .iter(world)
                .map(|(d, wt)| (*d, *wt))
                .collect();

            for (decal, wt) in &decals {
                let decal_world_mat = wt.to_matrix();
                let decal_inv_world = decal_world_mat.inverse();
                let decal_world_cols = decal_world_mat.to_cols_array();
                let inv_world_cols  = decal_inv_world.to_cols_array();

                let mut data = [0u8; DECAL_PARAMS_SIZE as usize];
                data[0..64].copy_from_slice(bytemuck::cast_slice(&inv_world_cols));
                data[64..128].copy_from_slice(bytemuck::cast_slice(&inv_vp_cols));
                let color_arr: [f32; 4] = [decal.color.r, decal.color.g, decal.color.b, decal.color.a];
                data[128..144].copy_from_slice(bytemuck::cast_slice(&color_arr));
                data[144..208].copy_from_slice(bytemuck::cast_slice(&decal_world_cols));
                let _ = vp_cols; // available if needed by future extensions
                let misc: [f32; 4] = [width, height, 0.0, 0.0];
                data[208..224].copy_from_slice(bytemuck::cast_slice(&misc));
                self.queue.write_buffer(&self.decal_params_buf, 0, &data);

                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[decal] pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.hdr_view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.decal_pipeline);
                pass.set_bind_group(0, &self.decal_bg0, &[]);
                pass.set_bind_group(1, &self.decal_bg1, &[]);
                pass.draw(0..36, 0..1);
                draw_calls += 1;
            }
        }

        // ── particle pass (mid+ tier: compute sim → billboard render) ────
        if self.particles_enabled {
            let delta = world.resource::<lunar_core::Time>().delta_seconds();

            // gather emitters from ECS and manage CPU-side spawn
            let mut emitter_query = world.query::<(&ParticleEmitter, &WorldTransform3d)>();
            let mut to_spawn: Vec<CpuParticle> = Vec::new();
            for (emitter, wt) in emitter_query.iter(world) {
                if !emitter.active { continue; }
                let new_count = ((emitter.emission_rate * delta) as u32).min(emitter.max_particles);
                let pos = wt.translation;
                let fwd = wt.forward();
                for n in 0..new_count {
                    let angle = emitter.spread_angle;
                    let t = n as f32 / new_count.max(1) as f32;
                    let theta = t * std::f32::consts::TAU;
                    let spread = Vec3::new(theta.cos() * angle, 0.0, theta.sin() * angle);
                    let direction = (fwd + spread).normalize();
                    to_spawn.push(CpuParticle {
                        position: pos,
                        velocity: direction * emitter.initial_speed,
                        lifetime: emitter.particle_lifetime,
                        max_lifetime: emitter.particle_lifetime,
                        color_start: [emitter.color_start.r, emitter.color_start.g, emitter.color_start.b, emitter.color_start.a],
                        color_end: [emitter.color_end.r, emitter.color_end.g, emitter.color_end.b, emitter.color_end.a],
                        size_start: emitter.size_start,
                        size_end: emitter.size_end,
                        alive: true,
                    });
                }
            }

            // fill dead slots with newly spawned particles
            let mut new_gpu_writes: Vec<(u32, GpuParticle)> = Vec::new();
            let mut spawn_iter = to_spawn.into_iter();
            for (slot, cpu) in self.particle_cpu.iter_mut().enumerate() {
                if cpu.alive { continue; }
                let Some(spawned) = spawn_iter.next() else { break; };
                new_gpu_writes.push((slot as u32, spawned.as_gpu()));
                *cpu = spawned;
            }

            // upload newly spawned particles to their slots in the storage buffer
            for (slot, gpu_particle) in &new_gpu_writes {
                let offset = *slot as u64 * PARTICLE_STRIDE;
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        gpu_particle as *const GpuParticle as *const u8,
                        PARTICLE_STRIDE as usize,
                    )
                };
                self.queue.write_buffer(&self.particle_buf, offset, bytes);
            }

            // count alive particles (after CPU lifetime update that happens via compute)
            let alive_count = self.particle_cpu.iter().filter(|p| p.alive).count() as u32;
            if alive_count > 0 {
                let gravity = 9.8_f32;
                let sim_params: [f32; 4] = [delta, gravity, f32::from_bits(alive_count), 0.0];
                self.queue.write_buffer(&self.particle_sim_params_buf, 0, bytemuck::cast_slice(&sim_params));

                // compute pass: simulate alive particles
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("[particles] sim pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(&self.particle_sim_pipeline);
                cpass.set_bind_group(0, &self.particle_sim_bg, &[]);
                let wg = alive_count.div_ceil(64);
                cpass.dispatch_workgroups(wg, 1, 1);
                drop(cpass);

                // particle render pass: billboard quads into HDR (alpha-blended, MSAA)
                let (color_target, resolve_target) = match &self.msaa_color_view {
                    Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                    None => (&self.hdr_view as &wgpu::TextureView, None),
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[particles] render pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color_target,
                        resolve_target,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.particle_render_pipeline);
                pass.set_bind_group(0, &self.particle_render_bg, &[]);
                pass.draw(0..6, 0..alive_count);
                draw_calls += 1;
            }

            // update CPU lifetime state (particles were simulated on GPU; mirror the aging here)
            for cpu in &mut self.particle_cpu {
                if cpu.alive {
                    cpu.lifetime -= delta;
                    if cpu.lifetime <= 0.0 {
                        cpu.alive = false;
                    }
                }
            }
        }

        // ── detail sprite pass ────────────────────────────────────────────
        // gpu-driven billboarded ground cover — compute generates instances, render draws them.
        {
            let mut detail_query = world.query::<(bevy_ecs::entity::Entity, &DetailDensity, &WorldTransform3d, &ComputedVisibility)>();
            let detail_entities: Vec<(bevy_ecs::entity::Entity, DetailDensity, f32)> = detail_query
                .iter(world)
                .filter(|(_, _, _, vis)| vis.0)
                .map(|(e, dd, wt, _)| (e, dd.clone(), wt.translation.y))
                .collect();

            if !detail_entities.is_empty() {
                self.ensure_detail_sprite_resources();
                const MAX_SPRITES: u32 = 4096;
                const INSTANCE_STRIDE: u64 = 32; // SpriteInstance = 8 × f32 = 32 bytes

                for (entity, dd, _base_y) in &detail_entities {
                    let entity_key = entity.to_bits();
                    let density_id = dd.density_map.id();
                    let atlas_id   = dd.texture.id();
                    // (id, uploaded yet?) — bind groups must rebuild when the resolved view flips
                    let density_key = (density_id, self.surface_tex_cache.contains_key(&density_id));
                    let atlas_key   = (atlas_id,   self.surface_tex_cache.contains_key(&atlas_id));

                    // first sighting: allocate persistent buffers + build both bind groups once
                    if !self.detail_sprite_cache.contains_key(&entity_key) {
                        let inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("[detail sprite] instance buf"),
                            size: MAX_SPRITES as u64 * INSTANCE_STRIDE,
                            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        let count_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("[detail sprite] count buf"),
                            size: 4,
                            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        // draw indirect: [vertex_count=4, instance_count, first_vertex=0, first_instance=0]
                        let draw_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("[detail sprite] draw buf"),
                            size: 16,
                            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        // persistent compute params uniform (48 bytes used; re-written each frame)
                        let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("[detail sprite] compute params"),
                            size: 64,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        let (Some(compute_bg), Some(render_bg)) = (
                            self.build_detail_compute_bg(&params_buf, &inst_buf, &count_buf, density_id),
                            self.build_detail_render_bg(&inst_buf, atlas_id),
                        ) else { continue };
                        self.detail_sprite_cache.insert(entity_key, DetailSpriteEntry {
                            inst_buf, count_buf, draw_buf, params_buf, compute_bg, render_bg, density_key, atlas_key,
                        });
                    } else {
                        // rebuild a bind group only when its resolved texture view changed
                        let entry = self.detail_sprite_cache.get(&entity_key).unwrap();
                        if entry.density_key != density_key {
                            let (params_buf, inst_buf, count_buf) = (entry.params_buf.clone(), entry.inst_buf.clone(), entry.count_buf.clone());
                            if let Some(bg) = self.build_detail_compute_bg(&params_buf, &inst_buf, &count_buf, density_id) {
                                let e = self.detail_sprite_cache.get_mut(&entity_key).unwrap();
                                e.compute_bg = bg;
                                e.density_key = density_key;
                            }
                        }
                        let entry = self.detail_sprite_cache.get(&entity_key).unwrap();
                        if entry.atlas_key != atlas_key {
                            let inst_buf = entry.inst_buf.clone();
                            if let Some(bg) = self.build_detail_render_bg(&inst_buf, atlas_id) {
                                let e = self.detail_sprite_cache.get_mut(&entity_key).unwrap();
                                e.render_bg = bg;
                                e.atlas_key = atlas_key;
                            }
                        }
                    }

                    let entry = self.detail_sprite_cache.get(&entity_key).unwrap();

                    // reset count and draw_buf each frame
                    self.queue.write_buffer(&entry.count_buf, 0, &0u32.to_le_bytes());
                    // draw_buf: vertex_count=4 (strip quad), instance_count=0 (filled by copy), first_vertex=0, first_instance=0
                    let draw_init: [u32; 4] = [4, 0, 0, 0];
                    self.queue.write_buffer(&entry.draw_buf, 0, bytemuck::cast_slice(&draw_init));

                    // write compute params into the persistent params buffer
                    let grid_step = dd.grid_step.max(0.1);
                    let grid_count_x = ((dd.world_size.x / grid_step).ceil() as u32).min(256);
                    let grid_count_z = ((dd.world_size.y / grid_step).ceil() as u32).min(256);

                    let compute_params: [f32; 12] = [
                        cam_pos.x, cam_pos.y, cam_pos.z, dd.max_dist,
                        dd.world_origin.x, dd.world_origin.y, dd.world_size.x, dd.world_size.y,
                        grid_step, dd.density_scale, dd.size_range[0], dd.size_range[1],
                    ];
                    // additional u32 fields: variant_count + pad
                    let compute_params_tail: [u32; 2] = [dd.variant_count, 0];
                    self.queue.write_buffer(&entry.params_buf, 0, bytemuck::cast_slice(&compute_params));
                    self.queue.write_buffer(&entry.params_buf, 48, bytemuck::cast_slice(&compute_params_tail));

                    if let Some(compute_pipeline) = self.detail_sprite_compute_pipeline.as_ref() {
                        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("[detail sprite] compute"),
                            timestamp_writes: None,
                        });
                        cpass.set_pipeline(compute_pipeline);
                        cpass.set_bind_group(0, &entry.compute_bg, &[]);
                        cpass.dispatch_workgroups(grid_count_x.div_ceil(8), grid_count_z.div_ceil(8), 1);
                    }

                    // copy instance count → draw_buf instance_count field
                    encoder.copy_buffer_to_buffer(&entry.count_buf, 0, &entry.draw_buf, 4, 4);

                    // render the sprites
                    if let Some(render_pipeline) = self.detail_sprite_pipeline.as_ref() {
                        let (color_target, resolve_target) = match &self.msaa_color_view {
                            Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                            None => (&self.hdr_view as &wgpu::TextureView, None),
                        };
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("[detail sprite] render"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: color_target,
                                resolve_target,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                                depth_slice: None,
                            })],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: &self.depth_view,
                                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                                stencil_ops: None,
                            }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        rpass.set_pipeline(render_pipeline);
                        rpass.set_bind_group(0, &entry.render_bg, &[]);
                        rpass.draw_indirect(&entry.draw_buf, 0);
                        draw_calls += 1;
                    }
                }
            }
        }

        draw_calls
    }
    /// records all shadow passes: point-light cube shadows, then the directional
    /// cascade + z-prepass recording (parallel on native, sequential on wasm).
    // face_dirs / shadow_list indexed loops (one uses a sentinel final iteration) read clearer
    // as indexed loops than iterator adapters; the range-loop lint is intentionally allowed.
    #[allow(clippy::needless_range_loop)]
    pub(crate) fn record_shadows(&mut self, world: &mut World, encoder: &mut wgpu::CommandEncoder, dir_direction: Vec3, dir_enabled: u32, dir_casts_shadows: bool) {
        let dev_shadows          = world.get_resource::<DevRenderProfile>().map(|d| d.shadows         ).unwrap_or(true);
        let dev_max_cascades     = world.get_resource::<DevRenderProfile>().map(|d| d.max_shadow_cascades as usize).unwrap_or(NUM_CASCADES as usize);
        let dev_point_shadows    = world.get_resource::<DevRenderProfile>().map(|d| d.point_light_shadows).unwrap_or(true);

        // ── collect shadow casters ────────────────────────────────────────
        // shadow_list: (mesh_id, draw_scratch_index) for all visible shadow casters.
        // using entity lookup so every caster gets its own correct transform.
        // sorted by mesh_id so consecutive shadow draws can share VBO/IBO.
        let shadow_list: Vec<(u32, usize)> = {
            let shadow_entities: HashSet<Entity> = {
                let mut q = world.query::<(Entity, &ComputedVisibility, &ShadowCaster)>();
                q.iter(world).filter(|(_, vis, _)| vis.0).map(|(e, _, _)| e).collect()
            };
            let mut list: Vec<(u32, usize)> = self.draw_scratch.iter().enumerate()
                .filter(|(_, entry)| shadow_entities.contains(&entry.0))
                .map(|(i, entry)| (entry.1, i))
                .collect();
            list.sort_unstable_by_key(|&(mesh_id, _)| mesh_id);
            list
        };

        // ── dirty-flag shadow cascade invalidation ────────────────────────
        // cascades are re-rendered only when something relevant changed.
        // triggers: light direction changed, draw list size changed (entity added/removed),
        // or any shadow-casting entity's mesh_id changed (proxy for transform change).
        {
            let dir_changed = (dir_direction - self.shadow_last_dir).length_squared() > 1e-6;
            let draw_changed = shadow_list.len() != self.shadow_last_draw_count;
            if dir_changed || draw_changed {
                self.shadow_cascade_dirty = [true; 3];
                self.shadow_last_dir = dir_direction;
                self.shadow_last_draw_count = shadow_list.len();
            }
        }

        // ── point light shadow pass ──────────────────────────────────────
        // for each light with casts_shadows=true (up to MAX_POINT_SHADOW_LIGHTS),
        // render scene into the appropriate 6 face layers of point_shadow_tex.
        if dev_point_shadows {
            // dirty detection: re-render all faces when any light position changes or draw count changes
            let pt_draw_count = self.draw_scratch.len();
            if pt_draw_count != self.point_shadow_last_draw_count {
                for dirty in &mut self.point_shadow_dirty { *dirty = [true; 6]; }
                self.point_shadow_last_draw_count = pt_draw_count;
            }
            let mut pt_shadow_idx = 0usize;
            for (light_i, &(light_pos, _, _, light_radius, casts, _)) in self.point_light_scratch.iter().enumerate() {
                if !casts || pt_shadow_idx >= MAX_POINT_SHADOW_LIGHTS { break; }
                let _ = light_i;
                let lp = Vec3::from(light_pos);
                let last_pos = self.point_shadow_last_positions[pt_shadow_idx];
                if (lp - last_pos).length_squared() > 1e-6 {
                    self.point_shadow_dirty[pt_shadow_idx] = [true; 6];
                    self.point_shadow_last_positions[pt_shadow_idx] = lp;
                }
                // face directions: +X,-X,+Y,-Y,+Z,-Z with their respective up vectors
                let face_dirs: [(Vec3, Vec3); 6] = [
                    (Vec3::X,       -Vec3::Y),
                    (-Vec3::X,      -Vec3::Y),
                    (Vec3::Y,        Vec3::Z),
                    (-Vec3::Y,      -Vec3::Z),
                    (Vec3::Z,       -Vec3::Y),
                    (-Vec3::Z,      -Vec3::Y),
                ];
                let near = 0.05f32;
                let far = light_radius;
                for face in 0..6usize {
                    if !self.point_shadow_dirty[pt_shadow_idx][face] { continue; }
                    let layer = pt_shadow_idx * 6 + face;
                    let (dir, up) = face_dirs[face];
                    let view = Mat4::look_at_rh(Vec3::from(lp), Vec3::from(lp) + dir, up);
                    let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, near, far);
                    let face_vp = proj * view;
                    // upload face VP + light pos + radius to the per-face slot
                    let slot_offset = (layer as u64) * UNIFORM_STRIDE;
                    let mut slot_data = [0u8; UNIFORM_STRIDE as usize];
                    slot_data[..64].copy_from_slice(bytemuck::cast_slice(&face_vp.to_cols_array()));
                    slot_data[64..76].copy_from_slice(bytemuck::cast_slice(&[lp.x, lp.y, lp.z]));
                    slot_data[76..80].copy_from_slice(bytemuck::cast_slice(&[light_radius]));
                    self.queue.write_buffer(&self.point_shadow_globals_buf, slot_offset, &slot_data[..80]);
                    // render shadow casters into this face layer
                    {
                        let mut pt_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("[point shadow] face pass"),
                            color_attachments: &[],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: &self.point_shadow_face_views[layer],
                                depth_ops: Some(wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(1.0),
                                    store: wgpu::StoreOp::Store,
                                }),
                                stencil_ops: None,
                            }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        pt_pass.set_pipeline(&self.point_shadow_pipeline);
                        pt_pass.set_bind_group(0, &self.point_shadow_globals_bg, &[layer as u32 * UNIFORM_STRIDE as u32]);
                        pt_pass.set_bind_group(1, &self.entity_bg, &[]);
                        let mut last_mesh = u32::MAX;
                        let mut last_gs = 0usize;
                        let sn = self.draw_scratch.len();
                        for si in 0..=sn {
                            let cur_mesh = if si == sn { u32::MAX } else { self.draw_scratch[si].1 };
                            if cur_mesh != last_mesh {
                                if si > last_gs
                                    && let Some(gpu) = self.mesh_gpu.get(&last_mesh) {
                                        let base = (ENTITY_SLOT_START + last_gs) as u32;
                                        pt_pass.draw_indexed(0..gpu.index_count, 0, base..base + (si - last_gs) as u32);
                                    }
                                if si < sn {
                                    if let Some(gpu) = self.mesh_gpu.get(&cur_mesh) {
                                        pt_pass.set_vertex_buffer(0, gpu.pos_buf.slice(..));
                                        pt_pass.set_index_buffer(gpu.ibuf.slice(..), gpu.index_fmt);
                                    }
                                    last_mesh = cur_mesh;
                                    last_gs = si;
                                }
                            }
                        }
                    }
                    self.point_shadow_dirty[pt_shadow_idx][face] = false;
                }
                pt_shadow_idx += 1;
            }
            // clear layers for unused shadow slots
            for unused in pt_shadow_idx..MAX_POINT_SHADOW_LIGHTS {
                for face in 0..6usize {
                    let layer = unused * 6 + face;
                    let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("[point shadow] clear unused"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.point_shadow_face_views[layer],
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
            }
        }

        // ── shadow + z-prepass: parallel command recording ───────────────
        // each shadow cascade and the z-prepass get their own CommandEncoder recorded
        // in parallel on a Rayon thread pool. shadow cascades and z-prepass have no
        // read/write conflicts with each other (each writes to a disjoint texture).
        // submitted in order before the main encoder so the opaque pass can use them.
        //
        // SAFETY: closures share a read-only &RenderEngine3d (no writes to self state
        // in the parallel section). each closure writes to a disjoint CommandEncoder.
        {
            // rebuild at most 1 dirty cascade per frame (prioritise cascade 0 — nearest/highest detail).
            // remaining dirty cascades stay dirty and are rebuilt on subsequent frames, spreading
            // the spike across frames. a stale cascade 2 (far, low detail) is imperceptible for 1-2 frames.
            let all_dirty: Vec<usize> = (0..NUM_CASCADES as usize)
                .filter(|&c| dir_enabled != 0 && dir_casts_shadows && dev_shadows && c < dev_max_cascades && self.shadow_cascade_dirty[c])
                .collect();
            let dirty_cascades: Vec<usize> = all_dirty.into_iter().take(1).collect();
            for &c in &dirty_cascades { self.shadow_cascade_dirty[c] = false; }

            // clear skipped cascades on the main encoder (no content change, just clear)
            for cascade in 0..NUM_CASCADES as usize {
                if !dirty_cascades.contains(&cascade) {
                    let label = format!("[shadow] cascade-{cascade}");
                    let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(label.as_str()),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.shadow_cascade_views[cascade],
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
            }

            // record dirty shadow cascades + z-prepass in parallel (native only)
            #[cfg(not(target_arch = "wasm32"))]
            let parallel_cmds = {
                use rayon::prelude::*;
                // total parallel tasks: dirty cascades + 1 if z-prepass needed
                let needs_zprepass = self.render_tier != RenderTier::LowGles;
                let _task_count = dirty_cascades.len() + if needs_zprepass { 1 } else { 0 };
                let mut tasks: Vec<usize> = dirty_cascades.clone(); // cascade indices
                if needs_zprepass { tasks.push(usize::MAX); } // sentinel for z-prepass

                // extract the read-only references needed by all recording closures.
                // all wgpu pipeline/buffer types are Send+Sync on native, so the
                // move closures are Send and rayon can dispatch them across threads.
                let device        = &self.device;
                let shad_pl       = &self.shadow_pipeline;
                let shad_gbg      = &self.shadow_globals_bg;
                let ent_bg        = &self.entity_bg;
                let casc_views    = &self.shadow_cascade_views;
                let mesh_gpu      = &self.mesh_gpu;
                let zpr_pl        = &self.zprepass_pipeline;
                let glob_bg       = &self.globals_bg;
                let lights_bg_ref = &self.lights_bg;
                let mat_bg        = &self.material_bg;
                let depth_vw      = &self.depth_view;
                let draw_ref      = &self.draw_scratch;
                tasks.par_iter().map(move |&task| {
                    // shadow_list is owned locally and shared by reference across tasks
                    let s_device     = device;
                    let s_shad_pl    = shad_pl;
                    let s_shad_gbg   = shad_gbg;
                    let s_ent_bg     = ent_bg;
                    let s_casc       = casc_views;
                    let s_mesh_gpu   = mesh_gpu;
                    let s_zpr_pl     = zpr_pl;
                    let s_glob_bg    = glob_bg;
                    let s_lights     = lights_bg_ref;
                    let s_mat_bg     = mat_bg;
                    let s_depth      = depth_vw;
                    let s_draw       = draw_ref;
                    let label = if task == usize::MAX { "[z-prepass]".to_string() } else { format!("[shadow] cascade-{task}") };
                    let mut enc = s_device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(&label) });
                    if task == usize::MAX {
                        let mut zpass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("[z-prepass]"),
                            color_attachments: &[],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: s_depth,
                                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                                stencil_ops: None,
                            }),
                            timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                        });
                        zpass.set_pipeline(s_zpr_pl);
                        zpass.set_bind_group(0, s_glob_bg, &[]);
                        zpass.set_bind_group(1, s_mat_bg, &[]);
                        zpass.set_bind_group(2, s_ent_bg, &[]);
                        zpass.set_bind_group(3, s_lights, &[]);
                        let n = s_draw.len();
                        let mut last_mesh = u32::MAX; let mut last_mat = u32::MAX; let mut group_start = 0usize;
                        let mut i = 0usize;
                        while i <= n {
                            let done = i == n;
                            let (cur_mesh, cur_mat) = if done { (u32::MAX, u32::MAX) } else { (s_draw[i].1, s_draw[i].2) };
                            if (cur_mesh != last_mesh || cur_mat != last_mat) && i > group_start
                                && let Some(gpu) = s_mesh_gpu.get(&last_mesh) {
                                    let base = (ENTITY_SLOT_START + group_start) as u32;
                                    zpass.draw_indexed(0..gpu.index_count, 0, base..base + (i - group_start) as u32);
                                }
                            if done { break; }
                            if cur_mesh != last_mesh || cur_mat != last_mat {
                                if let Some(gpu) = s_mesh_gpu.get(&cur_mesh) {
                                    zpass.set_vertex_buffer(0, gpu.pos_buf.slice(..));
                                    zpass.set_index_buffer(gpu.ibuf.slice(..), gpu.index_fmt);
                                }
                                last_mesh = cur_mesh; last_mat = cur_mat; group_start = i;
                            }
                            i += 1;
                        }
                    } else {
                        let cascade = task;
                        let mut spass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some(&label),
                            color_attachments: &[],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: &s_casc[cascade],
                                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                                stencil_ops: None,
                            }),
                            timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                        });
                        spass.set_pipeline(s_shad_pl);
                        spass.set_bind_group(0, s_shad_gbg, &[Self::slot_offset(cascade)]);
                        spass.set_bind_group(1, s_ent_bg, &[]);
                        let mut last_mesh = u32::MAX; let mut gs_slot = 0usize; let mut gs_idx = 0usize;
                        let sn = shadow_list.len();
                        for idx in 0..=sn {
                            let done = idx == sn;
                            let cur_mesh = if done { u32::MAX } else { shadow_list[idx].0 };
                            if cur_mesh != last_mesh && idx > gs_idx
                                && let Some(gpu) = s_mesh_gpu.get(&last_mesh) {
                                    let base = (ENTITY_SLOT_START + gs_slot) as u32;
                                    spass.draw_indexed(0..gpu.index_count, 0, base..base + (idx - gs_idx) as u32);
                                }
                            if done { break; }
                            if cur_mesh != last_mesh {
                                if let Some(gpu) = s_mesh_gpu.get(&cur_mesh) {
                                    spass.set_vertex_buffer(0, gpu.pos_buf.slice(..));
                                    spass.set_index_buffer(gpu.ibuf.slice(..), gpu.index_fmt);
                                }
                                last_mesh = cur_mesh; gs_slot = shadow_list[idx].1; gs_idx = idx;
                            }
                        }
                    }
                    enc.finish()
                }).collect::<Vec<_>>()
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                if !parallel_cmds.is_empty() {
                    self.queue.submit(parallel_cmds);
                }
            }

            // WASM: sequential shadow + z-prepass on the main encoder
            #[cfg(target_arch = "wasm32")]
            {
                for &cascade in &dirty_cascades {
                    let label = format!("[shadow] cascade-{cascade}");
                    let mut sp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(label.as_str()),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.shadow_cascade_views[cascade],
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                    });
                    sp.set_pipeline(&self.shadow_pipeline);
                    sp.set_bind_group(0, &self.shadow_globals_bg, &[Self::slot_offset(cascade)]);
                    sp.set_bind_group(1, &self.entity_bg, &[]);
                    let mut last_mesh = u32::MAX; let mut gs_slot = 0usize; let mut gs_idx = 0usize;
                    let sn = shadow_list.len();
                    for idx in 0..=sn {
                        let done = idx == sn;
                        let cur_mesh = if done { u32::MAX } else { shadow_list[idx].0 };
                        if cur_mesh != last_mesh && idx > gs_idx {
                            if let Some(gpu) = self.mesh_gpu.get(&last_mesh) {
                                let base = (ENTITY_SLOT_START + gs_slot) as u32;
                                sp.draw_indexed(0..gpu.index_count, 0, base..base + (idx - gs_idx) as u32);
                            }
                        }
                        if done { break; }
                        if cur_mesh != last_mesh {
                            if let Some(gpu) = self.mesh_gpu.get(&cur_mesh) {
                                sp.set_vertex_buffer(0, gpu.pos_buf.slice(..));
                                sp.set_index_buffer(gpu.ibuf.slice(..), gpu.index_fmt);
                            }
                            last_mesh = cur_mesh; gs_slot = shadow_list[idx].1; gs_idx = idx;
                        }
                    }
                }
                if self.render_tier != RenderTier::LowGles {
                    let mut zpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("[z-prepass]"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.depth_view,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                    });
                    zpass.set_pipeline(&self.zprepass_pipeline);
                    zpass.set_bind_group(0, &self.globals_bg, &[]);
                    zpass.set_bind_group(1, &self.material_bg, &[]);
                    zpass.set_bind_group(2, &self.entity_bg, &[]);
                    zpass.set_bind_group(3, &self.lights_bg, &[]);
                    let n = self.draw_scratch.len();
                    let mut last_mesh = u32::MAX; let mut last_mat = u32::MAX; let mut group_start = 0usize;
                    let mut i = 0usize;
                    while i <= n {
                        let done = i == n;
                        let (cur_mesh, cur_mat) = if done { (u32::MAX, u32::MAX) } else { (self.draw_scratch[i].1, self.draw_scratch[i].2) };
                        if (cur_mesh != last_mesh || cur_mat != last_mat) && i > group_start {
                            if let Some(gpu) = self.mesh_gpu.get(&last_mesh) {
                                let base = (ENTITY_SLOT_START + group_start) as u32;
                                zpass.draw_indexed(0..gpu.index_count, 0, base..base + (i - group_start) as u32);
                            }
                        }
                        if done { break; }
                        if cur_mesh != last_mesh || cur_mat != last_mat {
                            if let Some(gpu) = self.mesh_gpu.get(&cur_mesh) {
                                zpass.set_vertex_buffer(0, gpu.pos_buf.slice(..));
                                zpass.set_index_buffer(gpu.ibuf.slice(..), gpu.index_fmt);
                            }
                            last_mesh = cur_mesh; last_mat = cur_mat; group_start = i;
                        }
                        i += 1;
                    }
                }
            }
        }
    }
}
