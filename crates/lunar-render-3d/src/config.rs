//! `RenderEngine3d` — resize, render scale, msaa, quality-preset stepping, accessors.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

impl RenderEngine3d {
	pub fn tier(&self) -> RenderTier {
		self.render_tier
	}
	pub(crate) fn gpu_indirect_active(&self) -> bool {
		self.has_indirect
			&& self.cull_indirect_pipeline.is_some()
			&& !self.mega_mesh_entries.is_empty()
	}
	pub fn resize(&mut self, width: u32, height: u32) {
		if width == 0 || height == 0 {
			return;
		}
		if self.surface_config.width == width && self.surface_config.height == height {
			return;
		}
		self.surface_config.width = width;
		self.surface_config.height = height;
		self.surface.configure(&self.device, &self.surface_config);
		// compute render resolution (may be smaller than display when render_scale < 1.0)
		let render_w = ((width as f32 * self.render_scale).ceil() as u32).max(1);
		let render_h = ((height as f32 * self.render_scale).ceil() as u32).max(1);
		self.render_w = render_w;
		self.render_h = render_h;
		self.depth_view =
			Self::make_depth_view(&self.device, render_w, render_h, self.msaa_samples);
		self.msaa_color_view = Self::make_msaa_color_view(
			&self.device,
			render_w,
			render_h,
			self.hdr_format,
			self.msaa_samples,
		);
		let (hdr_texture, hdr_view) =
			Self::make_hdr_texture(&self.device, render_w, render_h, self.hdr_format);
		let n = self.bloom_mip_views.len();
		let (mip_views, mip_sizes, ds_bgs, us_bgs) = Self::build_bloom_resources(
			&self.device,
			&hdr_texture,
			&self.bloom_params_buf,
			&self.bloom_downsample_bgl,
			&self.post_sampler,
			render_w,
			render_h,
			n,
			self.hdr_format,
		);
		// store new resources before rebuilding composite bind group
		self.hdr_texture = hdr_texture;
		self.hdr_view = hdr_view;
		self.bloom_mip_views = mip_views;
		self.bloom_mip_sizes = mip_sizes;
		self.bloom_downsample_bgs = ds_bgs;
		self.bloom_upsample_bgs = us_bgs;

		// rebuild GTAO depth and AO textures at the new render resolution
		let ao_w = (render_w / 2).max(1);
		let ao_h = (render_h / 2).max(1);
		self.gtao_depth_view = Self::make_depth_view(&self.device, render_w, render_h, 1);
		let gtao_ao_a = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[gtao] ao ping"),
			size: wgpu::Extent3d {
				width: ao_w,
				height: ao_h,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rg16Float,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let gtao_ao_b = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[gtao] ao pong"),
			size: wgpu::Extent3d {
				width: ao_w,
				height: ao_h,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rg16Float,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		self.gtao_ao_view_a = gtao_ao_a.create_view(&wgpu::TextureViewDescriptor::default());
		self.gtao_ao_view_b = gtao_ao_b.create_view(&wgpu::TextureViewDescriptor::default());
		self.gtao_ao_a = gtao_ao_a;
		self.gtao_ao_b = gtao_ao_b;

		// rebuild GTAO bind groups (they reference the new gtao_depth_view and ao views)
		self.gtao_main_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[gtao] main bg"),
			layout: &self.gtao_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.gtao_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&self.gtao_ao_view_b),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&self.gtao_point_sampler),
				},
			],
		});
		self.gtao_blur_h_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[gtao] blur-h bg"),
			layout: &self.gtao_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.gtao_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&self.gtao_ao_view_a),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&self.gtao_point_sampler),
				},
			],
		});
		self.gtao_blur_v_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[gtao] blur-v bg"),
			layout: &self.gtao_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.gtao_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&self.gtao_ao_view_b),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&self.gtao_point_sampler),
				},
			],
		});

		// rebuild SSR and fog textures at the new render resolution
		let ssr_hw = (render_w / 2).max(1);
		let ssr_hh = (render_h / 2).max(1);
		let ssr_texture = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[ssr] reflection texture"),
			size: wgpu::Extent3d {
				width: ssr_hw,
				height: ssr_hh,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: self.hdr_format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let ssr_view = ssr_texture.create_view(&wgpu::TextureViewDescriptor::default());
		let fog_texture = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[fog] scattering texture"),
			size: wgpu::Extent3d {
				width: ssr_hw,
				height: ssr_hh,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: self.hdr_format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let fog_view = fog_texture.create_view(&wgpu::TextureViewDescriptor::default());

		// rebuild SSR bg0 with new depth view
		self.ssr_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[ssr] bg0"),
			layout: &self.ssr_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.hdr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
			],
		});
		self.fog_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[fog] bg0"),
			layout: &self.fog_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
			],
		});
		self.atmos_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[atmos] bg0"),
			layout: &self.atmos_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
			],
		});
		self.decal_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[decal] bg0"),
			layout: &self.decal_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
			],
		});

		self.ssr_view = ssr_view;
		self.ssr_texture = ssr_texture;
		self.fog_view = fog_view;
		self.fog_texture = fog_texture;

		// rebuild water bg0 with the new hdr_view (for refraction sampling).
		// binding 3 is the planar reflection texture (1×1 fallback when disabled) — must be
		// included or the bind group won't match water_bgl0's 4-entry layout.
		let refl_v = self
			.reflection_view
			.as_ref()
			.unwrap_or(&self.reflection_fallback_view);
		self.water_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[water] bg0"),
			layout: &self.water_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.hdr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(refl_v),
				},
			],
		});

		// rebuild composite bind group (binding 4=ssr, 5=fog, 6=sampler, 7=contact shadow).
		// binding 7 is the contact-shadow texture (1×1 fallback when disabled) — must be
		// included or the bind group won't match composite_bgl's 8-entry layout.
		let bloom_view = self.bloom_mip_views.first().unwrap_or(&self.hdr_view);
		let cs_view_ref = self
			.contact_shadow_view
			.as_ref()
			.unwrap_or(&self.contact_shadow_fallback_view);
		self.composite_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[composite] bg"),
			layout: &self.composite_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.composite_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&self.hdr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(bloom_view),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&self.gtao_ao_view_a),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::TextureView(&self.ssr_view),
				},
				wgpu::BindGroupEntry {
					binding: 5,
					resource: wgpu::BindingResource::TextureView(&self.fog_view),
				},
				wgpu::BindGroupEntry {
					binding: 6,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 7,
					resource: wgpu::BindingResource::TextureView(cs_view_ref),
				},
			],
		});

		// rebuild fxaa/taa ldr texture and bind groups at the new resolution
		let fxaa_ldr_texture = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[fxaa] ldr texture"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: self.surface_config.format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let fxaa_ldr_view = fxaa_ldr_texture.create_view(&wgpu::TextureViewDescriptor::default());
		self.fxaa_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[fxaa] bg"),
			layout: &self.fxaa_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.fxaa_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
			],
		});

		// rebuild taa history textures and bind groups at the new resolution
		let staa_history_a_texture = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[staa] history A"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: STAA_HISTORY_FORMAT,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let staa_history_a_view =
			staa_history_a_texture.create_view(&wgpu::TextureViewDescriptor::default());
		let staa_history_b_texture = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[staa] history B"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: STAA_HISTORY_FORMAT,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let staa_history_b_view =
			staa_history_b_texture.create_view(&wgpu::TextureViewDescriptor::default());

		self.staa_bg_even = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[staa] bg even"),
			layout: &self.staa_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.staa_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&staa_history_a_view),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 5,
					resource: wgpu::BindingResource::Sampler(&self.staa_nearest_sampler),
				},
			],
		});
		self.staa_bg_odd = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[staa] bg odd"),
			layout: &self.staa_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: self.staa_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&staa_history_b_view),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&self.post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 5,
					resource: wgpu::BindingResource::Sampler(&self.staa_nearest_sampler),
				},
			],
		});
		// reset: force cold start on next frame so stale history doesn't bleed through
		self.staa_frame_index = 0;
		self.staa_ping = false;

		self.staa_history_a_texture = staa_history_a_texture;
		self.staa_history_a_view = staa_history_a_view;
		self.staa_history_b_texture = staa_history_b_texture;
		self.staa_history_b_view = staa_history_b_view;

		self.fxaa_ldr_view = fxaa_ldr_view;
		self.fxaa_ldr_texture = fxaa_ldr_texture;

		// rebuild upscale resources at the new dimensions
		if self.upscale_active {
			let dw = self.surface_config.width;
			let dh = self.surface_config.height;
			self.ensure_upscale_resources(render_w, render_h, dw, dh);
		}
	}
	/// apply a new render scale without resizing the display surface.
	/// recreates all render-resolution textures at the new size.
	pub fn set_render_scale(&mut self, render_scale: f32) {
		let scale = render_scale.clamp(0.1, 1.0);
		if (scale - self.render_scale).abs() < 1e-4 {
			return;
		}
		self.render_scale = scale;
		let dw = self.surface_config.width;
		let dh = self.surface_config.height;
		if dw > 0 && dh > 0 {
			// resize() early-returns when surface dimensions are unchanged — bypass by
			// clearing the stored size so it sees a dimension change and rebuilds textures
			self.surface_config.width = 0;
			self.resize(dw, dh);
		}
	}
	pub fn surface_width(&self) -> u32 {
		self.surface_config.width
	}
	pub fn surface_height(&self) -> u32 {
		self.surface_config.height
	}
	pub fn render_tier(&self) -> RenderTier {
		self.render_tier
	}
	/// apply a new MSAA sample count. rebuilds all MSAA-dependent pipelines and
	/// the depth/MSAA color views. causes a brief gpu stall (hidden by a settings menu).
	pub fn apply_msaa_change(&mut self, samples: u32) {
		let samples = match samples {
			4 => samples,
			_ => 1,
		}; // WebGPU spec guarantees only 1 and 4 for Depth32Float
		if samples == self.msaa_samples {
			return;
		}
		self.msaa_samples = samples;
		let w = self.render_w;
		let h = self.render_h;
		self.depth_view = Self::make_depth_view(&self.device, w, h, samples);
		self.msaa_color_view =
			Self::make_msaa_color_view(&self.device, w, h, self.hdr_format, samples);
		self.rebuild_msaa_pipelines();
		// force static bundle rebuild next frame
		self.static_bundle = None;
		log::info!("msaa changed to {samples}x");
	}
	pub(crate) fn rebuild_msaa_pipelines(&mut self) {
		let msaa = self.msaa_samples;
		let hdr = self.hdr_format;
		let tier = self.render_tier;

		#[cfg(not(target_arch = "wasm32"))]
		let cache = self.pipeline_cache.as_ref();
		#[cfg(target_arch = "wasm32")]
		let cache: Option<&wgpu::PipelineCache> = None;

		// vertex buffer layouts — same constants as from_surface
		let vert_attrs = [
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Float32x3,
				offset: 0,
				shader_location: 0,
			},
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Snorm8x4,
				offset: 12,
				shader_location: 1,
			},
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Snorm8x4,
				offset: 16,
				shader_location: 2,
			},
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Unorm16x2,
				offset: 20,
				shader_location: 3,
			},
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Unorm16x2,
				offset: 24,
				shader_location: 4,
			},
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Unorm8x4,
				offset: 28,
				shader_location: 5,
			},
		];
		let vertex_buffers = [wgpu::VertexBufferLayout {
			array_stride: VERTEX_STRIDE,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &vert_attrs,
		}];
		let pos_attrs = [wgpu::VertexAttribute {
			format: wgpu::VertexFormat::Float32x3,
			offset: 0,
			shader_location: 0,
		}];
		let pos_vertex_buffers = [wgpu::VertexBufferLayout {
			array_stride: POS_VERTEX_STRIDE,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &pos_attrs,
		}];

		// recreate pipeline layouts from stored bgls
		let main_layout = self
			.device
			.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("3d pipeline layout"),
				bind_group_layouts: &[
					Some(&self.globals_bgl),
					Some(&self.material_bgl),
					Some(&self.mesh_bgl),
					Some(&self.lights_bgl),
					Some(&self.lightmap_bgl),
					Some(&self.cluster_bgl_render),
				],
				immediate_size: 0,
			});
		let zprepass_layout = self
			.device
			.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("3d z-prepass pipeline layout"),
				bind_group_layouts: &[
					Some(&self.globals_bgl),
					Some(&self.material_bgl),
					Some(&self.mesh_bgl),
				],
				immediate_size: 0,
			});
		let surface_layout = self
			.device
			.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[surface] pipeline layout"),
				bind_group_layouts: &[
					Some(&self.globals_bgl),
					Some(&self.mesh_bgl),
					Some(&self.surface_bgl),
				],
				immediate_size: 0,
			});
		let water_layout = self
			.device
			.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[water] pipeline layout"),
				bind_group_layouts: &[Some(&self.water_bgl0), Some(&self.water_bgl1)],
				immediate_size: 0,
			});
		let terrain_layout = self
			.device
			.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[terrain] pipeline layout"),
				bind_group_layouts: &[
					Some(&self.terrain_globals_bgl),
					Some(&self.terrain_params_bgl),
				],
				immediate_size: 0,
			});
		let particle_layout = self
			.device
			.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[particles] render pipeline layout"),
				bind_group_layouts: &[Some(&self.particle_render_bgl)],
				immediate_size: 0,
			});

		let msaa_state = wgpu::MultisampleState {
			count: msaa,
			..Default::default()
		};
		let depth_state = Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth32Float,
			depth_write_enabled: Some(tier == RenderTier::LowGles),
			depth_compare: Some(if tier == RenderTier::LowGles {
				wgpu::CompareFunction::Less
			} else {
				wgpu::CompareFunction::LessEqual
			}),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		});

		self.opaque_pipeline =
			self.device
				.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
					label: Some("3d opaque pipeline"),
					layout: Some(&main_layout),
					vertex: wgpu::VertexState {
						module: &self.msaa_main_shader,
						entry_point: Some("vs_main"),
						buffers: &vertex_buffers,
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					},
					fragment: Some(wgpu::FragmentState {
						module: &self.msaa_main_shader,
						entry_point: Some("fs_main"),
						targets: &[Some(wgpu::ColorTargetState {
							format: hdr,
							blend: Some(wgpu::BlendState::ALPHA_BLENDING),
							write_mask: wgpu::ColorWrites::ALL,
						})],
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					}),
					primitive: wgpu::PrimitiveState {
						topology: wgpu::PrimitiveTopology::TriangleList,
						front_face: wgpu::FrontFace::Ccw,
						cull_mode: Some(wgpu::Face::Back),
						..Default::default()
					},
					depth_stencil: depth_state.clone(),
					multisample: msaa_state,
					cache,
					multiview_mask: None,
				});

		self.sky_pipeline = self
			.device
			.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("3d sky pipeline"),
				layout: Some(&main_layout),
				vertex: wgpu::VertexState {
					module: &self.msaa_main_shader,
					entry_point: Some("vs_main"),
					buffers: &vertex_buffers,
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				},
				fragment: Some(wgpu::FragmentState {
					module: &self.msaa_main_shader,
					entry_point: Some("fs_main"),
					targets: &[Some(wgpu::ColorTargetState {
						format: hdr,
						blend: None,
						write_mask: wgpu::ColorWrites::ALL,
					})],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				}),
				primitive: wgpu::PrimitiveState {
					topology: wgpu::PrimitiveTopology::TriangleList,
					front_face: wgpu::FrontFace::Ccw,
					cull_mode: None,
					..Default::default()
				},
				depth_stencil: Some(wgpu::DepthStencilState {
					format: wgpu::TextureFormat::Depth32Float,
					depth_write_enabled: Some(false),
					depth_compare: Some(wgpu::CompareFunction::Always),
					stencil: wgpu::StencilState::default(),
					bias: wgpu::DepthBiasState::default(),
				}),
				multisample: msaa_state,
				cache,
				multiview_mask: None,
			});

		self.zprepass_pipeline =
			self.device
				.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
					label: Some("3d z-prepass pipeline"),
					layout: Some(&zprepass_layout),
					vertex: wgpu::VertexState {
						module: &self.msaa_main_shader,
						entry_point: Some("vs_depth"),
						buffers: &pos_vertex_buffers,
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					},
					fragment: None,
					primitive: wgpu::PrimitiveState {
						topology: wgpu::PrimitiveTopology::TriangleList,
						front_face: wgpu::FrontFace::Ccw,
						cull_mode: Some(wgpu::Face::Back),
						..Default::default()
					},
					depth_stencil: Some(wgpu::DepthStencilState {
						format: wgpu::TextureFormat::Depth32Float,
						depth_write_enabled: Some(true),
						depth_compare: Some(wgpu::CompareFunction::Less),
						stencil: wgpu::StencilState::default(),
						bias: wgpu::DepthBiasState::default(),
					}),
					multisample: msaa_state,
					cache,
					multiview_mask: None,
				});

		self.transparent_pipeline =
			self.device
				.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
					label: Some("3d transparent pipeline"),
					layout: Some(&main_layout),
					vertex: wgpu::VertexState {
						module: &self.msaa_main_shader,
						entry_point: Some("vs_main"),
						buffers: &vertex_buffers,
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					},
					fragment: Some(wgpu::FragmentState {
						module: &self.msaa_main_shader,
						entry_point: Some("fs_main"),
						targets: &[Some(wgpu::ColorTargetState {
							format: hdr,
							blend: Some(wgpu::BlendState::ALPHA_BLENDING),
							write_mask: wgpu::ColorWrites::ALL,
						})],
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					}),
					primitive: wgpu::PrimitiveState {
						topology: wgpu::PrimitiveTopology::TriangleList,
						front_face: wgpu::FrontFace::Ccw,
						cull_mode: None,
						..Default::default()
					},
					depth_stencil: Some(wgpu::DepthStencilState {
						format: wgpu::TextureFormat::Depth32Float,
						depth_write_enabled: Some(false),
						depth_compare: Some(wgpu::CompareFunction::LessEqual),
						stencil: wgpu::StencilState::default(),
						bias: wgpu::DepthBiasState::default(),
					}),
					multisample: msaa_state,
					cache,
					multiview_mask: None,
				});

		self.surface_pipeline =
			self.device
				.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
					label: Some("[surface] pipeline"),
					layout: Some(&surface_layout),
					vertex: wgpu::VertexState {
						module: &self.msaa_surface_shader,
						entry_point: Some("vs_surface"),
						buffers: &vertex_buffers,
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					},
					fragment: Some(wgpu::FragmentState {
						module: &self.msaa_surface_shader,
						entry_point: Some("fs_surface"),
						targets: &[Some(wgpu::ColorTargetState {
							format: hdr,
							blend: Some(wgpu::BlendState::ALPHA_BLENDING),
							write_mask: wgpu::ColorWrites::ALL,
						})],
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					}),
					primitive: wgpu::PrimitiveState {
						topology: wgpu::PrimitiveTopology::TriangleList,
						cull_mode: Some(wgpu::Face::Back),
						..Default::default()
					},
					depth_stencil: Some(wgpu::DepthStencilState {
						format: wgpu::TextureFormat::Depth32Float,
						depth_write_enabled: Some(false),
						depth_compare: Some(wgpu::CompareFunction::LessEqual),
						stencil: wgpu::StencilState::default(),
						bias: wgpu::DepthBiasState::default(),
					}),
					multisample: msaa_state,
					cache,
					multiview_mask: None,
				});

		self.water_pipeline = self
			.device
			.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("[water] pipeline"),
				layout: Some(&water_layout),
				vertex: wgpu::VertexState {
					module: &self.msaa_water_shader,
					entry_point: Some("vs_main"),
					buffers: &vertex_buffers,
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				},
				fragment: Some(wgpu::FragmentState {
					module: &self.msaa_water_shader,
					entry_point: Some("fs_main"),
					targets: &[Some(wgpu::ColorTargetState {
						format: hdr,
						blend: Some(wgpu::BlendState::ALPHA_BLENDING),
						write_mask: wgpu::ColorWrites::ALL,
					})],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				}),
				primitive: wgpu::PrimitiveState {
					topology: wgpu::PrimitiveTopology::TriangleList,
					cull_mode: None,
					..Default::default()
				},
				depth_stencil: Some(wgpu::DepthStencilState {
					format: wgpu::TextureFormat::Depth32Float,
					depth_write_enabled: Some(false),
					depth_compare: Some(wgpu::CompareFunction::LessEqual),
					stencil: wgpu::StencilState::default(),
					bias: wgpu::DepthBiasState::default(),
				}),
				multisample: msaa_state,
				cache,
				multiview_mask: None,
			});

		self.terrain_pipeline =
			self.device
				.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
					label: Some("[terrain] pipeline"),
					layout: Some(&terrain_layout),
					vertex: wgpu::VertexState {
						module: &self.msaa_terrain_shader,
						entry_point: Some("vs_main"),
						buffers: &vertex_buffers,
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					},
					fragment: Some(wgpu::FragmentState {
						module: &self.msaa_terrain_shader,
						entry_point: Some("fs_main"),
						targets: &[Some(wgpu::ColorTargetState {
							format: hdr,
							blend: None,
							write_mask: wgpu::ColorWrites::ALL,
						})],
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					}),
					primitive: wgpu::PrimitiveState {
						topology: wgpu::PrimitiveTopology::TriangleList,
						cull_mode: Some(wgpu::Face::Back),
						..Default::default()
					},
					depth_stencil: Some(wgpu::DepthStencilState {
						format: wgpu::TextureFormat::Depth32Float,
						depth_write_enabled: Some(true),
						depth_compare: Some(wgpu::CompareFunction::LessEqual),
						stencil: wgpu::StencilState::default(),
						bias: wgpu::DepthBiasState::default(),
					}),
					multisample: wgpu::MultisampleState {
						count: msaa,
						mask: !0,
						alpha_to_coverage_enabled: false,
					},
					cache,
					multiview_mask: None,
				});

		self.particle_render_pipeline =
			self.device
				.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
					label: Some("[particles] render pipeline"),
					layout: Some(&particle_layout),
					vertex: wgpu::VertexState {
						module: &self.msaa_particle_render_shader,
						entry_point: Some("vs_main"),
						buffers: &[],
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					},
					fragment: Some(wgpu::FragmentState {
						module: &self.msaa_particle_render_shader,
						entry_point: Some("fs_main"),
						targets: &[Some(wgpu::ColorTargetState {
							format: hdr,
							blend: Some(wgpu::BlendState::ALPHA_BLENDING),
							write_mask: wgpu::ColorWrites::ALL,
						})],
						compilation_options: wgpu::PipelineCompilationOptions::default(),
					}),
					primitive: wgpu::PrimitiveState {
						topology: wgpu::PrimitiveTopology::TriangleList,
						cull_mode: None,
						..Default::default()
					},
					depth_stencil: Some(wgpu::DepthStencilState {
						format: wgpu::TextureFormat::Depth32Float,
						depth_write_enabled: Some(false),
						depth_compare: Some(wgpu::CompareFunction::LessEqual),
						stencil: wgpu::StencilState::default(),
						bias: wgpu::DepthBiasState::default(),
					}),
					multisample: msaa_state,
					cache,
					multiview_mask: None,
				});
	}

	// ── render ─────────────────────────────────────────────────────────────
	pub(crate) fn preset_ord(p: QualityPreset) -> u8 {
		match p {
			QualityPreset::Minimum => 0,
			QualityPreset::Low => 1,
			QualityPreset::Medium => 2,
			QualityPreset::High => 3,
			QualityPreset::Ultra => 4,
		}
	}
	/// step quality preset down by one level (respects `min`).
	pub(crate) fn preset_step_down(current: QualityPreset, min: QualityPreset) -> QualityPreset {
		let next = match current {
			QualityPreset::Ultra => QualityPreset::High,
			QualityPreset::High => QualityPreset::Medium,
			QualityPreset::Medium => QualityPreset::Low,
			QualityPreset::Low => QualityPreset::Minimum,
			QualityPreset::Minimum => QualityPreset::Minimum,
		};
		if Self::preset_ord(next) < Self::preset_ord(min) {
			min
		} else {
			next
		}
	}
	/// step quality preset up by one level (respects `max`).
	pub(crate) fn preset_step_up(current: QualityPreset, max: QualityPreset) -> QualityPreset {
		let next = match current {
			QualityPreset::Minimum => QualityPreset::Low,
			QualityPreset::Low => QualityPreset::Medium,
			QualityPreset::Medium => QualityPreset::High,
			QualityPreset::High => QualityPreset::Ultra,
			QualityPreset::Ultra => QualityPreset::Ultra,
		};
		if Self::preset_ord(next) > Self::preset_ord(max) {
			max
		} else {
			next
		}
	}
}
