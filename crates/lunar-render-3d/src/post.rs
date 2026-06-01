//! `RenderEngine3d` — post-processing and gtao/reflection recording.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

impl RenderEngine3d {
    /// records the post-processing chain (bloom, atmospheric sky, ssr, fog, contact
    /// shadow, motion vectors, composite, taa, fxaa) into the frame encoder, ending at
    /// the swapchain. `view` is the surface view (moved in; not used after).
    pub(crate) fn record_post_processing(&mut self, fc: &FrameContext, world: &World, encoder: &mut wgpu::CommandEncoder, view: wgpu::TextureView) {
        let &FrameContext { view_proj, staa_jitter_ndc, cam_pos, cam_wt, dev_bloom, dev_ssao, dev_ssr, dev_fog, dev_fxaa, dev_staa, dev_vignette, dev_chrom_ab, dev_film_grain, dev_contact_shadows, upscale_mode, dir_color, dir_direction, sky_color, .. } = fc;
        // ── bloom passes ─────────────────────────────────────────────────
        if self.bloom_enabled && dev_bloom && !self.bloom_mip_views.is_empty() {
            let n = self.bloom_mip_views.len();

            // upload bloom params for all steps (downsample + upsample)
            let bloom_threshold = 1.0_f32;
            let filter_radius = 1.0_f32;
            let total_steps = n + n.saturating_sub(1);
            for i in 0..total_steps.min(MAX_BLOOM_MIPS) {
                let (src_w, src_h) = if i < n {
                    // downsample: src is HDR (step 0) or previous mip
                    if i == 0 { (self.surface_config.width, self.surface_config.height) }
                    else { self.bloom_mip_sizes[i - 1] }
                } else {
                    // upsample: src is the mip being read (larger index)
                    let up_step = i - n;
                    self.bloom_mip_sizes[n - 1 - up_step]
                };
                let threshold = if i == 0 { bloom_threshold } else { 0.0 };
                let params: [f32; 4] = [
                    1.0 / src_w as f32,
                    1.0 / src_h as f32,
                    filter_radius,
                    threshold,
                ];
                self.queue.write_buffer(
                    &self.bloom_params_buf,
                    (i * UNIFORM_STRIDE as usize) as u64,
                    bytemuck::cast_slice(&params),
                );
            }

            // downsample: HDR → mip0 → mip1 → ... → mip(n-1)
            for i in 0..n {
                let (dst_w, dst_h) = self.bloom_mip_sizes[i];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[bloom] downsample"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.bloom_mip_views[i],
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, dst_w as f32, dst_h as f32, 0.0, 1.0);
                pass.set_pipeline(&self.bloom_downsample_pipeline);
                pass.set_bind_group(0, &self.bloom_downsample_bgs[i], &[]);
                pass.draw(0..3, 0..1);
            }

            // upsample: mip(n-1) → mip(n-2) → ... → mip0 (additive blend)
            for i in 0..self.bloom_upsample_bgs.len() {
                let dst_idx = n - 2 - i;
                let (dst_w, dst_h) = self.bloom_mip_sizes[dst_idx];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[bloom] upsample"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.bloom_mip_views[dst_idx],
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, dst_w as f32, dst_h as f32, 0.0, 1.0);
                pass.set_pipeline(&self.bloom_upsample_pipeline);
                pass.set_bind_group(0, &self.bloom_upsample_bgs[i], &[]);
                pass.draw(0..3, 0..1);
            }
        }

        // ── atmospheric scattering sky pass ──────────────────────────────
        // runs after the main color pass; alpha-blends sky only onto depth==1.0 pixels
        if let Some(atmos) = world.get_resource::<AtmosphericScattering>().copied() {
            // sun direction: dir_direction points from scene toward the light source
            let sun_dir = (-dir_direction).normalize();
            let mut atmos_data = [0u8; ATMOS_PARAMS_SIZE as usize];
            let sun_dir_arr: [f32; 3] = [sun_dir.x, sun_dir.y, sun_dir.z];
            atmos_data[0..12].copy_from_slice(bytemuck::cast_slice(&sun_dir_arr));
            atmos_data[12..16].copy_from_slice(&atmos.sun_intensity.to_le_bytes());
            atmos_data[16..28].copy_from_slice(bytemuck::cast_slice(&atmos.rayleigh_scatter));
            atmos_data[28..32].copy_from_slice(&atmos.mie_scatter.to_le_bytes());
            atmos_data[32..36].copy_from_slice(&atmos.rayleigh_scale.to_le_bytes());
            atmos_data[36..40].copy_from_slice(&atmos.mie_scale.to_le_bytes());
            atmos_data[40..44].copy_from_slice(&atmos.mie_anisotropy.to_le_bytes());
            atmos_data[44..48].copy_from_slice(&6_371_000.0_f32.to_le_bytes());
            atmos_data[48..52].copy_from_slice(&6_471_000.0_f32.to_le_bytes());
            atmos_data[52..56].copy_from_slice(&atmos.exposure.to_le_bytes());
            self.queue.write_buffer(&self.atmos_params_buf, 0, &atmos_data);

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[atmos] sky pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.hdr_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.atmos_pipeline);
            pass.set_bind_group(0, &self.atmos_bg0, &[]);
            pass.set_bind_group(1, &self.atmos_bg1, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── SSR pass (mid+ tier) ─────────────────────────────────────────
        if dev_ssr {
            let width  = self.render_w as f32;
            let height = self.render_h as f32;
            let inv_vp   = view_proj.inverse();
            let view_mat = Mat4::look_at_rh(cam_pos, cam_pos + cam_wt.forward(), cam_wt.up());
            let inv_vp_cols  = inv_vp.to_cols_array();
            let vp_cols      = view_proj.to_cols_array();
            let view_cols    = view_mat.to_cols_array();
            let mut ssr_data = [0u8; SSR_PARAMS_SIZE as usize];
            ssr_data[0..64].copy_from_slice(bytemuck::cast_slice(&inv_vp_cols));
            ssr_data[64..128].copy_from_slice(bytemuck::cast_slice(&vp_cols));
            ssr_data[128..192].copy_from_slice(bytemuck::cast_slice(&view_cols));
            // screen_size(vec2) + max_steps(u32) + thickness + stride + fade_start + 2 pads
            let max_steps: u32 = 32;
            ssr_data[192..196].copy_from_slice(&width.to_le_bytes());
            ssr_data[196..200].copy_from_slice(&height.to_le_bytes());
            ssr_data[200..204].copy_from_slice(&max_steps.to_le_bytes());
            ssr_data[204..208].copy_from_slice(&0.5_f32.to_le_bytes()); // thickness
            ssr_data[208..212].copy_from_slice(&1.0_f32.to_le_bytes()); // stride
            ssr_data[212..216].copy_from_slice(&0.1_f32.to_le_bytes()); // fade_start
            self.queue.write_buffer(&self.ssr_params_buf, 0, &ssr_data);

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[ssr] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ssr_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.ssr_pipeline);
            pass.set_bind_group(0, &self.ssr_bg0, &[]);
            pass.set_bind_group(1, &self.ssr_bg1, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── volumetric fog pass (mid+ tier) ──────────────────────────────
        if self.fog_enabled && dev_fog {
            let width  = self.render_w as f32;
            let height = self.render_h as f32;
            let inv_vp      = view_proj.inverse();
            let inv_vp_cols = inv_vp.to_cols_array();
            // write fog params: inv_view_proj(64) + rest(64) = 128 bytes
            let mut fog_data = [0u8; FOG_PARAMS_SIZE as usize];
            fog_data[0..64].copy_from_slice(bytemuck::cast_slice(&inv_vp_cols));
            // rest 64 bytes: dir_direction(12)+step_count(4)+dir_color(12)+density(4)+
            //                fog_color(12)+max_dist(4)+sun(4)+aniso(4)+w(4)+h(4)
            let dir_d = dir_direction.normalize();
            let step_count: u32 = 16;
            // sun_dir points towards sun (negate scene light direction)
            let sun_dir: [f32; 3] = [-dir_d.x, -dir_d.y, -dir_d.z];
            let fog_color: [f32; 3] = [sky_color.r * 0.5, sky_color.g * 0.5, sky_color.b * 0.7];
            let rest: [f32; 16] = [
                sun_dir[0], sun_dir[1], sun_dir[2], f32::from_bits(step_count),
                dir_color.r, dir_color.g, dir_color.b, 0.01_f32,    // density
                fog_color[0], fog_color[1], fog_color[2], 200.0_f32, // max_distance
                2.0_f32, 0.6_f32, width, height,                     // sun_intensity, anisotropy
            ];
            fog_data[64..128].copy_from_slice(bytemuck::cast_slice(&rest));
            self.queue.write_buffer(&self.fog_params_buf, 0, &fog_data);

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[fog] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.fog_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.fog_pipeline);
            pass.set_bind_group(0, &self.fog_bg0, &[]);
            pass.set_bind_group(1, &self.fog_bg1, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── contact shadow pass (before composite) ───────────────────────
        if dev_contact_shadows {
            let width  = self.surface_config.width;
            let height = self.surface_config.height;
            let inv_proj = view_proj.inverse();
            self.ensure_contact_shadow_resources(width, height, &inv_proj);

            // update light_dir_vs in params (view-space directional light direction)
            let light_dir_vs_raw = (cam_wt.rotation.inverse() * dir_direction).normalize();
            if let Some(params_buf) = self.contact_shadow_params_buf.as_ref() {
                let light_dir_data: [f32; 3] = [light_dir_vs_raw.x, light_dir_vs_raw.y, light_dir_vs_raw.z];
                self.queue.write_buffer(params_buf, 64, bytemuck::cast_slice(&light_dir_data));
            }

            if let (Some(pipeline), Some(bgl), Some(params_buf), Some(cs_view), Some(depth_view)) = (
                self.contact_shadow_pipeline.as_ref(),
                self.contact_shadow_bgl.as_ref(),
                self.contact_shadow_params_buf.as_ref(),
                self.contact_shadow_view.as_ref(),
                Some(&self.gtao_depth_view),
            ) {
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[contact shadow] bg"),
                    layout: bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
                    ],
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[contact shadow] pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: cs_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bg, &[]);
                pass.draw(0..3, 0..1);
            }

            // rebuild composite_bg if contact_shadow_tex was just created
            if self.composite_bg_dirty {
                let cs_view_ref = self.contact_shadow_view.as_ref()
                    .unwrap_or(&self.contact_shadow_fallback_view);
                self.composite_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[composite] bg"),
                    layout: &self.composite_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.composite_params_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hdr_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(self.bloom_mip_views.first().unwrap_or(&self.hdr_view)) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.gtao_ao_view_a) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.ssr_view) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.fog_view) },
                        wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
                        wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(cs_view_ref) },
                    ],
                });
                self.composite_bg_dirty = false;
            }
        }

        // ── motion vector pass (after main color, before composite) ─────
        {
            let width  = self.surface_config.width;
            let height = self.surface_config.height;
            self.ensure_motion_vector_resources(width, height);
            let inv_vp = view_proj.inverse();
            let inv_vp_cols  = inv_vp.to_cols_array();
            let prev_vp_cols = self.prev_view_proj.to_cols_array();
            if let Some(params_buf) = self.motion_vec_params_buf.as_ref() {
                let mut mv_data = [0u8; MOTION_VECTOR_PARAMS_SIZE as usize];
                mv_data[0..64].copy_from_slice(bytemuck::cast_slice(&inv_vp_cols));
                mv_data[64..128].copy_from_slice(bytemuck::cast_slice(&prev_vp_cols));
                mv_data[128..132].copy_from_slice(&(width as f32).to_le_bytes());
                mv_data[132..136].copy_from_slice(&(height as f32).to_le_bytes());
                self.queue.write_buffer(params_buf, 0, &mv_data);
            }
            if let (Some(pipeline), Some(bgl), Some(params_buf), Some(mv_view)) = (
                self.motion_vec_pipeline.as_ref(),
                self.motion_vec_bgl.as_ref(),
                self.motion_vec_params_buf.as_ref(),
                self.motion_vec_view.as_ref(),
            ) {
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[motion vec] bg"),
                    layout: bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.staa_nearest_sampler) },
                    ],
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[motion vec] pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: mv_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bg, &[]);
                pass.draw(0..3, 0..1);
            }
            self.prev_view_proj = view_proj;
        }

        // ── composite pass → swapchain ────────────────────────────────────
        {
            let time = world.resource::<lunar_core::Time>();
            let quality = world.get_resource::<QualitySettings>();
            let (bloom_strength, vignette_strength, vignette_radius, ca_strength, grain_strength, flags) = {
                let q = quality;
                let mut f: u32 = 0;
                let bloom_s;
                let vig_s;
                let vig_r;
                let ca_s;
                let grain_s;
                if let Some(q) = q {
                    if self.bloom_enabled && dev_bloom && q.bloom { f |= 1; }
                    if dev_vignette && q.vignette { f |= 2; }
                    if dev_chrom_ab && q.chromatic_aberration { f |= 4; }
                    if dev_film_grain && q.film_grain { f |= 8; }
                    if self.ssao_enabled && dev_ssao && q.ssao { f |= 16; }
                    if dev_ssr { f |= 32; }
                    if self.fog_enabled && dev_fog && q.volumetric_fog { f |= 64; }
                    if dev_contact_shadows && self.contact_shadow_tex.is_some() { f |= 128; }
                    bloom_s = 0.04_f32;
                    vig_s   = if dev_vignette && q.vignette { 0.3 } else { 0.0 };
                    vig_r   = 0.3_f32;
                    ca_s    = if dev_chrom_ab && q.chromatic_aberration { 1.5 } else { 0.0 };
                    grain_s = if dev_film_grain && q.film_grain { 0.5 } else { 0.0 };
                } else {
                    bloom_s = 0.04; vig_s = 0.0; vig_r = 0.0; ca_s = 0.0; grain_s = 0.0;
                    if self.bloom_enabled && dev_bloom { f |= 1; }
                    if dev_contact_shadows && self.contact_shadow_tex.is_some() { f |= 128; }
                }
                (bloom_s, vig_s, vig_r, ca_s, grain_s, f)
            };
            let composite_data: [f32; 8] = [
                bloom_strength,
                vignette_strength,
                vignette_radius,
                ca_strength,
                grain_strength,
                time.elapsed_seconds().fract(),
                f32::from_bits(flags),
                0.0, // _pad
            ];
            self.queue.write_buffer(&self.composite_params_buf, 0, bytemuck::cast_slice(&composite_data));

            // fxaa and taa both need composite output in a sampleable intermediate texture.
            // taa takes priority over fxaa (they're mutually exclusive on mid/high tier).
            // when FSR is active, composite writes to the render-resolution fsr_ldr texture first.
            let use_intermediate = dev_staa || dev_fxaa;
            let composite_target = if self.upscale_active {
                self.fsr_ldr_view.as_ref().unwrap_or(&self.fxaa_ldr_view)
            } else if use_intermediate {
                &self.fxaa_ldr_view
            } else {
                &view
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[composite] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: composite_target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &self.composite_bg, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── upscale pass: render resolution → display resolution ──────────────
        // runs when render_scale < 1.0. mode selected from upscale_mode (resolved
        // above to respect DevRenderProfile::forced_upscale_mode).
        if self.upscale_active
            && let (Some(easu_bg), Some(rcas_bg), Some(mid_view), Some(params_buf)) = (
                self.fsr_easu_bg.as_ref(),
                self.fsr_rcas_bg.as_ref(),
                self.fsr_mid_view.as_ref(),
                self.fsr_params_buf.as_ref(),
            ) {
                let dw = self.surface_config.width as f32;
                let dh = self.surface_config.height as f32;
                let upscale_data: [f32; 8] = [
                    self.render_w as f32, self.render_h as f32, dw, dh,
                    0.25, 0.0, 0.0, 0.0,  // rcas_sharpness + padding
                ];
                self.queue.write_buffer(params_buf, 0, bytemuck::cast_slice(&upscale_data));

                let final_target = if dev_staa || dev_fxaa {
                    &self.fxaa_ldr_view
                } else {
                    &view
                };

                let run_pass = |encoder: &mut wgpu::CommandEncoder, label: &'static str, pipeline: &wgpu::RenderPipeline, bg: &wgpu::BindGroup, target: &wgpu::TextureView| {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(label),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target, resolve_target: None,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, bg, &[]);
                    pass.draw(0..3, 0..1);
                };

                match upscale_mode {
                    UpscaleMode::Nearest => if let Some(pl) = &self.upscale_nearest_pipeline {
                        run_pass(&mut *encoder, "[upscale] nearest", pl, easu_bg, final_target);
                    },
                    UpscaleMode::Linear => if let Some(pl) = &self.upscale_linear_pipeline {
                        run_pass(&mut *encoder, "[upscale] linear", pl, easu_bg, final_target);
                    },
                    UpscaleMode::Lanczos => if let Some(pl) = &self.upscale_lanczos_pipeline {
                        run_pass(&mut *encoder, "[upscale] lanczos", pl, easu_bg, final_target);
                    },
                    UpscaleMode::Bicubic => if let Some(pl) = &self.upscale_bicubic_pipeline {
                        run_pass(&mut *encoder, "[upscale] bicubic", pl, easu_bg, final_target);
                    },
                    UpscaleMode::Fsr3 => {
                        // EASU: fsr_ldr → fsr_mid
                        if let Some(pl) = &self.fsr_easu_pipeline {
                            run_pass(&mut *encoder, "[upscale] fsr easu", pl, easu_bg, mid_view);
                        }
                        // RCAS: fsr_mid → final target
                        if let Some(pl) = &self.fsr_rcas_pipeline {
                            run_pass(&mut *encoder, "[upscale] fsr rcas", pl, rcas_bg, final_target);
                        }
                    },
                }
        }

        // ── TAA pass → swapchain + history ───────────────────────────────────
        if dev_staa {
            let w  = self.surface_config.width;
            let h  = self.surface_config.height;

            // pack params: prev_vp(64) + inv_vp(64) + jitter(8) + rcp_frame(8) + blend_alpha(4) + frame_index(4) + depth_scale(8)
            let inv_vp = view_proj.inverse();
            let mut taa_data = [0u8; STAA_PARAMS_SIZE as usize];
            taa_data[0..64].copy_from_slice(bytemuck::cast_slice(&self.staa_prev_vp_jittered.to_cols_array()));
            taa_data[64..128].copy_from_slice(bytemuck::cast_slice(&inv_vp.to_cols_array()));
            taa_data[128..132].copy_from_slice(bytemuck::cast_slice(&[staa_jitter_ndc.x]));
            taa_data[132..136].copy_from_slice(bytemuck::cast_slice(&[staa_jitter_ndc.y]));
            taa_data[136..140].copy_from_slice(bytemuck::cast_slice(&[1.0_f32 / w as f32]));
            taa_data[140..144].copy_from_slice(bytemuck::cast_slice(&[1.0_f32 / h as f32]));
            taa_data[144..148].copy_from_slice(bytemuck::cast_slice(&[0.1_f32]));  // blend_alpha
            taa_data[148..152].copy_from_slice(&self.staa_frame_index.to_le_bytes());
            // depth_scale: ratio of render resolution to display resolution.
            // staa runs at display resolution but depth_tex is at render resolution,
            // so we must scale the texel coordinates to stay in-bounds.
            taa_data[152..156].copy_from_slice(bytemuck::cast_slice(&[self.render_w as f32 / w as f32]));
            taa_data[156..160].copy_from_slice(bytemuck::cast_slice(&[self.render_h as f32 / h as f32]));
            // previous frame jitter (uv space) so the shader can fully un-jitter velocity:
            // without it a static camera reads ~0.4px phantom motion and never reaches
            // zero-spatial (pure temporal ssaa) → soft edges when still.
            taa_data[160..164].copy_from_slice(bytemuck::cast_slice(&[self.staa_prev_jitter.x]));
            taa_data[164..168].copy_from_slice(bytemuck::cast_slice(&[self.staa_prev_jitter.y]));
            self.queue.write_buffer(&self.staa_params_buf, 0, &taa_data);

            // ping-pong: even frame writes to history_b (reads from history_a via bg_even)
            //            odd  frame writes to history_a (reads from history_b via bg_odd)
            let (bg, write_a, write_b) = if !self.staa_ping {
                (&self.staa_bg_even, false, true)
            } else {
                (&self.staa_bg_odd, true, false)
            };
            let history_write_view = if write_b { &self.staa_history_b_view } else { &self.staa_history_a_view };

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[staa] pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: history_write_view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    }),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.staa_pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);

            // advance taa state for next frame
            self.staa_frame_index = self.staa_frame_index.wrapping_add(1);
            self.staa_ping       = !self.staa_ping;
            let _ = (write_a, write_b);
        }

        // always track the jittered vp regardless of staa state, so the first active
        // frame has a correct value for history reprojection.
        self.staa_prev_vp_jittered = view_proj;          // jittered vp, matches history
        self.staa_prev_jitter      = staa_jitter_ndc;    // this frame's jitter → "prev" next frame

        // ── FXAA pass → swapchain ─────────────────────────────────────────
        if dev_fxaa && !dev_staa {
            let w = self.surface_config.width;
            let h = self.surface_config.height;
            let fxaa_data: [f32; 4] = [1.0 / w as f32, 1.0 / h as f32, 0.0, 0.0];
            self.queue.write_buffer(&self.fxaa_params_buf, 0, bytemuck::cast_slice(&fxaa_data));

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[fxaa] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.fxaa_pipeline);
            pass.set_bind_group(0, &self.fxaa_bg, &[]);
            pass.draw(0..3, 0..1);
        }

    }
    /// records GTAO (half-res horizon-based AO + bilateral blur) and any planar
    /// reflection passes into the frame encoder, before the main color pass.
    pub(crate) fn record_gtao_reflection(&mut self, fc: &FrameContext, world: &mut World, encoder: &mut wgpu::CommandEncoder) {
        let &FrameContext { cam_pos, cam_wt, aspect, dev_ssao, dev_staa, sky_color, camera, .. } = fc;
        // ── GTAO passes (mid/high tier, ssao enabled) ────────────────────
        // taa also needs non-msaa depth for reprojection; share this prepass when either is active
        if (self.ssao_enabled && dev_ssao) || dev_staa {
            // non-MSAA depth prepass so GTAO/TAA can sample depth without MSAA complication
            {
                let mut zpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[gtao] depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.gtao_depth_view,
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
                zpass.set_pipeline(&self.zprepass_nonmsaa_pipeline);
                zpass.set_bind_group(0, &self.globals_bg, &[]);
                zpass.set_bind_group(1, &self.material_bg, &[]);
                zpass.set_bind_group(2, &self.entity_bg, &[]);
                zpass.set_bind_group(3, &self.lights_bg, &[]);
                {
                    let mut last_mesh = u32::MAX;
                    let mut last_mat = u32::MAX;
                    let mut group_start = 0usize;
                    let n = self.draw_scratch.len();
                    let mut i = 0usize;
                    while i <= n {
                        let done = i == n;
                        let (cur_mesh, cur_mat) = if done { (u32::MAX, u32::MAX) }
                            else { (self.draw_scratch[i].1, self.draw_scratch[i].2) };
                        if (cur_mesh != last_mesh || cur_mat != last_mat) && i > group_start
                            && let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) {
                                let base = (ENTITY_SLOT_START + group_start) as u32;
                                zpass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + (i - group_start) as u32);
                            }
                        if done { break; }
                        if cur_mesh != last_mesh || cur_mat != last_mat {
                            if let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) {
                                zpass.set_vertex_buffer(0, gpu_mesh.pos_buf.slice(..));
                                zpass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                            }
                            last_mesh = cur_mesh; last_mat = cur_mat; group_start = i;
                        }
                        i += 1;
                    }
                }
            }

            // upload GTAO params
            let (ao_w, ao_h) = (
                (self.surface_config.width / 2).max(1),
                (self.surface_config.height / 2).max(1),
            );
            let (fov_y, near, far) = match camera.projection {
                Projection::Perspective { fov_y, near, far } => (fov_y, near, far),
                Projection::Orthographic { .. } => (std::f32::consts::FRAC_PI_3, 0.1, 1000.0),
            };
            let proj = camera.view_proj(cam_wt, aspect);
            let inv_proj = proj.inverse();
            let gtao_params: [f32; 40] = {
                let mut d = [0f32; 40];
                d[..16].copy_from_slice(&inv_proj.to_cols_array());
                d[16..32].copy_from_slice(&proj.to_cols_array());
                d[32] = world.resource::<lunar_core::Time>().elapsed_seconds();
                d[33] = 1.5; // radius metres
                d[34] = far;
                d[35] = if self.render_tier == RenderTier::High { 5.0 } else { 3.0 }; // slice_count
                d[36] = if self.render_tier == RenderTier::High { 6.0 } else { 4.0 }; // step_count
                d[37] = ao_w as f32;
                d[38] = ao_h as f32;
                d[39] = 0.0;
                let _ = (fov_y, near);
                d
            };
            self.queue.write_buffer(&self.gtao_params_buf, 0, bytemuck::cast_slice(&gtao_params));

            let run_fullscreen_pass = |encoder: &mut wgpu::CommandEncoder,
                                       label: &str,
                                       pipeline: &wgpu::RenderPipeline,
                                       bg: &wgpu::BindGroup,
                                       target: &wgpu::TextureView,
                                       w: u32, h: u32| {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            };

            run_fullscreen_pass(&mut *encoder, "[gtao] main", &self.gtao_pipeline, &self.gtao_main_bg, &self.gtao_ao_view_a, ao_w, ao_h);
            run_fullscreen_pass(&mut *encoder, "[gtao] blur-h", &self.gtao_blur_h_pipeline, &self.gtao_blur_h_bg, &self.gtao_ao_view_b, ao_w, ao_h);
            run_fullscreen_pass(&mut *encoder, "[gtao] blur-v", &self.gtao_blur_v_pipeline, &self.gtao_blur_v_bg, &self.gtao_ao_view_a, ao_w, ao_h);
        }

        // ── planar reflection pass ────────────────────────────────────────
        // find visible PlanarReflector entities (max 1 per frame)
        {
            let mut reflector: Option<(f32, u32)> = None;  // (plane_y, resolution_divisor)
            {
                let mut rq = world.query::<(&PlanarReflector, &WorldTransform3d, &ComputedVisibility)>();
                for (refl, wt, vis) in rq.iter(world) {
                    if !vis.0 { continue; }
                    if reflector.is_none() {
                        reflector = Some((refl.plane_y + wt.translation.y, refl.resolution_divisor.max(1)));
                    }
                }
            }
            if let Some((plane_y, div)) = reflector {
                let rw = (self.surface_config.width  / div).max(1);
                let rh = (self.surface_config.height / div).max(1);

                // lazy-create reflection texture
                let needs_new = self.reflection_tex.is_none()
                    || self.reflection_tex.as_ref().map(|t| {
                        let sz = t.size();
                        sz.width != rw || sz.height != rh
                    }).unwrap_or(false);
                if needs_new {
                    let rt = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("[reflection] tex"),
                        size: wgpu::Extent3d { width: rw, height: rh, depth_or_array_layers: 1 },
                        mip_level_count: 1, sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: self.hdr_format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    self.reflection_view = Some(rt.create_view(&wgpu::TextureViewDescriptor::default()));
                    self.reflection_tex  = Some(rt);
                    self.water_bg_dirty  = true;
                }

                // compute reflected camera: flip position and forward about plane_y
                let refl_cam_pos = lunar_math::Vec3::new(cam_pos.x, 2.0 * plane_y - cam_pos.y, cam_pos.z);
                // reflected view: negate Y of forward + up
                let cam_fwd  = cam_wt.forward();
                let cam_up   = cam_wt.up();
                let refl_fwd = lunar_math::Vec3::new(cam_fwd.x, -cam_fwd.y, cam_fwd.z);
                let refl_up  = lunar_math::Vec3::new(cam_up.x,  -cam_up.y,  cam_up.z);
                let refl_target = refl_cam_pos + refl_fwd;
                let refl_view = Mat4::look_at_rh(refl_cam_pos, refl_target, refl_up);
                let aspect = rw as f32 / rh as f32;
                let proj_mat = Mat4::perspective_rh(std::f32::consts::FRAC_PI_4, aspect, 0.1, 500.0);
                let refl_vp = proj_mat * refl_view;

                // write reflected globals to a dedicated buffer
                if self.reflection_globals_buf.is_none() {
                    self.reflection_globals_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("[reflection] globals buf"),
                        size: GLOBALS_SIZE,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
                let refl_vp_cols = refl_vp.to_cols_array();
                let time = world.resource::<lunar_core::Time>();
                let mut refl_globals = [0f32; 24];
                refl_globals[..16].copy_from_slice(&refl_vp_cols);
                refl_globals[16] = refl_cam_pos.x;
                refl_globals[17] = refl_cam_pos.y;
                refl_globals[18] = refl_cam_pos.z;
                refl_globals[19] = time.elapsed_seconds();
                refl_globals[20] = time.delta_seconds();
                let refl_globals_buf = self.reflection_globals_buf.as_ref().unwrap();
                self.queue.write_buffer(refl_globals_buf, 0, bytemuck::cast_slice(&refl_globals));

                // rebuild reflection_globals_bg if needed
                if self.reflection_globals_bg.is_none() {
                    self.reflection_globals_bg = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("[reflection] globals bg"),
                        layout: &self.globals_bgl,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0, resource: refl_globals_buf.as_entire_binding(),
                        }],
                    }));
                }

                // depth buffer for the reflection pass (simple 1-sample), cached on (rw, rh)
                let depth_needs_new = self.reflection_depth_tex.is_none()
                    || self.reflection_depth_tex.as_ref().map(|t| {
                        let sz = t.size();
                        sz.width != rw || sz.height != rh
                    }).unwrap_or(false);
                if depth_needs_new {
                    let dt = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("[reflection] depth"),
                        size: wgpu::Extent3d { width: rw, height: rh, depth_or_array_layers: 1 },
                        mip_level_count: 1, sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Depth32Float,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    });
                    self.reflection_depth_view = Some(dt.create_view(&wgpu::TextureViewDescriptor::default()));
                    self.reflection_depth_tex  = Some(dt);
                }
                let refl_depth_view = self.reflection_depth_view.as_ref().unwrap();

                // rebuild reflection_globals_bg when buf is first created
                if self.reflection_globals_bg.is_none() {
                    let buf = self.reflection_globals_buf.as_ref().unwrap();
                    self.reflection_globals_bg = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("[reflection] globals bg"),
                        layout: &self.globals_bgl,
                        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
                    }));
                }

                if let Some(refl_view) = self.reflection_view.as_ref() {
                    // render sky + opaque geometry into the reflection texture.
                    // for this sprint we draw a clear to sky-tinted color as the background.
                    // full per-entity geometry reflection via opaque_pipeline with reflected
                    // globals is a future sprint (requires per-entity instance data rebind).
                    let sky_r = sky_color.r * 0.7;
                    let sky_g = sky_color.g * 0.8;
                    let sky_b = sky_color.b * 0.9;
                    let _rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("[reflection] pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: refl_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color { r: sky_r as f64, g: sky_g as f64, b: sky_b as f64, a: 1.0 }),
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &refl_depth_view,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    // full per-entity rendering with reflected globals is a future sprint.
                    // for now the reflected background color provides basic water reflection.
                }

                // rebuild water_bg0 to point at the new reflection texture
                if self.water_bg_dirty {
                    let refl_v = self.reflection_view.as_ref().unwrap_or(&self.reflection_fallback_view);
                    self.water_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("[water] bg0"),
                        layout: &self.water_bgl0,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hdr_view) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
                            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(refl_v) },
                        ],
                    });
                    self.water_bg_dirty = false;
                }
            }
        }

    }
}
