//! `RenderEngine3d` — top-level frame render, dynamic resolution, cascade helpers.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

impl RenderEngine3d {
    // a few loops below index multiple parallel arrays by the same counter, or use a
    // sentinel final iteration for batch flushing — clearer as indexed loops than as
    // iterator adapters, so the range-loop/counter lints are intentionally allowed.
    #[allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]
    pub(crate) fn render_frame(&mut self, world: &mut World) -> u32 {
        // ── gather camera — copy immediately so world borrows end here ────
        let cam_entity = {
            let active = world.resource::<ActiveCamera3d>();
            let Some(e) = active.entity else { return 0; };
            e
        };
        let camera = { let Some(c) = world.get::<Camera3d>(cam_entity) else { return 0; }; *c };
        let cam_wt  = { let Some(t) = world.get::<WorldTransform3d>(cam_entity) else { return 0; }; *t };

        // apply render scale and MSAA changes before computing viewport
        {
            let desired_scale = world.get_resource::<QualitySettings>()
                .map(|q| q.render_scale.clamp(0.1, 1.0))
                .unwrap_or(1.0);
            if (desired_scale - self.render_scale).abs() > 1e-4 {
                self.set_render_scale(desired_scale);
            }
            let desired_msaa = world.get_resource::<QualitySettings>()
                .map(|q| q.msaa_samples.clamp(1, 8))
                .unwrap_or(self.msaa_samples);
            if desired_msaa != self.msaa_samples {
                self.apply_msaa_change(desired_msaa);
            }
        }

        // viewport rect for the primary camera: used for scissor/viewport state in color passes.
        // for split-screen, secondary cameras use render-to-texture; the primary camera's rect
        // is applied here to confine its rendering to its portion of the screen.
        let primary_viewport: ViewportRect = {
            let viewports = world.resource::<ActiveViewports>();
            viewports.viewports.iter()
                .find(|(e, _)| *e == cam_entity)
                .map(|(_, r)| *r)
                .unwrap_or(ViewportRect::FULL)
        };

        // when FSR is active, all render passes use render resolution, not display resolution
        let (vp_x, vp_y, vp_w, vp_h) = primary_viewport.to_pixels(self.render_w, self.render_h);

        // aspect ratio from viewport rect (not full window) so projection is correct for the rect
        let aspect = if primary_viewport.height > 1e-6 {
            (vp_w as f32) / (vp_h as f32)
        } else {
            world.resource::<ViewportAspect>().0
        };

        // unjittered vp — used for taa prev_vp storage and as the shadow/other-pass matrix
        let view_proj_unjittered = camera.view_proj(cam_wt, aspect);

        // when taa is active, jitter the projection matrix using Halton(2,3) 8-point sequence.
        // this sub-pixel shift (≤0.5px) is applied via the projection matrix column 2 so the
        // jitter is depth-independent (constant screen-space offset for all vertices).
        // the jitter makes each frame sample a different sub-pixel position; TAA then accumulates
        // these samples to achieve effective temporal super-sampling on edge-adjacent pixels.
        // we jitter EVERY frame (standard TAA): the shader reprojects + un-jitters history, so
        // there is no motion stutter, and accumulation never resets. (it used to gate on an exact
        // `prev_vp == vp` stationary check, but that flickered off on the tiniest mouse motion —
        // resetting the accumulation before it could converge, which read as permanent softness.)

        // precompute before the jitter decision (full dev_staa is built below with the rest)
        // dev_staa is resolved below with the rest of the dev profile, but we need
        // the combined (dev+quality) staa decision here for the jitter gate. duplicate
        // the short reads rather than restructure the whole function.
        let staa_on = world.get_resource::<DevRenderProfile>().map(|d| d.staa).unwrap_or(true)
            && world.get_resource::<QualitySettings>().map(|q| q.staa).unwrap_or(false)
            && self.staa_enabled; // staa_enabled = false on LowGles (no compute), true otherwise

        let (view_proj, staa_jitter_ndc) = if staa_on {
            // Halton low-discrepancy sequence: base 2 for x, base 3 for y.
            // use frame_index+1 so index 0 maps to a non-zero offset (avoids identity jitter).
            let idx = (self.staa_frame_index % 8 + 1) as u64;
            fn halton(mut i: u64, base: u64) -> f32 {
                let (mut f, mut r) = (1.0f64, 0.0f64);
                while i > 0 { f /= base as f64; r += f * (i % base) as f64; i /= base; }
                r as f32
            }
            // NDC jitter: ≤0.5px in display-resolution screen space.
            // using display (not render) dimensions means the oscillation stays
            // sub-pixel at the output regardless of render scale. at render_scale < 1
            // the render-pixel shift is render_scale * 0.5px — effectively zero at
            // very low scales, which is correct: no sub-pixel info to accumulate there.
            let dw = self.surface_config.width as f32;
            let dh = self.surface_config.height as f32;
            let jx = (halton(idx, 2) - 0.5) * 2.0 / dw;
            let jy = (halton(idx, 3) - 0.5) * 2.0 / dh;

            // modify the projection column 2 (z-axis.xy) to add a constant NDC offset.
            // P[2][0] += Δx shifts NDC.x by -Δx for all depths (clip.w cancels out).
            let view_mat = Camera3d::view_matrix(cam_wt);
            let mut jittered_proj = camera.projection.matrix(aspect);
            jittered_proj.z_axis.x -= jx;
            jittered_proj.z_axis.y -= jy;
            // jitter in UV space = NDC jitter / 2 (NDC spans [-1,1], UV spans [0,1])
            (jittered_proj * view_mat, Vec2::new(jx * 0.5, jy * 0.5))
        } else {
            (view_proj_unjittered, Vec2::ZERO)
        };

        let cam_pos = cam_wt.translation;

        // ── read dev render profile (dev's feature ceiling) ───────────────
        // all pass gates below AND with this so disabled features are never executed
        // regardless of user quality settings or hardware tier.
        // fxaa/staa/ssr also AND with QualitySettings so runtime toggles take effect
        // without having to restart (the pipelines and textures are always built).
        let dev_bloom            = world.get_resource::<DevRenderProfile>().map(|d| d.bloom            ).unwrap_or(true);
        let dev_ssao             = world.get_resource::<DevRenderProfile>().map(|d| d.ssao             ).unwrap_or(true);
        let dev_ssr              = world.get_resource::<DevRenderProfile>().map(|d| d.ssr              ).unwrap_or(true)
            && world.get_resource::<QualitySettings>().map(|q| q.ssr  ).unwrap_or(false);
        let dev_fog              = world.get_resource::<DevRenderProfile>().map(|d| d.volumetric_fog   ).unwrap_or(true);
        let dev_fxaa             = world.get_resource::<DevRenderProfile>().map(|d| d.fxaa             ).unwrap_or(true)
            && world.get_resource::<QualitySettings>().map(|q| q.fxaa ).unwrap_or(false);
        let dev_staa             = world.get_resource::<DevRenderProfile>().map(|d| d.staa             ).unwrap_or(true)
            && world.get_resource::<QualitySettings>().map(|q| q.staa ).unwrap_or(false);
        let dev_vignette         = world.get_resource::<DevRenderProfile>().map(|d| d.vignette         ).unwrap_or(true);
        let dev_chrom_ab         = world.get_resource::<DevRenderProfile>().map(|d| d.chromatic_aberration).unwrap_or(true);
        let dev_film_grain       = world.get_resource::<DevRenderProfile>().map(|d| d.film_grain       ).unwrap_or(true);
        let dev_point_shadows    = world.get_resource::<DevRenderProfile>().map(|d| d.point_light_shadows).unwrap_or(true);
        let dev_max_point_lights = world.get_resource::<DevRenderProfile>().map(|d| d.max_point_lights as usize).unwrap_or(MAX_CLUSTERED_LIGHTS);
        let dev_soft_shadows     = world.get_resource::<DevRenderProfile>().map(|d| d.soft_shadows     ).unwrap_or(false);
        let dev_contact_shadows  = world.get_resource::<DevRenderProfile>().map(|d| d.contact_shadows  ).unwrap_or(false);

        // upscale resources — set_render_scale already ran above, just check active state
        self.upscale_active = self.render_scale < 0.999;
        if self.upscale_active {
            self.ensure_upscale_resources(self.render_w, self.render_h, self.surface_config.width, self.surface_config.height);
        }

        // resolve upscale mode: dev forced_upscale_mode takes priority over user setting
        let upscale_mode = world.get_resource::<DevRenderProfile>()
            .and_then(|d| d.forced_upscale_mode)
            .or_else(|| world.get_resource::<QualitySettings>().map(|q| q.upscale_mode))
            .unwrap_or(UpscaleMode::Lanczos);

        // ── gather sky ────────────────────────────────────────────────────
        let sky = world.get_resource::<Sky>().copied();
        let sky_color = sky.map_or(Color::rgb(0.1, 0.1, 0.15), |s| s.sky_color);

        // ── gather lights ─────────────────────────────────────────────────
        let ambient = world.get_resource::<AmbientLight>().copied().unwrap_or_default();

        // directional light: first entity with both DirectionalLight + WorldTransform3d
        let mut dir_color = Color::WHITE;
        let mut dir_illuminance: f32 = 0.0;
        let mut dir_direction = Vec3::NEG_Y;
        let mut dir_enabled: u32 = 0;
        let mut dir_casts_shadows = false;
        {
            let mut dq = world.query::<(&DirectionalLight, &WorldTransform3d)>();
            if let Some((dl, wt)) = dq.iter(world).next() {
                dir_color = dl.color;
                dir_illuminance = dl.illuminance;
                dir_direction = wt.forward();
                dir_enabled = 1;
                dir_casts_shadows = dl.casts_shadows;
            }
        }

        // point lights: up to MAX_POINT_LIGHTS closest to camera
        self.point_light_scratch.clear();
        {
            let mut pq = world.query::<(&PointLight, &WorldTransform3d)>();
            pq.iter(world).for_each(|(pl, wt)| {
                self.point_light_scratch.push((wt.translation, pl.color, pl.intensity, pl.radius, pl.casts_shadows));
            });
        }
        self.point_light_scratch.sort_unstable_by(|a, b| {
            let da = (a.0 - cam_pos).length_squared();
            let db = (b.0 - cam_pos).length_squared();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        let max_lights = dev_max_point_lights.min(MAX_CLUSTERED_LIGHTS);
        self.point_light_scratch.truncate(max_lights);

        // ── compute cascade splits (log-linear blend, λ=0.5) ─────────────
        // produces 3 split depths in view space separating the 3 cascade slices.
        let cascade_splits = Self::compute_cascade_splits(SHADOW_NEAR, SHADOW_FAR, NUM_CASCADES as usize, CASCADE_LAMBDA);

        // ── compute per-cascade light-space matrices ──────────────────────
        let light_spaces = if dir_enabled != 0 {
            let cam_forward = cam_wt.forward();
            let cam_up_vec  = cam_wt.up();
            let cam_right   = cam_wt.right();
            let (fov_y, near) = match camera.projection {
                Projection::Perspective { fov_y, near, .. } => (fov_y, near),
                Projection::Orthographic { .. } => (std::f32::consts::FRAC_PI_3, 0.1),
            };
            [
                Self::cascade_light_space(cam_pos, cam_forward, cam_up_vec, cam_right, fov_y, aspect, dir_direction, near, cascade_splits[0]),
                Self::cascade_light_space(cam_pos, cam_forward, cam_up_vec, cam_right, fov_y, aspect, dir_direction, cascade_splits[0], cascade_splits[1]),
                Self::cascade_light_space(cam_pos, cam_forward, cam_up_vec, cam_right, fov_y, aspect, dir_direction, cascade_splits[1], cascade_splits[2]),
            ]
        } else {
            [Mat4::IDENTITY; 3]
        };

        // frustum + HZB occlusion culling (1-frame pipelined on high tier)
        self.cull_entities(world, view_proj, cam_pos);
        self.gather_draw_list(world, cam_pos);
        // ── upload missing meshes ─────────────────────────────────────────
        let mut mesh_evict_ids: Vec<u32> = Vec::new();
        for i in 0..self.draw_scratch.len() {
            let mesh_id = self.draw_scratch[i].1;
            if !self.mesh_gpu.contains_key(&mesh_id) {
                let registry = world.resource::<MeshRegistry>();
                if let Some(data) = registry.get_mesh(lunar_assets::Handle::new(mesh_id, 0)) {
                    let gpu = Self::upload_mesh_data(&self.device, &self.queue, data);
                    self.mesh_gpu.insert(mesh_id, gpu);
                    if data.gpu_only { mesh_evict_ids.push(mesh_id); }
                }
            }
            // also append to mega-buffers when has_indirect and not yet there
            if self.has_indirect && !self.mega_mesh_entries.contains_key(&mesh_id) {
                let registry = world.resource::<MeshRegistry>();
                if let Some(data) = registry.get_mesh(lunar_assets::Handle::new(mesh_id, 0)) {
                    self.append_to_mega_buffers(mesh_id, data);
                }
            }
        }

        // ── surface shader gather ─────────────────────────────────────────
        self.surface_scratch.clear();
        {
            let elapsed = world.resource::<lunar_core::Time>().elapsed_seconds();
            let mut sq = world.query::<(Entity, &Mesh3d, &SurfaceShader, &WorldTransform3d, &ComputedVisibility)>();
            let surface_slot_base = ENTITY_SLOT_START + self.draw_scratch.len();
            let mut surface_idx = 0usize;
            for (entity, mesh, surf, wt, vis) in sq.iter(world) {
                if !vis.0 || surface_idx >= 64 { break; }
                let slot = surface_slot_base + surface_idx;
                // evaluate UV transforms
                let mut packed = [SurfaceStagePacked {
                    uv_offset: [0.0, 0.0], uv_scale: 1.0, blend: 0, alpha: 1.0,
                    use_lm_uv: 0, enabled: 0, _pad: 0,
                }; 4];
                let mut tex_ids = [u32::MAX; 4];
                for (si, stage) in surf.stages.iter().enumerate().take(4) {
                    let blend_u32 = match stage.blend {
                        lunar_3d::BlendMode::Opaque    => 0u32,
                        lunar_3d::BlendMode::Add       => 1u32,
                        lunar_3d::BlendMode::Multiply  => 2u32,
                        lunar_3d::BlendMode::AlphaBlend => 3u32,
                    };
                    let alpha = match stage.alpha_gen {
                        lunar_3d::AlphaGen::Identity => 1.0f32,
                        lunar_3d::AlphaGen::Const(a) => a,
                    };
                    let use_lm_uv = (stage.tc_gen == lunar_3d::TcGen::Lightmap) as u32;
                    // scroll: accumulate scroll * elapsed, then add rotation-derived offset
                    let scroll_x = stage.uv_transform.scroll.x * elapsed;
                    let scroll_y = stage.uv_transform.scroll.y * elapsed;
                    packed[si] = SurfaceStagePacked {
                        uv_offset: [scroll_x, scroll_y],
                        uv_scale: stage.uv_transform.scale,
                        blend: blend_u32, alpha, use_lm_uv,
                        enabled: 1, _pad: 0,
                    };
                    tex_ids[si] = stage.texture.id();
                    // ensure mesh is uploaded
                    let mesh_id = mesh.0.id();
                    if !self.mesh_gpu.contains_key(&mesh_id) {
                        let registry = world.resource::<MeshRegistry>();
                        if let Some(data) = registry.get_mesh(lunar_assets::Handle::new(mesh_id, 0)) {
                            let gpu = Self::upload_mesh_data(&self.device, &self.queue, data);
                            self.mesh_gpu.insert(mesh_id, gpu);
                            if data.gpu_only { mesh_evict_ids.push(mesh_id); }
                        }
                    }
                }
                // upload transform to entity instances buffer
                Self::pack_mesh_uniforms(&mut self.uniform_staging, slot, wt.to_matrix());
                self.surface_scratch.push((entity, slot, tex_ids, packed));
                surface_idx += 1;
            }
        }

        // evict cpu mesh data for newly uploaded gpu_only meshes
        if !mesh_evict_ids.is_empty() {
            mesh_evict_ids.sort_unstable();
            mesh_evict_ids.dedup();
            let mut registry = world.resource_mut::<MeshRegistry>();
            for id in mesh_evict_ids {
                registry.evict_cpu_data(lunar_assets::Handle::new(id, 0));
            }
        }

        // ── grow buffers if needed ────────────────────────────────────────
        let needed = ENTITY_SLOT_START + self.draw_scratch.len() + self.surface_scratch.len();
        if needed > self.entity_capacity {
            self.entity_capacity = needed.next_power_of_two().max(INITIAL_ENTITY_CAPACITY);
            self.entity_buf = Self::make_entity_buf(&self.device, self.entity_capacity);
            self.entity_bg = Self::make_entity_bg(&self.device, &self.mesh_bgl, &self.entity_buf);
            self.uniform_staging.resize(self.entity_capacity * UNIFORM_STRIDE as usize, 0);
            self.material_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[material] storage buffer"),
                size: (self.entity_capacity * MATERIAL_UNIFORMS_SIZE as usize) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.material_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("[material] bg"),
                layout: &self.material_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.material_buf.as_entire_binding(),
                }],
            });
            self.material_staging.resize(self.entity_capacity * MATERIAL_UNIFORMS_SIZE as usize, 0);
            log::debug!("draw buffers grown to {} slots", self.entity_capacity);
        }

        // ── pack mesh + material staging ──────────────────────────────────
        // sky dome and sun are unlit (flags = 1)
        let dome_model = Mat4::from_translation(cam_pos);
        Self::pack_mesh_uniforms(&mut self.uniform_staging, SLOT_DOME, dome_model);
        Self::pack_material_uniforms(&mut self.material_staging, SLOT_DOME, sky_color, 0.0, 1.0, 1, 0, [0.0, 0.0], [1.0, 1.0]);

        if let Some(sky) = sky {
            let sun_model = Mat4::from_translation(cam_pos + Vec3::new(0.0, SUN_Y, 0.0));
            Self::pack_mesh_uniforms(&mut self.uniform_staging, SLOT_SUN, sun_model);
            Self::pack_material_uniforms(&mut self.material_staging, SLOT_SUN, sky.sun_color, 0.0, 1.0, 1, 0, [0.0, 0.0], [1.0, 1.0]);
        }

        // ── texture coverage hints (item E — mip streaming) ──────────────
        // collect (lm_id, coverage) pairs, then update asset server in one pass.
        {
            let mut hints: Vec<(u32, f32)> = Vec::new();
            for i in 0..self.draw_scratch.len() {
                let lm_id = self.draw_scratch[i].9;
                if lm_id == u32::MAX { continue; }
                let model = self.draw_scratch[i].6;
                let world_pos = model.w_axis;
                let dist = (Vec3::new(world_pos.x, world_pos.y, world_pos.z) - cam_pos).length().max(0.01);
                hints.push((lm_id, 1.0 / dist));
            }
            let mut asset_server = world.resource_mut::<lunar_assets::AssetServer>();
            asset_server.coverage_hints.clear();
            for (tid, cov) in hints {
                asset_server.hint_coverage(tid, cov);
            }
        }

        // upload lightmap textures (irradiance + direction) and create combined bind groups
        // step 1: collect needed (lm_id, dir_lm_id) pairs from draw_scratch
        let lm_needed: Vec<(u32, u32)> = {
            let mut v: Vec<(u32, u32)> = self.draw_scratch.iter()
                .filter(|e| e.9 != u32::MAX)
                .map(|e| (e.9, e.10))
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        // step 2: upload textures (uses asset_server borrow)
        let (lm_new_vram, lm_evict_ids): (u64, Vec<u32>) = {
            let asset_server = world.resource::<lunar_assets::AssetServer>();

            // helper: upload one Texture asset to GPU, return (Texture, TextureView)
            let upload_lm_tex = |device: &wgpu::Device, queue: &wgpu::Queue,
                                  tex: &lunar_assets::Texture, label: &str,
                                  srgb: bool| -> (wgpu::Texture, wgpu::TextureView) {
                let (gpu_fmt, bpr_fn): (wgpu::TextureFormat, Box<dyn Fn(u32) -> u32>) =
                    match tex.compression {
                        lunar_assets::TextureCompression::None => if srgb {
                            (wgpu::TextureFormat::Rgba8UnormSrgb, Box::new(|w| w * 4))
                        } else {
                            (wgpu::TextureFormat::Rgba8Unorm, Box::new(|w| w * 4))
                        },
                        // BC1: 8 bytes per 4×4 block (0.5 bytes/texel)
                        lunar_assets::TextureCompression::Bc1 =>
                            (wgpu::TextureFormat::Bc1RgbaUnormSrgb, Box::new(|w| w.div_ceil(4) * 8)),
                        // BC3/BC5/BC6H/BC7: 16 bytes per 4×4 block (1 byte/texel)
                        lunar_assets::TextureCompression::Bc3 =>
                            (wgpu::TextureFormat::Bc3RgbaUnorm, Box::new(|w| w.div_ceil(4) * 16)),
                        lunar_assets::TextureCompression::Bc5 =>
                            (wgpu::TextureFormat::Bc5RgUnorm, Box::new(|w| w.div_ceil(4) * 16)),
                        lunar_assets::TextureCompression::Bc6h =>
                            (wgpu::TextureFormat::Bc6hRgbFloat, Box::new(|w| w.div_ceil(4) * 16)),
                        lunar_assets::TextureCompression::Bc7 =>
                            (wgpu::TextureFormat::Bc7RgbaUnorm, Box::new(|w| w.div_ceil(4) * 16)),
                    };
                let gpu_tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
                    mip_level_count: tex.mip_level_count(),
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: gpu_fmt,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    gpu_tex.as_image_copy(),
                    &tex.pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(bpr_fn(tex.width)),
                        rows_per_image: Some(tex.height.div_ceil(4)),
                    },
                    wgpu::Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
                );
                for (mip_idx, mip_data) in tex.mips.iter().enumerate() {
                    let mip_w = (tex.width >> (mip_idx + 1)).max(1);
                    let mip_h = (tex.height >> (mip_idx + 1)).max(1);
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &gpu_tex,
                            mip_level: (mip_idx + 1) as u32,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        mip_data,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(bpr_fn(mip_w)),
                            rows_per_image: Some(mip_h.div_ceil(4)),
                        },
                        wgpu::Extent3d { width: mip_w, height: mip_h, depth_or_array_layers: 1 },
                    );
                }
                let view = gpu_tex.create_view(&Default::default());
                (gpu_tex, view)
            };

            let mut new_vram_bytes = 0u64;
            let mut evict_ids: Vec<u32> = Vec::new();
            // upload irradiance textures not yet in cache
            for &(lm_id, _) in &lm_needed {
                if !self.lm_tex_cache.contains_key(&lm_id)
                    && let Some(tex) = asset_server.get_texture_by_id(lm_id) {
                        let max_mips = tex.mip_level_count();
                        // desired_mip_count could limit uploads in future; upload full for now
                        let _desired = asset_server.desired_mip_count(lm_id, max_mips);
                        new_vram_bytes += (tex.width * tex.height * 4) as u64 * 4 / 3;
                        let entry = upload_lm_tex(&self.device, &self.queue, tex, "[lightmap] irr", true);
                        self.lm_tex_cache.insert(lm_id, entry);
                        evict_ids.push(lm_id);
                    }
            }
            // upload direction textures not yet in cache
            for &(_, dir_lm_id) in &lm_needed {
                if dir_lm_id != u32::MAX && !self.dir_lm_tex_cache.contains_key(&dir_lm_id)
                    && let Some(tex) = asset_server.get_texture_by_id(dir_lm_id) {
                        new_vram_bytes += (tex.width * tex.height * 4) as u64;
                        let entry = upload_lm_tex(&self.device, &self.queue, tex, "[lightmap] dir", false);
                        self.dir_lm_tex_cache.insert(dir_lm_id, entry);
                        evict_ids.push(dir_lm_id);
                    }
            }
            (new_vram_bytes, evict_ids)
        };  // asset_server released here
        // step 3: update VRAM tracking
        if lm_new_vram > 0
            && let Some(mut vram) = world.get_resource_mut::<lunar_assets::TextureVramUsage>() {
                vram.add_bytes(lm_new_vram);
            }
        // step 3b: evict cpu-side pixel data for newly uploaded lightmap textures
        if !lm_evict_ids.is_empty() {
            let mut asset_server = world.resource_mut::<lunar_assets::AssetServer>();
            for id in lm_evict_ids {
                if let Some(tex) = asset_server.get_texture_by_id_mut(id) {
                    tex.evict_cpu_data();
                }
            }
        }
        // step 4: create missing combined bind groups (only needs self, no world borrow)
        for &(lm_id, dir_lm_id) in &lm_needed {
                if self.lightmap_bg_cache.contains_key(&(lm_id, dir_lm_id)) { continue; }
                let Some((_, irr_view)) = self.lm_tex_cache.get(&lm_id) else { continue; };
                let dir_view: &wgpu::TextureView = if dir_lm_id != u32::MAX {
                    match self.dir_lm_tex_cache.get(&dir_lm_id) {
                        Some((_, v)) => v,
                        None => &self.dir_lm_fallback_view,
                    }
                } else {
                    &self.dir_lm_fallback_view
                };
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[lightmap] bg"),
                    layout: &self.lightmap_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(irr_view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(dir_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.lightmap_sampler) },
                    ],
                });
                self.lightmap_bg_cache.insert((lm_id, dir_lm_id), bg);
        }

        // ── lightmap atlas (phase 3, has_indirect path) ───────────────────
        // pack all loaded irradiance textures into one RGBA8 atlas when has_indirect.
        // rebuild when the set of lm_ids in lm_tex_cache changes.
        // direction textures are not atlased; dir lightmap effects are disabled in indirect path.
        if self.has_indirect && !self.lm_tex_cache.is_empty() {
            let mut current_ids: Vec<u32> = self.lm_tex_cache.keys().copied().collect();
            current_ids.sort_unstable();
            if current_ids != self.atlas_lm_ids {
                // collect texture data for all lightmap ids
                let asset_server = world.resource::<lunar_assets::AssetServer>();
                // gather (lm_id, width, height, pixels-as-rgba8) for each
                let mut entries: Vec<(u32, u32, u32, Vec<u8>)> = Vec::new();
                for &lm_id in &current_ids {
                    if let Some(tex) = asset_server.get_texture_by_id(lm_id)
                        && let lunar_assets::TextureCompression::None = tex.compression {
                            entries.push((lm_id, tex.width, tex.height, tex.pixels.to_vec()));
                        }
                }
                if !entries.is_empty() {
                    // shelf packer: sort by height desc, place left-to-right
                    entries.sort_unstable_by_key(|e| std::cmp::Reverse(e.3.len()));
                    let atlas_dim = ATLAS_SIZE;
                    let mut atlas_pixels = vec![0u8; (atlas_dim * atlas_dim * 4) as usize];
                    let mut cursor_x: u32 = 0;
                    let mut cursor_y: u32 = 0;
                    let mut row_height: u32 = 0;
                    let mut new_uvs: HashMap<u32, [f32; 4]> = HashMap::default();
                    for (lm_id, tw, th, pixels) in &entries {
                        let tw = *tw; let th = *th;
                        if tw > atlas_dim || th > atlas_dim { continue; }
                        if cursor_x + tw > atlas_dim {
                            cursor_x = 0;
                            cursor_y += row_height;
                            row_height = 0;
                        }
                        if cursor_y + th > atlas_dim { break; } // atlas full
                        // blit this texture into atlas
                        for row in 0..th {
                            let src_off = (row * tw * 4) as usize;
                            let dst_off = ((cursor_y + row) * atlas_dim * 4 + cursor_x * 4) as usize;
                            let len = (tw * 4) as usize;
                            atlas_pixels[dst_off..dst_off + len].copy_from_slice(&pixels[src_off..src_off + len]);
                        }
                        let f = atlas_dim as f32;
                        new_uvs.insert(*lm_id, [cursor_x as f32 / f, cursor_y as f32 / f, tw as f32 / f, th as f32 / f]);
                        cursor_x += tw;
                        row_height = row_height.max(th);
                    }
                    // create/recreate atlas texture
                    let atlas_tex = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("[lightmap] atlas"),
                        size: wgpu::Extent3d { width: atlas_dim, height: atlas_dim, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    self.queue.write_texture(
                        atlas_tex.as_image_copy(),
                        &atlas_pixels,
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(atlas_dim * 4), rows_per_image: Some(atlas_dim) },
                        wgpu::Extent3d { width: atlas_dim, height: atlas_dim, depth_or_array_layers: 1 },
                    );
                    let atlas_view = atlas_tex.create_view(&Default::default());
                    let atlas_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("[lightmap] atlas bg"),
                        layout: &self.lightmap_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.dir_lm_fallback_view) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.lightmap_sampler) },
                        ],
                    });
                    self.atlas_tex = Some(atlas_tex);
                    self.atlas_view = Some(atlas_view);
                    self.atlas_bg = Some(atlas_bg);
                    self.atlas_lm_uvs = new_uvs;
                    self.atlas_lm_ids = current_ids;
                }
            }
        }

        let probe_grid   = world.get_resource::<lunar_3d::AmbientProbeGrid>();
        let irradiance_sh = world.get_resource::<lunar_3d::IrradianceSH>();

        for i in 0..self.draw_scratch.len() {
            let (_, _, _, color, metallic, roughness, model, _, mat_flags, lm_id, dir_lm_id) = self.draw_scratch[i];
            Self::pack_mesh_uniforms(&mut self.uniform_staging, ENTITY_SLOT_START + i, model);
            // write per-entity SH: probe grid takes priority, then global IrradianceSH, else zeros
            let world_pos = Vec3::new(model.w_axis.x, model.w_axis.y, model.w_axis.z);
            let sh_coeffs: Option<[[f32; 3]; 9]> =
                probe_grid.map(|g| g.sample(world_pos))
                .or_else(|| irradiance_sh.map(|s| s.coefficients));
            if let Some(coeffs) = sh_coeffs {
                Self::pack_sh_uniforms(&mut self.uniform_staging, ENTITY_SLOT_START + i, &coeffs);
            }
            let has_lightmap: u32 = if lm_id != u32::MAX { 1 } else { 0 };
            // bit 1 = has directional lightmap; only set when not in GPU indirect path (dir not atlased)
            let dir_flag: u32 = if dir_lm_id != u32::MAX && !self.has_indirect { 2 } else { 0 };
            let combined_flags = mat_flags | dir_flag;
            let (lm_uv_offset, lm_uv_scale) = if lm_id != u32::MAX {
                match self.atlas_lm_uvs.get(&lm_id) {
                    Some(&uvs) => ([uvs[0], uvs[1]], [uvs[2], uvs[3]]),
                    None => ([0.0f32, 0.0], [1.0f32, 1.0]),
                }
            } else {
                ([0.0f32, 0.0], [1.0f32, 1.0])
            };
            Self::pack_material_uniforms(&mut self.material_staging, ENTITY_SLOT_START + i, color, metallic, roughness, combined_flags, has_lightmap, lm_uv_offset, lm_uv_scale);
        }

        // ── pack lights buffer ────────────────────────────────────────────
        // assign shadow slots to first MAX_POINT_SHADOW_LIGHTS lights with casts_shadows=true
        let mut shadow_slot_idx: usize = 0;
        let shadow_indices: Vec<u32> = self.point_light_scratch.iter()
            .map(|&(_, _, _, _, casts)| {
                if casts && dev_point_shadows && shadow_slot_idx < MAX_POINT_SHADOW_LIGHTS {
                    let idx = shadow_slot_idx as u32;
                    shadow_slot_idx += 1;
                    idx
                } else {
                    0xffffffff
                }
            })
            .collect();

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct PointLightGpuCpu {
            position:    [f32; 3],
            intensity:   f32,
            color:       [f32; 3],
            radius:      f32,
            shadow_index: u32,
            _pad:        [u32; 3],
        }

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct LightsGpu {
            ambient_color:     [f32; 3],
            ambient_intensity: f32,
            dir_color:         [f32; 3],
            dir_illuminance:   f32,
            dir_direction:     [f32; 3],
            dir_enabled:       u32,
            light_space_0:     [f32; 16],
            light_space_1:     [f32; 16],
            light_space_2:     [f32; 16],
            cascade_splits:    [f32; 4],
            sh_enabled:        u32,
            _sh_pad:           [u32; 3],
            sh_coeffs:         [[f32; 4]; 9],
        }

        let sh = world.get_resource::<IrradianceSH>();
        let sh_enabled: u32 = if sh.is_some() { 1 } else { 0 };
        let mut sh_coeffs = [[0.0f32; 4]; 9];
        if let Some(sh) = sh {
            for (i, c) in sh.coefficients.iter().enumerate() {
                sh_coeffs[i] = [c[0], c[1], c[2], 0.0];
            }
        }

        let lights_gpu = LightsGpu {
            ambient_color: [ambient.color.r, ambient.color.g, ambient.color.b],
            ambient_intensity: ambient.intensity,
            dir_color: [dir_color.r, dir_color.g, dir_color.b],
            dir_illuminance,
            dir_direction: [dir_direction.x, dir_direction.y, dir_direction.z],
            dir_enabled,
            light_space_0: light_spaces[0].to_cols_array(),
            light_space_1: light_spaces[1].to_cols_array(),
            light_space_2: light_spaces[2].to_cols_array(),
            cascade_splits: [cascade_splits[0], cascade_splits[1], cascade_splits[2], SHADOW_FAR],
            sh_enabled,
            _sh_pad: [0; 3],
            sh_coeffs,
        };
        self.queue.write_buffer(&self.lights_buf, 0, bytemuck::bytes_of(&lights_gpu));

        // upload light list to storage buffer (for clustered path in group 5)
        let light_count = self.point_light_scratch.len();
        if light_count > 0 {
            self.light_data_scratch.clear();
            self.light_data_scratch.resize(light_count * 48, 0);
            for (i, &(pos, color, intensity, radius, _)) in self.point_light_scratch.iter().enumerate() {
                let off = i * 48;
                let entry = PointLightGpuCpu {
                    position: [pos.x, pos.y, pos.z],
                    intensity,
                    color: [color.r, color.g, color.b],
                    radius,
                    shadow_index: shadow_indices[i],
                    _pad: [0; 3],
                };
                self.light_data_scratch[off..off + 48].copy_from_slice(bytemuck::bytes_of(&entry));
            }
            self.queue.write_buffer(&self.light_list_buf, 0, &self.light_data_scratch);
        }

        // ── cluster params + CPU light assignment (pre-encoder) ──────────
        // upload ClusterParams; CPU path fills cluster data here.
        // compute path dispatch happens after encoder creation below.
        let cluster_needs_compute = light_count > MAX_POINT_LIGHTS && self.has_indirect;
        {
            let proj = camera.view_proj(cam_wt, aspect);
            let focal_x = proj.x_axis.x;
            let (near, far) = match camera.projection {
                Projection::Perspective { near, far, .. } => (near, far),
                Projection::Orthographic { near, far, .. } => (near, far),
            };
            let mut cp_data = [0u8; CLUSTER_PARAMS_SIZE as usize];
            cp_data[..64].copy_from_slice(bytemuck::cast_slice(&proj.to_cols_array()));
            let sw = self.surface_config.width;
            let sh_dim = self.surface_config.height;
            cp_data[64..68].copy_from_slice(bytemuck::cast_slice(&[sw]));
            cp_data[68..72].copy_from_slice(bytemuck::cast_slice(&[sh_dim]));
            cp_data[72..76].copy_from_slice(bytemuck::cast_slice(&[light_count as u32]));
            cp_data[76..80].copy_from_slice(bytemuck::cast_slice(&[0u32]));
            cp_data[80..84].copy_from_slice(bytemuck::cast_slice(&[near]));
            cp_data[84..88].copy_from_slice(bytemuck::cast_slice(&[far]));
            cp_data[88..92].copy_from_slice(bytemuck::cast_slice(&[focal_x]));
            cp_data[92..96].copy_from_slice(bytemuck::cast_slice(&[0f32]));
            self.queue.write_buffer(&self.cluster_params_buf, 0, &cp_data);

            if !cluster_needs_compute {
                // CPU path: all clusters point to the full light list
                self.cluster_counts_scratch.clear();
                self.cluster_counts_scratch.resize(NUM_CLUSTERS, 0);
                self.cluster_indices_scratch.clear();
                self.cluster_indices_scratch.resize(NUM_CLUSTERS * MAX_LIGHTS_PER_CLUSTER, 0);
                for c in 0..NUM_CLUSTERS {
                    self.cluster_counts_scratch[c] = light_count as u32;
                    for j in 0..light_count {
                        self.cluster_indices_scratch[c * MAX_LIGHTS_PER_CLUSTER + j] = j as u32;
                    }
                }
                self.queue.write_buffer(&self.cluster_counts_buf, 0, bytemuck::cast_slice(&self.cluster_counts_scratch));
                self.queue.write_buffer(&self.cluster_indices_buf, 0, bytemuck::cast_slice(&self.cluster_indices_scratch));
            }
        }

        // ── upload surface shader textures + stage params ─────────────────
        let surface_evict_ids: Vec<u32> = {
            let asset_server = world.resource::<lunar_assets::AssetServer>();
            let mut evict_ids: Vec<u32> = Vec::new();
            for &(_, slot, tex_ids, packed_stages) in &self.surface_scratch {
                // upload any new textures
                for &tid in &tex_ids {
                    if tid != u32::MAX && !self.surface_tex_cache.contains_key(&tid)
                        && let Some(tex) = asset_server.get_texture_by_id(tid) {
                            let (gpu_fmt, bpr) = match tex.compression {
                                lunar_assets::TextureCompression::None =>
                                    (wgpu::TextureFormat::Rgba8UnormSrgb, tex.width * 4),
                                lunar_assets::TextureCompression::Bc1 =>
                                    (wgpu::TextureFormat::Bc1RgbaUnormSrgb, tex.width.div_ceil(4) * 8),
                                lunar_assets::TextureCompression::Bc3 =>
                                    (wgpu::TextureFormat::Bc3RgbaUnorm, tex.width.div_ceil(4) * 16),
                                lunar_assets::TextureCompression::Bc5 =>
                                    (wgpu::TextureFormat::Bc5RgUnorm, tex.width.div_ceil(4) * 16),
                                lunar_assets::TextureCompression::Bc6h =>
                                    (wgpu::TextureFormat::Bc6hRgbFloat, tex.width.div_ceil(4) * 16),
                                lunar_assets::TextureCompression::Bc7 =>
                                    (wgpu::TextureFormat::Bc7RgbaUnorm, tex.width.div_ceil(4) * 16),
                            };
                            let rows_per_image = match tex.compression {
                                lunar_assets::TextureCompression::None => tex.height,
                                _ => tex.height.div_ceil(4),
                            };
                            let gpu_tex = self.device.create_texture(&wgpu::TextureDescriptor {
                                label: Some("[surface] tex"),
                                size: wgpu::Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
                                mip_level_count: tex.mip_level_count(),
                                sample_count: 1, dimension: wgpu::TextureDimension::D2,
                                format: gpu_fmt,
                                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                                view_formats: &[],
                            });
                            self.queue.write_texture(gpu_tex.as_image_copy(), &tex.pixels,
                                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(rows_per_image) },
                                wgpu::Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 });
                            let view = gpu_tex.create_view(&Default::default());
                            self.surface_tex_cache.insert(tid, (gpu_tex, view));
                            evict_ids.push(tid);
                        }
                }
                // upload stage params for this entity
                let slot_offset = (slot - (ENTITY_SLOT_START + self.draw_scratch.len())) * UNIFORM_STRIDE as usize;
                if slot_offset + 128 <= 64 * UNIFORM_STRIDE as usize {
                    let mut stage_data = [0u8; 128];
                    for (i, &stage) in packed_stages.iter().enumerate() {
                        let off = i * 32;
                        stage_data[off..off + 8].copy_from_slice(bytemuck::cast_slice(&stage.uv_offset));
                        stage_data[off + 8..off + 12].copy_from_slice(bytemuck::cast_slice(&[stage.uv_scale]));
                        stage_data[off + 12..off + 16].copy_from_slice(bytemuck::cast_slice(&[stage.blend]));
                        stage_data[off + 16..off + 20].copy_from_slice(bytemuck::cast_slice(&[stage.alpha]));
                        stage_data[off + 20..off + 24].copy_from_slice(bytemuck::cast_slice(&[stage.use_lm_uv]));
                        stage_data[off + 24..off + 28].copy_from_slice(bytemuck::cast_slice(&[stage.enabled]));
                        stage_data[off + 28..off + 32].copy_from_slice(bytemuck::cast_slice(&[stage._pad]));
                    }
                    self.queue.write_buffer(&self.surface_params_buf, slot_offset as u64, &stage_data);
                }
                // create/update BG if texture combination changed
                if !self.surface_bg_cache.contains_key(&tex_ids) {
                    let get_view = |tid: u32| -> &wgpu::TextureView {
                        if tid != u32::MAX
                            && let Some((_, v)) = self.surface_tex_cache.get(&tid) { return v; }
                        &self.surface_fallback_view
                    };
                    let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("[surface] stage bg"),
                        layout: &self.surface_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.surface_params_buf, offset: 0, size: wgpu::BufferSize::new(128),
                            })},
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(get_view(tex_ids[0])) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(get_view(tex_ids[1])) },
                            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(get_view(tex_ids[2])) },
                            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(get_view(tex_ids[3])) },
                            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.surface_sampler) },
                        ],
                    });
                    self.surface_bg_cache.insert(tex_ids, bg);
                }
            }
            evict_ids
        };
        // evict cpu-side data for newly uploaded surface textures
        if !surface_evict_ids.is_empty() {
            let mut asset_server = world.resource_mut::<lunar_assets::AssetServer>();
            for id in surface_evict_ids {
                if let Some(tex) = asset_server.get_texture_by_id_mut(id) {
                    tex.evict_cpu_data();
                }
            }
        }

        // ── upload shadow globals (one slot per cascade) ──────────────────
        for (i, &ls) in light_spaces.iter().enumerate() {
            let cols = ls.to_cols_array();
            self.queue.write_buffer(
                &self.shadow_globals_buf,
                (i * UNIFORM_STRIDE as usize) as u64,
                bytemuck::cast_slice(&cols),
            );
        }

        // ── upload globals + small uniforms via queue.write_buffer ───────
        let upload_size = (needed * UNIFORM_STRIDE as usize) as u64;
        let time = world.resource::<lunar_core::Time>();
        let globals_data: [f32; 24] = {
            let vp = view_proj.to_cols_array();
            let mut d = [0f32; 24];
            d[..16].copy_from_slice(&vp);
            d[16] = cam_pos.x;
            d[17] = cam_pos.y;
            d[18] = cam_pos.z;
            d[19] = time.elapsed_seconds();
            d[20] = time.delta_seconds();
            // d[21] = shading_era (u32, stays 0 = ERA_MODERN)
            // d[22] = render_flags (u32 packed as f32 bits)
            //   bit 0: soft_shadows (PCSS directional + soft point)
            //   bit 1: contact_shadows (screen-space contact shadow pass active)
            let mut render_flags = 0u32;
            if dev_soft_shadows    { render_flags |= 1; }
            if dev_contact_shadows { render_flags |= 2; }
            d[22] = f32::from_bits(render_flags);
            d
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::cast_slice(&globals_data));

        // ── sort transparent draws back-to-front ──────────────────────────
        let cam_fwd = cam_wt.forward();
        self.transparent_scratch.clear();
        for i in 0..self.draw_scratch.len() {
            if self.draw_scratch[i].7 < 1.0 {
                self.transparent_scratch.push(i);
            }
        }
        // skip re-sort when camera direction and all transparent entity depths match
        // the previous frame within 1mm (quantized to i32 millimetres)
        let cur_depths: Vec<i32> = self.transparent_scratch.iter().map(|&i| {
            let w = self.draw_scratch[i].6.w_axis;
            ((Vec3::new(w.x, w.y, w.z) - cam_pos).dot(cam_fwd) * 1000.0) as i32
        }).collect();
        let cam_fwd_changed = (cam_fwd - self.transparent_last_cam_fwd).length_squared() > 1e-8;
        if cam_fwd_changed || cur_depths != self.transparent_last_depths {
            self.transparent_scratch.sort_unstable_by(|&a, &b| {
                let wa = self.draw_scratch[a].6.w_axis;
                let wb = self.draw_scratch[b].6.w_axis;
                let depth_a = (Vec3::new(wa.x, wa.y, wa.z) - cam_pos).dot(cam_fwd);
                let depth_b = (Vec3::new(wb.x, wb.y, wb.z) - cam_pos).dot(cam_fwd);
                depth_b.partial_cmp(&depth_a).unwrap_or(std::cmp::Ordering::Equal)
            });
            self.transparent_last_depths = cur_depths;
            self.transparent_last_cam_fwd = cam_fwd;
        }

        // ── acquire surface and create encoder ────────────────────────────
        let mut reconfigure_after_present = false;
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                // defer reconfigure until after present — can't configure while frame is alive
                reconfigure_after_present = true;
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.surface_config);
                return 0;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                return 0;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded => return 0,
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("wgpu validation error acquiring surface texture");
                return 0;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("[frame] encoder"),
        });

        // ── cluster compute dispatch (high tier, >8 lights) ─────────────
        if cluster_needs_compute {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("[cluster] assign pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.cluster_pipeline);
            cpass.set_bind_group(0, &self.cluster_bg_compute, &[]);
            cpass.dispatch_workgroups(CLUSTER_X, CLUSTER_Y, CLUSTER_Z);
        }

        // ── render graph pass ordering ────────────────────────────────────
        // the graph's topological sort drives pass execution order.
        // each frame we log the ordered pass names (debug only) so the DAG
        // is actually driving execution, not just present as dead data.
        {
            // compute the topological pass order and log it in debug builds.
            // this is the render graph DAG driving execution; the sorted order
            // replaces the hardcoded sequential pass list.
            let pass_ids: Vec<_> = self.render_graph.sorted_pass_ids().to_vec();
            if cfg!(debug_assertions) {
                let names: Vec<&str> = pass_ids.iter()
                    .map(|&id| self.render_graph.pass_name(id))
                    .collect();
                log::trace!("[render-graph] pass order: {names:?}");
            }
        }

        // ── upload mesh + material buffers ───────────────────────────────
        let material_upload_size = (needed * MATERIAL_UNIFORMS_SIZE as usize) as u64;
        if upload_size > 0 {
            #[cfg(not(target_arch = "wasm32"))]
            {
                // StagingBelt batches large per-frame uploads into GPU-side staging memory
                let entity_size = wgpu::BufferSize::new(upload_size).unwrap();
                let mat_size = wgpu::BufferSize::new(material_upload_size).unwrap();
                let mut view = self.staging_belt.write_buffer(
                    &mut encoder, &self.entity_buf, 0, entity_size,
                );
                view.copy_from_slice(&self.uniform_staging[..upload_size as usize]);
                drop(view);
                let mut view = self.staging_belt.write_buffer(
                    &mut encoder, &self.material_buf, 0, mat_size,
                );
                view.copy_from_slice(&self.material_staging[..material_upload_size as usize]);
            }
            #[cfg(target_arch = "wasm32")]
            {
                self.queue.write_buffer(&self.entity_buf, 0, &self.uniform_staging[..upload_size as usize]);
                self.queue.write_buffer(&self.material_buf, 0, &self.material_staging[..material_upload_size as usize]);
            }
        }

        // ── build indirect draw args (high tier + INDIRECT_FIRST_INSTANCE) ──
        // scans opaque batches once, writes DrawIndexedIndirect entries (5×u32 each).
        // render pass then uses draw_indexed_indirect per batch instead of draw_indexed.
        // phase 4 (GPU-driven indirect) supersedes phase 2 (CPU-built indirect)
        let _opaque_indirect_count: u32 = if self.has_indirect && !self.gpu_indirect_active() {
            self.indirect_args.clear();
            let n = self.draw_scratch.len();
            let mut i = 0usize;
            let mut last_mesh = u32::MAX;
            let mut last_mat = u32::MAX;
            let mut last_lm = u32::MAX;
            let mut last_dir_lm = u32::MAX;
            let mut group_start = 0usize;
            while i <= n {
                let transparent_or_end = i == n || self.draw_scratch[i].7 < 1.0;
                let (cur_mesh, cur_mat, cur_lm, cur_dir_lm) = if transparent_or_end {
                    (u32::MAX, u32::MAX, u32::MAX, u32::MAX)
                } else {
                    (self.draw_scratch[i].1, self.draw_scratch[i].2, self.draw_scratch[i].9, self.draw_scratch[i].10)
                };
                let group_changed = cur_mesh != last_mesh || cur_mat != last_mat || cur_lm != last_lm || cur_dir_lm != last_dir_lm;
                if group_changed && i > group_start
                    && let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) {
                        let base = (ENTITY_SLOT_START + group_start) as u32;
                        let count = (i - group_start) as u32;
                        // DrawIndexedIndirect: index_count, instance_count, first_index, base_vertex, first_instance
                        self.indirect_args.extend_from_slice(&[gpu_mesh.index_count, count, 0, 0u32, base]);
                    }
                if transparent_or_end { break; }
                if group_changed { last_mesh = cur_mesh; last_mat = cur_mat; last_lm = cur_lm; last_dir_lm = cur_dir_lm; group_start = i; }
                i += 1;
            }
            let needed_bytes = (self.indirect_args.len() * 4) as u64;
            if needed_bytes > 0 {
                let current_cap = self.indirect_buf.as_ref().map(|b| b.size()).unwrap_or(0);
                if needed_bytes > current_cap {
                    self.indirect_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("[indirect] opaque draw args"),
                        size: (self.entity_capacity * 20) as u64,
                        usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
                self.queue.write_buffer(
                    self.indirect_buf.as_ref().unwrap(), 0,
                    bytemuck::cast_slice(&self.indirect_args),
                );
            }
            (self.indirect_args.len() / 5) as u32
        } else { 0 };

        // ── late GPU indirect cull (phase 4) ─────────────────────────────
        // runs after draw_scratch is built. dispatches cull_indirect_pipeline:
        // GPU tests each draw_scratch entity's AABB and writes DrawIndexedIndirect
        // commands for visible entities into indirect_buf.
        // phase 5: the early-frame cull readback (item L) remains for game code;
        // the render path uses indirect_buf directly (no CPU readback for rendering).
        if self.gpu_indirect_active() {
            let entity_count = self.draw_scratch.len();
            if entity_count > 0 {
                self.ensure_gpu_cull_resources(entity_count);

                // build late AABB data in draw_scratch order
                self.late_aabb_scratch.clear();
                for i in 0..entity_count {
                    let entity = self.draw_scratch[i].0;
                    let (center, half) = match world.get::<Aabb3d>(entity) {
                        Some(aabb) => (Vec3::from(aabb.center), Vec3::from(aabb.half_extents)),
                        None => (Vec3::ZERO, Vec3::splat(1e6)),
                    };
                    self.late_aabb_scratch.extend_from_slice(&[center.x, center.y, center.z, 0.0, half.x, half.y, half.z, 0.0]);
                }

                // build draw params in draw_scratch order: [index_count, first_index, base_vertex, first_instance]
                self.dp_data_scratch.clear();
                for i in 0..entity_count {
                    let mesh_id = self.draw_scratch[i].1;
                    let slot = (ENTITY_SLOT_START + i) as u32;
                    if let Some(entry) = self.mega_mesh_entries.get(&mesh_id) {
                        self.dp_data_scratch.extend_from_slice(&[entry[1], entry[0], entry[2], slot]);
                    } else {
                        self.dp_data_scratch.extend_from_slice(&[0, 0, 0, slot]);
                    }
                }

                // build late frustum params with draw_scratch entity_count
                let frustum = *world.resource::<Frustum>();
                let planes = frustum.planes;
                let mut late_fp = [0f32; 32];
                for (p, plane) in planes.iter().enumerate() {
                    late_fp[p * 4] = plane.x; late_fp[p * 4 + 1] = plane.y;
                    late_fp[p * 4 + 2] = plane.z; late_fp[p * 4 + 3] = plane.w;
                }
                late_fp[24] = f32::from_bits(entity_count as u32);

                let aabb_buf = self.cull_aabb_buf.as_ref().unwrap();
                let late_fp_buf = self.late_cull_frustum_buf.as_ref().unwrap();
                let flags_buf = self.cull_flags_buf.as_ref().unwrap();
                let dp_buf = self.cull_draw_params_buf.as_ref().unwrap();
                let ind_buf = self.indirect_buf.as_ref().unwrap();
                let cnt_buf = self.cull_indirect_count_buf.as_ref().unwrap();

                self.queue.write_buffer(aabb_buf, 0, bytemuck::cast_slice(&self.late_aabb_scratch));
                self.queue.write_buffer(late_fp_buf, 0, bytemuck::cast_slice(&late_fp));
                self.queue.write_buffer(dp_buf, 0, bytemuck::cast_slice(&self.dp_data_scratch));
                self.queue.write_buffer(cnt_buf, 0, bytemuck::bytes_of(&0u32));

                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[late cull] bg"),
                    layout: self.cull_indirect_bgl.as_ref().unwrap(),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: aabb_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: late_fp_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: flags_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: dp_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: ind_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 5, resource: cnt_buf.as_entire_binding() },
                    ],
                });
                let mut late_enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("[late cull indirect]"),
                });
                {
                    let mut cpass = late_enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("[late cull indirect] pass"), timestamp_writes: None,
                    });
                    cpass.set_pipeline(self.cull_indirect_pipeline.as_ref().unwrap());
                    cpass.set_bind_group(0, &bg, &[]);
                    cpass.dispatch_workgroups((entity_count as u32).div_ceil(64), 1, 1);
                }
                self.queue.submit([late_enc.finish()]);
            }
        }

        self.record_shadows(world, &mut encoder, dir_direction, dir_enabled, dir_casts_shadows);

        // ── HZB build (high tier only) ───────────────────────────────────
        // builds a hierarchical min-depth buffer from the z-prepass result.
        // used next frame by cs_cull_hzb to occlude entities behind opaque geometry.
        if self.hzb_enabled {
            self.ensure_hzb_resources();

            // depth-only non-MSAA prepass into hzb_depth_src
            {
                let depth_src_view = self.hzb_depth_src_view.as_ref().unwrap();
                let mut hzb_zpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[hzb] depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_src_view,
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
                hzb_zpass.set_pipeline(&self.zprepass_nonmsaa_pipeline);
                hzb_zpass.set_bind_group(0, &self.globals_bg, &[]);
                hzb_zpass.set_bind_group(1, &self.material_bg, &[]);
                hzb_zpass.set_bind_group(2, &self.entity_bg, &[]);
                hzb_zpass.set_bind_group(3, &self.lights_bg, &[]);
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
                            hzb_zpass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + (i - group_start) as u32);
                        }
                    if done { break; }
                    if cur_mesh != last_mesh || cur_mat != last_mat {
                        if let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) {
                            hzb_zpass.set_vertex_buffer(0, gpu_mesh.pos_buf.slice(..));
                            hzb_zpass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                        }
                        last_mesh = cur_mesh; last_mat = cur_mat; group_start = i;
                    }
                    i += 1;
                }
            }

            // copy depth → HZB mip 0
            {
                let depth_src_view = self.hzb_depth_src_view.as_ref().unwrap();
                let copy_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[hzb] copy bg"),
                    layout: self.hzb_copy_bgl.as_ref().unwrap(),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth_src_view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hzb_mip_views[0]) },
                    ],
                });
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("[hzb] copy pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(self.hzb_copy_pipeline.as_ref().unwrap());
                cpass.set_bind_group(0, &copy_bg, &[]);
                let wg_x = self.hzb_width.div_ceil(8);
                let wg_y = self.hzb_height.div_ceil(8);
                cpass.dispatch_workgroups(wg_x, wg_y, 1);
            }

            // downsample each mip level
            let ds_pipeline = self.hzb_downsample_pipeline.as_ref().unwrap();
            let ds_bgl = self.hzb_downsample_bgl.as_ref().unwrap();
            for mip in 1..self.hzb_mip_count as usize {
                let src_view = &self.hzb_mip_views[mip - 1];
                let dst_view = &self.hzb_mip_views[mip];
                let mip_w = (self.hzb_width >> mip).max(1);
                let mip_h = (self.hzb_height >> mip).max(1);
                let ds_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("[hzb] downsample mip {mip}")),
                    layout: ds_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(dst_view) },
                    ],
                });
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(&format!("[hzb] downsample mip {mip}")),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(ds_pipeline);
                cpass.set_bind_group(0, &ds_bg, &[]);
                cpass.dispatch_workgroups(mip_w.div_ceil(8), mip_h.div_ceil(8), 1);
            }
        }

        // bundle read-only per-frame state once; shared by the pre-color and post passes
        let fc = FrameContext { view_proj, staa_jitter_ndc, cam_pos, cam_wt, aspect, camera, sky, dir_illuminance, dir_enabled, vp_x, vp_y, vp_w, vp_h, dev_bloom, dev_ssao, dev_ssr, dev_fog, dev_fxaa, dev_staa, dev_vignette, dev_chrom_ab, dev_film_grain, dev_contact_shadows, upscale_mode, dir_color, dir_direction, sky_color };
        self.record_gtao_reflection(&fc, world, &mut encoder);
        let draw_calls = self.record_scene_passes(&fc, world, &mut encoder);
        self.record_post_processing(&fc, world, &mut encoder, view);
        #[cfg(not(target_arch = "wasm32"))]
        self.staging_belt.finish();
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        if reconfigure_after_present {
            self.surface.configure(&self.device, &self.surface_config);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.staging_belt.recall();
        draw_calls
    }
    /// update the EMA frame-time tracker and adjust resolution scale.
    /// called by `render_3d_system` after each frame with the measured CPU frame time.
    pub fn tick_dynamic_resolution(&mut self, frame_time_ms: f32) -> f32 {
        // EMA with α=0.1 (smooths over ~10 frames)
        const ALPHA: f32 = 0.1;
        self.frame_time_ema_ms = ALPHA * frame_time_ms + (1.0 - ALPHA) * self.frame_time_ema_ms;

        let budget = self.frame_time_budget_ms;
        if self.frame_time_ema_ms > budget * 0.95 {
            // over 95% of budget: drop 5%, floor at 0.5
            self.resolution_scale = (self.resolution_scale - 0.05).max(0.5);
            self.auto_quality_over_frames += 1;
            self.auto_quality_under_frames = 0;
        } else if self.frame_time_ema_ms < budget * 0.80 {
            // under 80% of budget: raise 5%, ceil at 1.0
            self.resolution_scale = (self.resolution_scale + 0.05).min(1.0);
            self.auto_quality_under_frames += 1;
            self.auto_quality_over_frames = 0;
        } else {
            self.auto_quality_over_frames = 0;
            self.auto_quality_under_frames = 0;
        }
        self.resolution_scale
    }
    #[inline(always)]
    pub(crate) fn slot_offset(slot: usize) -> u32 {
        (slot * UNIFORM_STRIDE as usize) as u32
    }
    /// compute cascade split depths using logarithmic-linear blending.
    /// returns `n` split values in view-space depth (positive distance from camera).
    pub(crate) fn compute_cascade_splits(near: f32, far: f32, n: usize, lambda: f32) -> Vec<f32> {
        (1..=n)
            .map(|i| {
                let uniform = near + (far - near) * (i as f32 / n as f32);
                let log = near * (far / near).powf(i as f32 / n as f32);
                lambda * log + (1.0 - lambda) * uniform
            })
            .collect()
    }
    /// compute a tight orthographic light-space matrix for one cascade slice.
    /// fits the ortho projection to the 8 corners of the camera frustum slice.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cascade_light_space(
        cam_pos: Vec3,
        cam_fwd: Vec3,
        cam_up: Vec3,
        cam_right: Vec3,
        fov_y: f32,
        aspect: f32,
        light_dir: Vec3,
        slice_near: f32,
        slice_far: f32,
    ) -> Mat4 {
        let tan_half = (fov_y * 0.5).tan();
        let corners: [Vec3; 8] = {
            let mut c = [Vec3::ZERO; 8];
            let mut idx = 0;
            for &depth in &[slice_near, slice_far] {
                let half_h = tan_half * depth;
                let half_w = half_h * aspect;
                for sy in [-1.0_f32, 1.0] {
                    for sx in [-1.0_f32, 1.0] {
                        c[idx] = cam_pos + cam_fwd * depth + cam_up * (sy * half_h) + cam_right * (sx * half_w);
                        idx += 1;
                    }
                }
            }
            c
        };

        // centroid of corners → light looks at it
        let centroid = corners.iter().fold(Vec3::ZERO, |acc, &c| acc + c) / 8.0;
        let light_dir_n = light_dir.normalize();
        let light_up = if light_dir_n.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let light_view = Mat4::look_at_rh(centroid - light_dir_n * 100.0, centroid, light_up);

        // AABB of corners in light view space
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
        let (mut min_z, mut max_z) = (f32::MAX, f32::MIN);
        for &c in &corners {
            let lc = light_view * Vec3::new(c.x, c.y, c.z).extend(1.0);
            min_x = min_x.min(lc.x); max_x = max_x.max(lc.x);
            min_y = min_y.min(lc.y); max_y = max_y.max(lc.y);
            min_z = min_z.min(lc.z); max_z = max_z.max(lc.z);
        }
        // pull near plane back to catch casters behind the frustum
        let z_extend = (max_z - min_z) * 0.5;

        // texel snapping: quantise ortho center to the nearest shadow-map texel in world space.
        // without this, sub-texel camera movement shifts the texel grid causing shadow shimmer.
        let extent_x = max_x - min_x;
        let extent_y = max_y - min_y;
        let texel_x = extent_x / SHADOW_MAP_SIZE as f32;
        let texel_y = extent_y / SHADOW_MAP_SIZE as f32;
        let cx = ((min_x + max_x) * 0.5 / texel_x).round() * texel_x;
        let cy = ((min_y + max_y) * 0.5 / texel_y).round() * texel_y;
        let half_x = extent_x * 0.5;
        let half_y = extent_y * 0.5;
        let (min_x, max_x) = (cx - half_x, cx + half_x);
        let (min_y, max_y) = (cy - half_y, cy + half_y);

        let light_proj = Mat4::orthographic_rh(min_x, max_x, min_y, max_y, min_z - z_extend, max_z + z_extend);
        light_proj * light_view
    }
}
