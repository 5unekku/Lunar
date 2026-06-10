//! `RenderEngine3d` — construction, adapter/device init, canvas + pipeline-cache plumbing.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

impl RenderEngine3d {
	#[cfg(not(target_arch = "wasm32"))]
	pub fn from_surface(
		instance: &wgpu::Instance,
		surface: wgpu::Surface<'static>,
		config: &RenderConfig3d,
	) -> Self {
		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::HighPerformance,
			force_fallback_adapter: false,
			compatible_surface: Some(&surface),
		}))
		.expect("no wgpu adapter found");

		// request optional features based on adapter support
		let has_r11 = adapter
			.features()
			.contains(wgpu::Features::RG11B10UFLOAT_RENDERABLE);
		// indirect first instance needed for non-zero first_instance in indirect draws
		let has_indirect = adapter
			.features()
			.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);
		// PIPELINE_CACHE (Vulkan/DX12) lets the driver persist compiled PSOs across runs —
		// without it the create_pipeline_cache calls below are inert, so request it up front.
		let has_pipeline_cache = adapter
			.features()
			.contains(wgpu::Features::PIPELINE_CACHE);
		let mut required_features = if has_r11 {
			wgpu::Features::RG11B10UFLOAT_RENDERABLE
		} else {
			wgpu::Features::empty()
		};
		if has_indirect {
			required_features |= wgpu::Features::INDIRECT_FIRST_INSTANCE;
		}
		if has_pipeline_cache {
			required_features |= wgpu::Features::PIPELINE_CACHE;
		}
		// SPIR-V passthrough skips wgpu's runtime naga re-validation of our precompiled .spv.
		// only meaningful on Vulkan (where SPIR-V is the native format); DX12 wants DXIL/HLSL.
		let has_passthrough = adapter
			.features()
			.contains(wgpu::Features::PASSTHROUGH_SHADERS)
			&& adapter.get_info().backend == wgpu::Backend::Vulkan;
		if has_passthrough {
			required_features |= wgpu::Features::PASSTHROUGH_SHADERS;
		}
		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("lunar-render-3d device"),
			required_features,
			required_limits: wgpu::Limits {
				max_bind_groups: 8,
				..wgpu::Limits::default()
			},
			memory_hints: wgpu::MemoryHints::Performance,
			trace: wgpu::Trace::default(),
			experimental_features: wgpu::ExperimentalFeatures::disabled(),
		}))
		.expect("failed to create wgpu device");

		let hdr_format = if has_r11 {
			wgpu::TextureFormat::Rg11b10Ufloat
		} else {
			wgpu::TextureFormat::Rgba16Float
		};
		log::info!("HDR format: {hdr_format:?}, indirect: {has_indirect}");
		Self::init_with_adapter(
			&adapter,
			device,
			queue,
			surface,
			config,
			hdr_format,
			has_indirect,
		)
	}

	/// create the 3d render engine from a surface (wasm async path).
	///
	/// call from a `#[wasm_bindgen(start)]` async entry point after obtaining a
	/// wgpu surface from a `<canvas>` element. feature negotiation mirrors the
	/// native path; `RG11B10UFLOAT_RENDERABLE` and `INDIRECT_FIRST_INSTANCE` are
	/// unavailable on WebGPU so the engine will always run at `RenderTier::Mid`.
	#[cfg(target_arch = "wasm32")]
	pub async fn from_surface(
		instance: &wgpu::Instance,
		surface: wgpu::Surface<'static>,
		config: &RenderConfig3d,
	) -> Self {
		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::HighPerformance,
				force_fallback_adapter: false,
				compatible_surface: Some(&surface),
			})
			.await
			.expect(
				"no WebGPU adapter — Chrome 113+, Firefox with dom.webgpu.enabled, or Safari 17+",
			);
		// WebGPU does not expose RG11B10UFLOAT_RENDERABLE or INDIRECT_FIRST_INSTANCE
		let (device, queue) = adapter
			.request_device(&wgpu::DeviceDescriptor {
				label: Some("lunar-render-3d device"),
				required_features: wgpu::Features::empty(),
				required_limits: wgpu::Limits {
					max_bind_groups: 8,
					..wgpu::Limits::downlevel_webgl2_defaults()
				},
				memory_hints: wgpu::MemoryHints::Performance,
				trace: wgpu::Trace::default(),
				experimental_features: wgpu::ExperimentalFeatures::disabled(),
			})
			.await
			.expect("failed to create wgpu device");
		log::info!("HDR format: Rgba16Float (WebGPU), indirect: false");
		Self::init_with_adapter(
			&adapter,
			device,
			queue,
			surface,
			config,
			wgpu::TextureFormat::Rgba16Float,
			false,
		)
	}
	/// create a WebGPU surface from a canvas element (wasm only).
	#[cfg(target_arch = "wasm32")]
	pub fn create_canvas_surface(
		instance: &wgpu::Instance,
		canvas: &web_sys::HtmlCanvasElement,
	) -> Result<wgpu::Surface<'static>, String> {
		instance
			.create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
			.map_err(|e| format!("failed to create surface: {e:?}"))
	}
	/// find a canvas element by id (wasm only).
	#[cfg(target_arch = "wasm32")]
	pub fn find_canvas(id: &str) -> Result<web_sys::HtmlCanvasElement, String> {
		use wasm_bindgen::JsCast;
		web_sys::window()
			.and_then(|w| w.document())
			.and_then(|d| d.get_element_by_id(id))
			.and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
			.ok_or_else(|| format!("canvas #{id} not found"))
	}
	pub(crate) fn init_with_adapter(
		adapter: &wgpu::Adapter,
		device: wgpu::Device,
		queue: wgpu::Queue,
		surface: wgpu::Surface<'static>,
		config: &RenderConfig3d,
		hdr_format: wgpu::TextureFormat,
		has_indirect: bool,
	) -> Self {
		let render_tier = RenderTier::from_adapter(adapter);
		log::info!("render tier: {render_tier:?}");

		// shaders take the SPIR-V passthrough path only on Vulkan with PASSTHROUGH_SHADERS enabled
		// (requested in from_surface). everywhere else this stays false → the validating path.
		let shader_passthrough = device
			.features()
			.contains(wgpu::Features::PASSTHROUGH_SHADERS)
			&& adapter.get_info().backend == wgpu::Backend::Vulkan;
		log::info!("shader passthrough: {shader_passthrough}");

		let caps = surface.get_capabilities(adapter);
		// prefer non-sRGB linear format — game colors are sRGB-defined and used directly;
		// applying hardware gamma on top would wash them out on native vs browser
		let format = caps
			.formats
			.iter()
			.find(|&&f| {
				f == wgpu::TextureFormat::Bgra8Unorm || f == wgpu::TextureFormat::Rgba8Unorm
			})
			.copied()
			.or_else(|| caps.formats.first().copied())
			.unwrap_or(wgpu::TextureFormat::Bgra8Unorm);

		let surface_config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width: config.width,
			height: config.height,
			present_mode: if config.vsync {
				wgpu::PresentMode::AutoVsync
			} else {
				wgpu::PresentMode::AutoNoVsync
			},
			alpha_mode: caps.alpha_modes.first().copied().unwrap_or_default(),
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		};
		surface.configure(&device, &surface_config);

		// derive quality settings early so msaa_samples and other tier-specific values come from one place
		let quality_early = QualitySettings::from_tier(render_tier);
		let msaa_samples = quality_early.msaa_samples;
		let depth_view = Self::make_depth_view(&device, config.width, config.height, msaa_samples);
		let msaa_color_view = Self::make_msaa_color_view(
			&device,
			config.width,
			config.height,
			hdr_format,
			msaa_samples,
		);

		// ── bind group layouts ─────────────────────────────────────────────

		let globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[globals] bgl"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
				},
				count: None,
			}],
		});

		// group 1: material — storage array indexed by instance_id, set once per pass
		let material_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[material] bgl"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Storage { read_only: true },
					has_dynamic_offset: false,
					min_binding_size: None,
				},
				count: None,
			}],
		});

		// group 2: per-mesh — dynamic offset, one slot per draw call (model matrix)
		let mesh_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[mesh] bgl"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Storage { read_only: true },
					has_dynamic_offset: false,
					min_binding_size: None,
				},
				count: None,
			}],
		});

		// ── globals buffer ─────────────────────────────────────────────────

		let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[globals] view-proj+time"),
			size: GLOBALS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[globals] bg"),
			layout: &globals_bgl,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: globals_buf.as_entire_binding(),
			}],
		});

		// ── material storage buffer (group 1) ─────────────────────────────

		let entity_capacity = INITIAL_ENTITY_CAPACITY;
		let material_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[material] storage buffer"),
			size: (entity_capacity * MATERIAL_UNIFORMS_SIZE as usize) as u64,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let material_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[material] bg"),
			layout: &material_bgl,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: material_buf.as_entire_binding(),
			}],
		});
		let material_staging = vec![0u8; entity_capacity * MATERIAL_UNIFORMS_SIZE as usize];

		// ── mesh uniform buffer (group 2) ─────────────────────────────────

		let entity_buf = Self::make_entity_buf(&device, entity_capacity);
		let entity_bg = Self::make_entity_bg(&device, &mesh_bgl, &entity_buf);
		let uniform_staging = vec![0u8; entity_capacity * UNIFORM_STRIDE as usize];

		// ── lights buffer (group 3) ───────────────────────────────────────

		let lights_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[lights] bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(LIGHTS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Depth,
						view_dimension: wgpu::TextureViewDimension::D2Array,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
					count: None,
				},
				// binding 3: 4 point lights × 6 faces = 24-layer depth array for cube shadows
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Depth,
						view_dimension: wgpu::TextureViewDimension::D2Array,
						multisampled: false,
					},
					count: None,
				},
			],
		});

		let lights_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[lights] uniform buffer"),
			size: LIGHTS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		// 3-layer depth array — one layer per cascade
		let shadow_map = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[shadow] cascade depth array"),
			size: wgpu::Extent3d {
				width: SHADOW_MAP_SIZE,
				height: SHADOW_MAP_SIZE,
				depth_or_array_layers: NUM_CASCADES,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Depth32Float,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		// full-array view for shader sampling
		let shadow_map_view = shadow_map.create_view(&wgpu::TextureViewDescriptor {
			dimension: Some(wgpu::TextureViewDimension::D2Array),
			..Default::default()
		});
		// per-cascade single-layer views for render attachments
		let shadow_cascade_views = std::array::from_fn(|i| {
			shadow_map.create_view(&wgpu::TextureViewDescriptor {
				dimension: Some(wgpu::TextureViewDimension::D2),
				base_array_layer: i as u32,
				array_layer_count: Some(1),
				..Default::default()
			})
		});

		let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[shadow] comparison sampler"),
			compare: Some(wgpu::CompareFunction::LessEqual),
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});

		// point light shadow maps: 4 lights × 6 faces = 24 layers
		let point_shadow_map_size = quality_early.point_shadow_res;
		let point_shadow_layers = (MAX_POINT_SHADOW_LIGHTS * 6) as u32;
		let point_shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[point shadow] depth array"),
			size: wgpu::Extent3d {
				width: point_shadow_map_size,
				height: point_shadow_map_size,
				depth_or_array_layers: point_shadow_layers,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Depth32Float,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let point_shadow_array_view = point_shadow_tex.create_view(&wgpu::TextureViewDescriptor {
			dimension: Some(wgpu::TextureViewDimension::D2Array),
			..Default::default()
		});
		let point_shadow_face_views: Vec<wgpu::TextureView> = (0..point_shadow_layers)
			.map(|layer| {
				point_shadow_tex.create_view(&wgpu::TextureViewDescriptor {
					dimension: Some(wgpu::TextureViewDimension::D2),
					base_array_layer: layer,
					array_layer_count: Some(1),
					..Default::default()
				})
			})
			.collect();

		let lights_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[lights] bg"),
			layout: &lights_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: lights_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&shadow_map_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&shadow_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&point_shadow_array_view),
				},
			],
		});

		// ── shadow globals (group 0 of shadow pipeline) ───────────────────

		let shadow_globals_bgl =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[shadow globals] bgl"),
				entries: &[wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(SHADOW_GLOBALS_SIZE),
					},
					count: None,
				}],
			});

		// 3 cascade slots, 256-byte aligned (one per cascade, selected via dynamic offset)
		let shadow_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[shadow globals] cascade VPs"),
			size: NUM_CASCADES as u64 * UNIFORM_STRIDE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let shadow_globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[shadow globals] bg"),
			layout: &shadow_globals_bgl,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: shadow_globals_buf.as_entire_binding(),
			}],
		});

		// ── point shadow globals ────────────────────────────────────────────
		// 24 slots × UNIFORM_STRIDE, one per (light × face) combination.
		// uses dynamic offset so one bind group covers all 24 slots.
		let point_shadow_globals_bgl =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[point shadow globals] bgl"),
				entries: &[wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: true,
						min_binding_size: wgpu::BufferSize::new(POINT_SHADOW_GLOBALS_SIZE),
					},
					count: None,
				}],
			});
		let point_shadow_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[point shadow globals] buf"),
			size: (MAX_POINT_SHADOW_LIGHTS * 6) as u64 * UNIFORM_STRIDE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let point_shadow_globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[point shadow globals] bg"),
			layout: &point_shadow_globals_bgl,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
					buffer: &point_shadow_globals_buf,
					offset: 0,
					size: wgpu::BufferSize::new(POINT_SHADOW_GLOBALS_SIZE),
				}),
			}],
		});

		// ── pipeline cache (Vulkan/DX12 only) ─────────────────────────────
		// load compiled shader binaries from previous run to skip recompilation.
		// keyed per adapter (vendor/device/backend) so a multi-GPU box doesn't clobber.
		#[cfg(not(target_arch = "wasm32"))]
		let pipeline_cache_path = Self::pipeline_cache_path(adapter);
		#[cfg(not(target_arch = "wasm32"))]
		let pipeline_cache = Self::load_pipeline_cache(&device, pipeline_cache_path.as_deref());
		// PipelineCache is Vulkan/DX12 only — WebGPU has no equivalent
		#[cfg(not(target_arch = "wasm32"))]
		let pipeline_cache_ref: Option<&wgpu::PipelineCache> = pipeline_cache.as_ref();
		#[cfg(target_arch = "wasm32")]
		let pipeline_cache_ref: Option<&wgpu::PipelineCache> = None;

		// ── pipelines ──────────────────────────────────────────────────────

		let shader = make_shader!(device, shader_passthrough, "3d PBR shader", SHADER_SRC, "shader.spv");

		let shadow_shader = make_shader!(device, shader_passthrough, "3d shadow shader", SHADOW_SHADER_SRC, "shadow.spv");

		// group 4: irradiance tex (b0) + dir tex (b1) + sampler (b2), bound per draw group
		let lightmap_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[lightmap] bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});
		let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[lightmap] sampler"),
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::MipmapFilterMode::Linear,
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			..Default::default()
		});
		// irradiance fallback: 1×1 white (used for non-lightmapped entities)
		let lightmap_fallback_tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[lightmap] fallback irr 1x1"),
			size: wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		queue.write_texture(
			lightmap_fallback_tex.as_image_copy(),
			&[255u8, 255, 255, 255],
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(4),
				rows_per_image: Some(1),
			},
			wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
		);
		let lightmap_fallback_view = lightmap_fallback_tex.create_view(&Default::default());
		// direction fallback: 1×1 neutral direction (0,0,1) packed as (128,128,255)
		let dir_lm_fallback_tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[lightmap] fallback dir 1x1"),
			size: wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		queue.write_texture(
			dir_lm_fallback_tex.as_image_copy(),
			&[128u8, 128, 255, 255],
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(4),
				rows_per_image: Some(1),
			},
			wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
		);
		let dir_lm_fallback_view = dir_lm_fallback_tex.create_view(&Default::default());
		let lightmap_fallback_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[lightmap] fallback bg"),
			layout: &lightmap_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&lightmap_fallback_view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&dir_lm_fallback_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
				},
			],
		});

		// cluster render BGL must exist before pipeline_layout; full cluster setup done later.
		let cluster_bgl_render_early =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[cluster] render bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Uniform,
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(CLUSTER_PARAMS_SIZE),
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Storage { read_only: true },
							has_dynamic_offset: false,
							min_binding_size: None,
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 2,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Storage { read_only: true },
							has_dynamic_offset: false,
							min_binding_size: None,
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 3,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Storage { read_only: true },
							has_dynamic_offset: false,
							min_binding_size: None,
						},
						count: None,
					},
				],
			});

		let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("3d pipeline layout"),
			bind_group_layouts: &[
				Some(&globals_bgl),
				Some(&material_bgl),
				Some(&mesh_bgl),
				Some(&lights_bgl),
				Some(&lightmap_bgl),
				Some(&cluster_bgl_render_early),
			],
			immediate_size: 0,
		});

		let zprepass_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("3d z-prepass pipeline layout"),
				bind_group_layouts: &[Some(&globals_bgl), Some(&material_bgl), Some(&mesh_bgl)],
				immediate_size: 0,
			});

		let shadow_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("3d shadow pipeline layout"),
				bind_group_layouts: &[Some(&shadow_globals_bgl), Some(&mesh_bgl)],
				immediate_size: 0,
			});

		let vertex_buffers = &[wgpu::VertexBufferLayout {
			array_stride: VERTEX_STRIDE,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &[
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
			],
		}];
		// position-only layout for shadow and z-prepass pipelines (12 bytes/vertex vs 32)
		let pos_vertex_buffers = &[wgpu::VertexBufferLayout {
			array_stride: POS_VERTEX_STRIDE,
			step_mode: wgpu::VertexStepMode::Vertex,
			attributes: &[wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Float32x3,
				offset: 0,
				shader_location: 0,
			}],
		}];

		let opaque_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("3d opaque pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				buffers: vertex_buffers,
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
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
			depth_stencil: Some(wgpu::DepthStencilState {
				format: wgpu::TextureFormat::Depth32Float,
				// with z-prepass (mid/high) depth is already populated — use LessEqual
				depth_write_enabled: Some(render_tier == RenderTier::LowGles),
				depth_compare: Some(if render_tier == RenderTier::LowGles {
					wgpu::CompareFunction::Less
				} else {
					wgpu::CompareFunction::LessEqual
				}),
				stencil: wgpu::StencilState::default(),
				bias: wgpu::DepthBiasState::default(),
			}),
			multisample: wgpu::MultisampleState {
				count: msaa_samples,
				..Default::default()
			},
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("3d sky pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				buffers: vertex_buffers,
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
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
			multisample: wgpu::MultisampleState {
				count: msaa_samples,
				..Default::default()
			},
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// Z-prepass: depth-only, no fragment shader, uses same vertex layout as opaque.
		// on mid/high tier this runs before the opaque pass to eliminate overdraw.
		let zprepass_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("3d z-prepass pipeline"),
			layout: Some(&zprepass_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_depth"),
				buffers: pos_vertex_buffers,
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
			multisample: wgpu::MultisampleState {
				count: msaa_samples,
				..Default::default()
			},
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("3d shadow pipeline"),
			layout: Some(&shadow_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shadow_shader,
				entry_point: Some("vs_shadow"),
				buffers: pos_vertex_buffers,
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: None,
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				front_face: wgpu::FrontFace::Ccw,
				// front-face culling reduces peter-panning shadow acne
				cull_mode: Some(wgpu::Face::Front),
				..Default::default()
			},
			depth_stencil: Some(wgpu::DepthStencilState {
				format: wgpu::TextureFormat::Depth32Float,
				depth_write_enabled: Some(true),
				depth_compare: Some(wgpu::CompareFunction::Less),
				stencil: wgpu::StencilState::default(),
				bias: wgpu::DepthBiasState::default(),
			}),
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── clustered forward lighting resources ─────────────────────────
		// cluster_bgl_render was already created above (needed by pipeline_layout)
		let cluster_bgl_render = cluster_bgl_render_early;
		let light_entry_size: u64 = 48; // matches PointLightGpu in shader (48 bytes)
		let light_list_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[cluster] light list"),
			size: MAX_CLUSTERED_LIGHTS as u64 * light_entry_size,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let cluster_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[cluster] params"),
			size: CLUSTER_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let cluster_counts_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[cluster] counts"),
			size: (NUM_CLUSTERS * 4) as u64,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let cluster_indices_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[cluster] light indices"),
			size: (NUM_CLUSTERS * MAX_LIGHTS_PER_CLUSTER * 4) as u64,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		// compute BGL: all bindings in COMPUTE, counts/indices are read_write
		let cluster_bgl_compute =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[cluster] compute bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::COMPUTE,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Uniform,
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(CLUSTER_PARAMS_SIZE),
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::COMPUTE,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Storage { read_only: true },
							has_dynamic_offset: false,
							min_binding_size: None,
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 2,
						visibility: wgpu::ShaderStages::COMPUTE,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Storage { read_only: false },
							has_dynamic_offset: false,
							min_binding_size: None,
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 3,
						visibility: wgpu::ShaderStages::COMPUTE,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Storage { read_only: false },
							has_dynamic_offset: false,
							min_binding_size: None,
						},
						count: None,
					},
				],
			});
		// cluster_bgl_render already bound above via cluster_bgl_render_early alias
		let cluster_bg_compute = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[cluster] compute bg"),
			layout: &cluster_bgl_compute,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: cluster_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: light_list_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: cluster_counts_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: cluster_indices_buf.as_entire_binding(),
				},
			],
		});
		let cluster_bg_render = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[cluster] render bg"),
			layout: &cluster_bgl_render,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: cluster_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: light_list_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: cluster_counts_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: cluster_indices_buf.as_entire_binding(),
				},
			],
		});

		// ── surface shader pipeline (group 2 = stage params + 4 textures + sampler) ──
		let surface_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[surface] stage bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: true,
						min_binding_size: wgpu::BufferSize::new(128),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 4,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 5,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});
		let surface_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[surface] sampler"),
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::MipmapFilterMode::Linear,
			address_mode_u: wgpu::AddressMode::Repeat,
			address_mode_v: wgpu::AddressMode::Repeat,
			..Default::default()
		});
		let surface_fallback_tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[surface] fallback 1x1"),
			size: wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8UnormSrgb,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		queue.write_texture(
			surface_fallback_tex.as_image_copy(),
			&[255u8, 255, 255, 255],
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(4),
				rows_per_image: Some(1),
			},
			wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
		);
		let surface_fallback_view = surface_fallback_tex.create_view(&Default::default());
		let surface_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[surface] stage params"),
			size: 512 * UNIFORM_STRIDE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let surface_shader_module = make_shader!(device, shader_passthrough, "[surface] shader", SURFACE_SHADER_SRC, "surface.spv");
		let surface_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[surface] pipeline layout"),
				bind_group_layouts: &[Some(&globals_bgl), Some(&mesh_bgl), Some(&surface_bgl)],
				immediate_size: 0,
			});
		let surface_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[surface] pipeline"),
			layout: Some(&surface_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &surface_shader_module,
				entry_point: Some("vs_surface"),
				buffers: vertex_buffers,
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &surface_shader_module,
				entry_point: Some("fs_surface"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
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
			depth_stencil: Some(wgpu::DepthStencilState {
				format: wgpu::TextureFormat::Depth32Float,
				depth_write_enabled: Some(true),
				depth_compare: Some(wgpu::CompareFunction::LessEqual),
				stencil: wgpu::StencilState::default(),
				bias: wgpu::DepthBiasState::default(),
			}),
			multisample: wgpu::MultisampleState {
				count: msaa_samples,
				..Default::default()
			},
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// point shadow pipeline: writes linear depth, uses point_shadow.wgsl
		let point_shadow_shader = make_shader!(device, shader_passthrough, "[point shadow] shader", POINT_SHADOW_SHADER_SRC, "point_shadow.spv");
		let point_shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("[point shadow] pipeline layout"),
			bind_group_layouts: &[Some(&point_shadow_globals_bgl), Some(&mesh_bgl)],
			immediate_size: 0,
		});
		let point_shadow_pipeline =
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("[point shadow] pipeline"),
				layout: Some(&point_shadow_layout),
				vertex: wgpu::VertexState {
					module: &point_shadow_shader,
					entry_point: Some("vs_point_shadow"),
					buffers: pos_vertex_buffers,
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				},
				fragment: Some(wgpu::FragmentState {
					module: &point_shadow_shader,
					entry_point: Some("fs_point_shadow"),
					targets: &[],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				}),
				primitive: wgpu::PrimitiveState {
					topology: wgpu::PrimitiveTopology::TriangleList,
					front_face: wgpu::FrontFace::Ccw,
					cull_mode: Some(wgpu::Face::Front),
					..Default::default()
				},
				depth_stencil: Some(wgpu::DepthStencilState {
					format: wgpu::TextureFormat::Depth32Float,
					depth_write_enabled: Some(true),
					depth_compare: Some(wgpu::CompareFunction::Less),
					stencil: wgpu::StencilState::default(),
					bias: wgpu::DepthBiasState::default(),
				}),
				multisample: wgpu::MultisampleState::default(),
				cache: pipeline_cache_ref,
				multiview_mask: None,
			});

		// cluster light assignment compute pipeline
		let cluster_shader = make_shader!(device, shader_passthrough, "[cluster] shader", CLUSTER_SHADER_SRC, "cluster.spv");
		let cluster_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("[cluster] pipeline layout"),
			bind_group_layouts: &[Some(&cluster_bgl_compute)],
			immediate_size: 0,
		});
		let cluster_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
			label: Some("[cluster] pipeline"),
			layout: Some(&cluster_layout),
			module: &cluster_shader,
			entry_point: Some("cs_cluster_assign"),
			compilation_options: wgpu::PipelineCompilationOptions::default(),
			cache: pipeline_cache_ref,
		});

		// transparent pipeline: same shader as opaque but no depth write, no backface cull
		let transparent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("3d transparent pipeline"),
			layout: Some(&pipeline_layout),
			vertex: wgpu::VertexState {
				module: &shader,
				entry_point: Some("vs_main"),
				buffers: vertex_buffers,
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
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
			multisample: wgpu::MultisampleState {
				count: msaa_samples,
				..Default::default()
			},
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── HDR texture ────────────────────────────────────────────────────
		// color pass renders here; MSAA (if enabled) resolves into this non-MSAA tex

		let quality = quality_early;
		let bloom_enabled = quality.bloom;
		let bloom_mip_count = quality.bloom_mips as usize;
		let fxaa_enabled = true; // pipeline always built; quality.fxaa gates it at runtime via dev_fxaa

		let (hdr_texture, hdr_view) =
			Self::make_hdr_texture(&device, config.width, config.height, hdr_format);

		// ── post sampler (linear clamp) ────────────────────────────────────
		let post_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[post] linear sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			mipmap_filter: wgpu::MipmapFilterMode::Linear,
			..Default::default()
		});

		// ── bloom ──────────────────────────────────────────────────────────

		let bloom_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[bloom] params buffer"),
			size: 2 * MAX_BLOOM_MIPS as u64 * UNIFORM_STRIDE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let bloom_downsample_bgl =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[bloom] bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Uniform,
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(BLOOM_PARAMS_SIZE),
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Texture {
							sample_type: wgpu::TextureSampleType::Float { filterable: true },
							view_dimension: wgpu::TextureViewDimension::D2,
							multisampled: false,
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 2,
						visibility: wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
						count: None,
					},
				],
			});

		let bloom_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[bloom] pipeline layout"),
				bind_group_layouts: &[Some(&bloom_downsample_bgl)],
				immediate_size: 0,
			});

		let bloom_shader = make_shader!(device, shader_passthrough, "[bloom] shader", BLOOM_SHADER_SRC, "bloom.spv");

		let bloom_downsample_pipeline =
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("[bloom] downsample pipeline"),
				layout: Some(&bloom_pipeline_layout),
				vertex: wgpu::VertexState {
					module: &bloom_shader,
					entry_point: Some("vs_main"),
					buffers: &[],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				},
				fragment: Some(wgpu::FragmentState {
					module: &bloom_shader,
					entry_point: Some("fs_downsample"),
					targets: &[Some(wgpu::ColorTargetState {
						format: hdr_format,
						blend: None,
						write_mask: wgpu::ColorWrites::ALL,
					})],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				}),
				primitive: wgpu::PrimitiveState::default(),
				depth_stencil: None,
				multisample: wgpu::MultisampleState::default(),
				cache: pipeline_cache_ref,
				multiview_mask: None,
			});

		let bloom_upsample_pipeline =
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("[bloom] upsample pipeline"),
				layout: Some(&bloom_pipeline_layout),
				vertex: wgpu::VertexState {
					module: &bloom_shader,
					entry_point: Some("vs_main"),
					buffers: &[],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				},
				fragment: Some(wgpu::FragmentState {
					module: &bloom_shader,
					entry_point: Some("fs_upsample"),
					targets: &[Some(wgpu::ColorTargetState {
						format: hdr_format,
						// additive blend: dst = dst + src
						blend: Some(wgpu::BlendState {
							color: wgpu::BlendComponent {
								src_factor: wgpu::BlendFactor::One,
								dst_factor: wgpu::BlendFactor::One,
								operation: wgpu::BlendOperation::Add,
							},
							alpha: wgpu::BlendComponent::REPLACE,
						}),
						write_mask: wgpu::ColorWrites::ALL,
					})],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				}),
				primitive: wgpu::PrimitiveState::default(),
				depth_stencil: None,
				multisample: wgpu::MultisampleState::default(),
				cache: pipeline_cache_ref,
				multiview_mask: None,
			});

		// build per-step bind groups and mip views for the bloom chain
		let (bloom_mip_views, bloom_mip_sizes, bloom_downsample_bgs, bloom_upsample_bgs) =
			Self::build_bloom_resources(
				&device,
				&hdr_texture,
				&bloom_params_buf,
				&bloom_downsample_bgl,
				&post_sampler,
				config.width,
				config.height,
				bloom_mip_count,
				hdr_format,
			);

		// ── composite ──────────────────────────────────────────────────────

		let composite_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[composite] params buffer"),
			size: COMPOSITE_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let composite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[composite] bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(COMPOSITE_PARAMS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 4,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				// binding 5: fog_tex (rgba16f volumetric scattering result)
				wgpu::BindGroupLayoutEntry {
					binding: 5,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				// binding 6: sampler
				wgpu::BindGroupLayoutEntry {
					binding: 6,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				// binding 7: contact_shadow_tex (R8Unorm; 1×1 zero fallback when disabled)
				wgpu::BindGroupLayoutEntry {
					binding: 7,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
			],
		});

		// 1×1 zero R8Unorm texture — fallback contact shadow when pass is disabled
		let contact_shadow_fallback_tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[contact shadow] fallback tex"),
			size: wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::R8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		{
			let fallback_data = [0u8; 4];
			queue.write_texture(
				contact_shadow_fallback_tex.as_image_copy(),
				&fallback_data[..1],
				wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(1),
					rows_per_image: Some(1),
				},
				wgpu::Extent3d {
					width: 1,
					height: 1,
					depth_or_array_layers: 1,
				},
			);
		}
		let contact_shadow_fallback_view =
			contact_shadow_fallback_tex.create_view(&wgpu::TextureViewDescriptor::default());

		let composite_shader = make_shader!(device, shader_passthrough, "[composite] shader", COMPOSITE_SHADER_SRC, "composite.spv");

		let composite_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[composite] pipeline layout"),
				bind_group_layouts: &[Some(&composite_bgl)],
				immediate_size: 0,
			});

		let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[composite] pipeline"),
			layout: Some(&composite_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &composite_shader,
				entry_point: Some("vs_main"),
				buffers: &[],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &composite_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── FXAA ───────────────────────────────────────────────────────────

		let fxaa_ldr_texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[fxaa] ldr texture"),
			size: wgpu::Extent3d {
				width: config.width,
				height: config.height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let fxaa_ldr_view = fxaa_ldr_texture.create_view(&wgpu::TextureViewDescriptor::default());

		let fxaa_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[fxaa] params buffer"),
			size: FXAA_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let fxaa_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[fxaa] bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(FXAA_PARAMS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});

		let fxaa_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[fxaa] bg"),
			layout: &fxaa_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: fxaa_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
			],
		});

		let fxaa_shader = make_shader!(device, shader_passthrough, "[fxaa] shader", FXAA_SHADER_SRC, "fxaa.spv");

		let fxaa_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("[fxaa] pipeline layout"),
			bind_group_layouts: &[Some(&fxaa_bgl)],
			immediate_size: 0,
		});

		let fxaa_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[fxaa] pipeline"),
			layout: Some(&fxaa_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &fxaa_shader,
				entry_point: Some("vs_main"),
				buffers: &[],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &fxaa_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── TAA (selective temporal AA, mid+ tier) ────────────────────────
		// initialized here, but gtao_depth_tex is only available after the GTAO section below.
		// the bind groups are rebuilt after gtao_depth_tex is known (see below).
		// placeholder views and bind groups are set to the fxaa_ldr_view temporarily.

		let staa_enabled = render_tier != RenderTier::LowGles; // hardware capability only; quality.staa gates it at runtime via dev_staa

		let staa_history_a_texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[staa] history A"),
			size: wgpu::Extent3d {
				width: config.width,
				height: config.height,
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

		let staa_history_b_texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[staa] history B"),
			size: wgpu::Extent3d {
				width: config.width,
				height: config.height,
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

		let staa_nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[staa] nearest sampler"),
			mag_filter: wgpu::FilterMode::Nearest,
			min_filter: wgpu::FilterMode::Nearest,
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			..Default::default()
		});

		let staa_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[staa] params buffer"),
			size: STAA_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let staa_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[staa] bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(STAA_PARAMS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						// depth texture: Depth32Float bound as unfilterable float
						sample_type: wgpu::TextureSampleType::Float { filterable: false },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 4,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 5,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
					count: None,
				},
			],
		});

		// bind groups are rebuilt once gtao_depth_tex is known (at the end of GTAO section).
		// placeholder: use fxaa_ldr_view for depth until the real depth is available.
		// these are immediately overwritten in the "finalize taa bind groups" block below.
		let staa_bg_placeholder = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[staa] bg (placeholder)"),
			layout: &staa_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: staa_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&staa_history_a_view),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 5,
					resource: wgpu::BindingResource::Sampler(&staa_nearest_sampler),
				},
			],
		});

		let taa_shader = make_shader!(device, shader_passthrough, "[staa] shader", STAA_SHADER_SRC, "staa.spv");

		let staa_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[staa] pipeline"),
			layout: Some(
				&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
					label: Some("[staa] pipeline layout"),
					bind_group_layouts: &[Some(&staa_bgl)],
					immediate_size: 0,
				}),
			),
			vertex: wgpu::VertexState {
				module: &taa_shader,
				entry_point: Some("vs_main"),
				buffers: &[],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &taa_shader,
				entry_point: Some("fs_main"),
				targets: &[
					// present → swapchain; history → higher precision for accumulation
					Some(wgpu::ColorTargetState {
						format,
						blend: None,
						write_mask: wgpu::ColorWrites::ALL,
					}),
					Some(wgpu::ColorTargetState {
						format: STAA_HISTORY_FORMAT,
						blend: None,
						write_mask: wgpu::ColorWrites::ALL,
					}),
				],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── SSR (screen-space reflections, mid+ tier) ─────────────────────

		let ssr_enabled = true; // pipeline always built; quality.ssr gates it at runtime via dev_ssr
		let ssr_w = (config.width / 2).max(1);
		let ssr_h = (config.height / 2).max(1);

		let ssr_texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[ssr] reflection texture"),
			size: wgpu::Extent3d {
				width: ssr_w,
				height: ssr_h,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: hdr_format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let ssr_view = ssr_texture.create_view(&wgpu::TextureViewDescriptor::default());

		let ssr_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[ssr] params buffer"),
			size: SSR_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		// group 0: globals + hdr + depth + samplers
		// group 0: globals + hdr_tex + depth_tex (float, textureLoad) + linear sampler
		let ssr_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[ssr] bgl0"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: false },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				// linear sampler for HDR texture sampling on ray hit
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});
		// group 1: SSR params
		let ssr_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[ssr] bgl1"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(SSR_PARAMS_SIZE),
				},
				count: None,
			}],
		});

		// point (non-filtering) sampler for depth texture reads in SSR + fog
		let _depth_point_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[depth] point sampler"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Nearest,
			min_filter: wgpu::FilterMode::Nearest,
			..Default::default()
		});

		// SSR bg0 uses the non-MSAA depth texture (created below in GTAO section).
		// Declare as uninitialized here and assign after GTAO init via shadowing let.
		let ssr_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[ssr] bg1"),
			layout: &ssr_bgl1,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: ssr_params_buf.as_entire_binding(),
			}],
		});

		let ssr_shader = make_shader!(device, shader_passthrough, "[ssr] shader", SSR_SHADER_SRC, "ssr.spv");
		let ssr_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("[ssr] pipeline layout"),
			bind_group_layouts: &[Some(&ssr_bgl0), Some(&ssr_bgl1)],
			immediate_size: 0,
		});
		let ssr_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[ssr] pipeline"),
			layout: Some(&ssr_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &ssr_shader,
				entry_point: Some("vs_main"),
				buffers: &[],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &ssr_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── volumetric fog (mid+ tier) ─────────────────────────────────────

		let fog_enabled = quality.volumetric_fog;
		let fog_w = (config.width / 2).max(1);
		let fog_h = (config.height / 2).max(1);

		let fog_texture = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[fog] scattering texture"),
			size: wgpu::Extent3d {
				width: fog_w,
				height: fog_h,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: hdr_format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let fog_view = fog_texture.create_view(&wgpu::TextureViewDescriptor::default());

		let fog_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[fog] params buffer"),
			size: FOG_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let fog_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[fog] bgl0"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: false },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
			],
		});
		let fog_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[fog] bgl1"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(FOG_PARAMS_SIZE),
				},
				count: None,
			}],
		});

		// fog bg0 uses the non-MSAA depth texture (created in GTAO section, assigned below).
		let fog_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[fog] bg1"),
			layout: &fog_bgl1,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: fog_params_buf.as_entire_binding(),
			}],
		});

		let fog_shader = make_shader!(device, shader_passthrough, "[fog] shader", FOG_SHADER_SRC, "volumetric_fog.spv");
		let fog_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("[fog] pipeline layout"),
			bind_group_layouts: &[Some(&fog_bgl0), Some(&fog_bgl1)],
			immediate_size: 0,
		});
		let fog_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[fog] pipeline"),
			layout: Some(&fog_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &fog_shader,
				entry_point: Some("vs_main"),
				buffers: &[],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &fog_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
					blend: None,
					write_mask: wgpu::ColorWrites::ALL,
				})],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── atmospheric scattering sky (mid+ tier) ────────────────────────

		let atmos_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[atmos] params buffer"),
			size: ATMOS_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		// group 0: globals + depth texture (read via textureLoad to check geometry coverage)
		let atmos_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[atmos] bgl0"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: false },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
			],
		});

		let atmos_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[atmos] bgl1"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(ATMOS_PARAMS_SIZE),
				},
				count: None,
			}],
		});

		let atmos_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[atmos] bg1"),
			layout: &atmos_bgl1,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: atmos_params_buf.as_entire_binding(),
			}],
		});

		let atmos_shader = make_shader!(device, shader_passthrough, "[atmos] shader", ATMOS_SHADER_SRC, "atmos.spv");
		let atmos_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[atmos] pipeline layout"),
				bind_group_layouts: &[Some(&atmos_bgl0), Some(&atmos_bgl1)],
				immediate_size: 0,
			});
		let atmos_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[atmos] pipeline"),
			layout: Some(&atmos_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &atmos_shader,
				entry_point: Some("vs_main"),
				buffers: &[],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &atmos_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
					// alpha blend: sky only writes to pixels with depth=1.0 (output alpha 0 for geometry)
					blend: Some(wgpu::BlendState::ALPHA_BLENDING),
					write_mask: wgpu::ColorWrites::ALL,
				})],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			}),
			primitive: wgpu::PrimitiveState::default(),
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});
		// atmos_bg0 needs gtao_depth_tex (created in GTAO section); assigned after that section.

		// ── water rendering — Gerstner waves + refraction ─────────────────

		let water_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[water] params buffer"),
			size: WATER_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let water_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[water] bgl0"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				// binding 3: reflection_tex (planar reflection; 1×1 fallback when disabled)
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
			],
		});

		let water_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[water] bgl1"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(WATER_PARAMS_SIZE),
				},
				count: None,
			}],
		});

		// 1×1 black Rgba16Float fallback — used as reflection_tex when planar reflections are off
		let reflection_fallback_tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("[reflection] fallback tex"),
			size: wgpu::Extent3d {
				width: 1,
				height: 1,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: hdr_format,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
			view_formats: &[],
		});
		let reflection_fallback_view =
			reflection_fallback_tex.create_view(&wgpu::TextureViewDescriptor::default());

		let water_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[water] bg0"),
			layout: &water_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&hdr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&reflection_fallback_view),
				},
			],
		});

		let water_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[water] bg1"),
			layout: &water_bgl1,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: water_params_buf.as_entire_binding(),
			}],
		});

		let water_shader = make_shader!(device, shader_passthrough, "[water] shader", WATER_SHADER_SRC, "water.spv");
		let water_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[water] pipeline layout"),
				bind_group_layouts: &[Some(&water_bgl0), Some(&water_bgl1)],
				immediate_size: 0,
			});
		let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[water] pipeline"),
			layout: Some(&water_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &water_shader,
				entry_point: Some("vs_main"),
				buffers: vertex_buffers,
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &water_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
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
			multisample: wgpu::MultisampleState {
				count: msaa_samples,
				..Default::default()
			},
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── decal system — box-projected, depth-sampled ───────────────────

		let decal_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[decal] params buffer"),
			size: DECAL_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let decal_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[decal] bgl0"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: false },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
			],
		});

		let decal_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[decal] bgl1"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(DECAL_PARAMS_SIZE),
				},
				count: None,
			}],
		});

		let decal_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[decal] bg1"),
			layout: &decal_bgl1,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: decal_params_buf.as_entire_binding(),
			}],
		});

		let decal_shader = make_shader!(device, shader_passthrough, "[decal] shader", DECAL_SHADER_SRC, "decal.spv");
		let decal_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[decal] pipeline layout"),
				bind_group_layouts: &[Some(&decal_bgl0), Some(&decal_bgl1)],
				immediate_size: 0,
			});
		let decal_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[decal] pipeline"),
			layout: Some(&decal_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &decal_shader,
				entry_point: Some("vs_main"),
				buffers: &[],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &decal_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
					blend: Some(wgpu::BlendState::ALPHA_BLENDING),
					write_mask: wgpu::ColorWrites::ALL,
				})],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			}),
			primitive: wgpu::PrimitiveState {
				topology: wgpu::PrimitiveTopology::TriangleList,
				cull_mode: Some(wgpu::Face::Front),
				..Default::default()
			},
			depth_stencil: None,
			multisample: wgpu::MultisampleState::default(),
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});
		// decal_bg0 needs gtao_depth_tex; assigned after GTAO section.

		// ── terrain rendering — geometry clipmap ───────────────────────────

		// bg group 0: globals only (shared view-global bind group)
		let terrain_globals_bgl =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[terrain] globals bgl"),
				entries: &[wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
					},
					count: None,
				}],
			});

		let terrain_globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[terrain] globals bg"),
			layout: &terrain_globals_bgl,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: globals_buf.as_entire_binding(),
			}],
		});

		let terrain_params_bgl =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[terrain] params bgl"),
				entries: &[
					// binding 0: TerrainParams uniform
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Uniform,
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(TERRAIN_PARAMS_SIZE),
						},
						count: None,
					},
					// binding 1: heightmap texture
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::VERTEX,
						ty: wgpu::BindingType::Texture {
							sample_type: wgpu::TextureSampleType::Float { filterable: true },
							view_dimension: wgpu::TextureViewDimension::D2,
							multisampled: false,
						},
						count: None,
					},
					// binding 2: heightmap sampler
					wgpu::BindGroupLayoutEntry {
						binding: 2,
						visibility: wgpu::ShaderStages::VERTEX,
						ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
						count: None,
					},
				],
			});

		let terrain_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[terrain] pipeline layout"),
				bind_group_layouts: &[Some(&terrain_globals_bgl), Some(&terrain_params_bgl)],
				immediate_size: 0,
			});

		let terrain_shader = make_shader!(device, shader_passthrough, "[terrain] shader", TERRAIN_SHADER_SRC, "terrain.spv");

		let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
			label: Some("[terrain] pipeline"),
			layout: Some(&terrain_pipeline_layout),
			vertex: wgpu::VertexState {
				module: &terrain_shader,
				entry_point: Some("vs_main"),
				buffers: &[wgpu::VertexBufferLayout {
					array_stride: VERTEX_STRIDE,
					step_mode: wgpu::VertexStepMode::Vertex,
					attributes: &wgpu::vertex_attr_array![
						0 => Float32x3, // position
						1 => Snorm8x4,  // normal (ignored by terrain shader)
						2 => Snorm8x4,  // tangent (ignored)
						3 => Unorm16x2, // uv (ignored)
						4 => Unorm16x2, // uv_lightmap (ignored)
						5 => Unorm8x4,  // color (ignored)
					],
				}],
				compilation_options: wgpu::PipelineCompilationOptions::default(),
			},
			fragment: Some(wgpu::FragmentState {
				module: &terrain_shader,
				entry_point: Some("fs_main"),
				targets: &[Some(wgpu::ColorTargetState {
					format: hdr_format,
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
				count: msaa_samples,
				mask: !0,
				alpha_to_coverage_enabled: false,
			},
			cache: pipeline_cache_ref,
			multiview_mask: None,
		});

		// ── GTAO ───────────────────────────────────────────────────────────

		let ssao_enabled = quality.ssao;
		let ao_w = (config.width / 2).max(1);
		let ao_h = (config.height / 2).max(1);

		// non-MSAA depth texture dedicated to GTAO input
		let gtao_depth_tex = Self::make_depth_view(&device, config.width, config.height, 1);

		let gtao_ao_a = device.create_texture(&wgpu::TextureDescriptor {
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
		let gtao_ao_b = device.create_texture(&wgpu::TextureDescriptor {
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
		let gtao_ao_view_a = gtao_ao_a.create_view(&wgpu::TextureViewDescriptor::default());
		let gtao_ao_view_b = gtao_ao_b.create_view(&wgpu::TextureViewDescriptor::default());

		let gtao_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[gtao] params buffer"),
			size: GTAO_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		// GTAO non-MSAA z-prepass (sample_count=1, writes to gtao_depth_tex)
		// reuses same vertex format as z-prepass but with no multisample
		let zprepass_nonmsaa_pipeline =
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("3d z-prepass (gtao depth, non-MSAA) pipeline"),
				layout: Some(&zprepass_pipeline_layout),
				vertex: wgpu::VertexState {
					module: &shader,
					entry_point: Some("vs_depth"),
					buffers: pos_vertex_buffers,
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
				multisample: wgpu::MultisampleState::default(), // always sample_count=1
				cache: pipeline_cache_ref,
				multiview_mask: None,
			});

		let gtao_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[gtao] bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(GTAO_PARAMS_SIZE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: false },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: false },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 4,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
					count: None,
				},
			],
		});

		let gtao_point_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("[gtao] point sampler"),
			mag_filter: wgpu::FilterMode::Nearest,
			min_filter: wgpu::FilterMode::Nearest,
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			..Default::default()
		});

		let gtao_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
			label: Some("[gtao] pipeline layout"),
			bind_group_layouts: &[Some(&gtao_bgl)],
			immediate_size: 0,
		});

		let gtao_shader = make_shader!(device, shader_passthrough, "[gtao] shader", GTAO_SHADER_SRC, "gtao.spv");

		let gtao_ao_format = wgpu::TextureFormat::Rg16Float;

		let make_gtao_pipeline = |entry: &'static str, blend: Option<wgpu::BlendState>| {
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some(entry),
				layout: Some(&gtao_pipeline_layout),
				vertex: wgpu::VertexState {
					module: &gtao_shader,
					entry_point: Some("vs_main"),
					buffers: &[],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				},
				fragment: Some(wgpu::FragmentState {
					module: &gtao_shader,
					entry_point: Some(entry),
					targets: &[Some(wgpu::ColorTargetState {
						format: gtao_ao_format,
						blend,
						write_mask: wgpu::ColorWrites::ALL,
					})],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				}),
				primitive: wgpu::PrimitiveState::default(),
				depth_stencil: None,
				multisample: wgpu::MultisampleState::default(),
				cache: pipeline_cache_ref,
				multiview_mask: None,
			})
		};

		let gtao_pipeline = make_gtao_pipeline("fs_gtao", None);
		let gtao_blur_h_pipeline = make_gtao_pipeline("fs_blur_h", None);
		let gtao_blur_v_pipeline = make_gtao_pipeline("fs_blur_v", None);

		// main pass writes to ao_a, so bind ao_b as the dummy src to avoid read/write conflict
		let gtao_main_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[gtao] main bg"),
			layout: &gtao_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: gtao_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&gtao_ao_view_b),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&gtao_point_sampler),
				},
			],
		});
		let gtao_blur_h_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[gtao] blur-h bg"),
			layout: &gtao_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: gtao_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&gtao_ao_view_a),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&gtao_point_sampler),
				},
			],
		});
		let gtao_blur_v_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[gtao] blur-v bg"),
			layout: &gtao_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: gtao_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&gtao_ao_view_b),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&gtao_point_sampler),
				},
			],
		});

		// rebuild composite_bg now that ao_view_a, ssr_view, and fog_view are available
		// binding 4 = ssr_tex, binding 5 = fog_tex, binding 6 = sampler
		let composite_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[composite] bg"),
			layout: &composite_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: composite_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&hdr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(
						bloom_mip_views.first().unwrap_or(&hdr_view),
					),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&gtao_ao_view_a),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::TextureView(&ssr_view),
				},
				wgpu::BindGroupEntry {
					binding: 5,
					resource: wgpu::BindingResource::TextureView(&fog_view),
				},
				wgpu::BindGroupEntry {
					binding: 6,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 7,
					resource: wgpu::BindingResource::TextureView(&contact_shadow_fallback_view),
				},
			],
		});

		// rebuild ssr, fog, atmos, and decal bg0 now that the non-MSAA depth texture is available
		let decal_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[decal] bg0"),
			layout: &decal_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
			],
		});
		let atmos_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[atmos] bg0"),
			layout: &atmos_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
			],
		});
		let ssr_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[ssr] bg0"),
			layout: &ssr_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&hdr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
			],
		});
		let fog_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[fog] bg0"),
			layout: &fog_bgl0,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
			],
		});

		// ── particle system (compute simulation, mid+ tier) ───────────────

		let particles_enabled = render_tier != RenderTier::LowGles;
		let particle_cap = quality.particle_cap.max(1);

		let particle_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[particles] storage buffer"),
			size: particle_cap as u64 * PARTICLE_STRIDE,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let particle_sim_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("[particles] sim params buffer"),
			size: PARTICLE_SIM_PARAMS_SIZE,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let particle_sim_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("[particles] sim bgl"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::COMPUTE,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Storage { read_only: false },
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(PARTICLE_STRIDE),
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::COMPUTE,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(PARTICLE_SIM_PARAMS_SIZE),
					},
					count: None,
				},
			],
		});

		let particle_sim_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[particles] sim bg"),
			layout: &particle_sim_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: particle_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: particle_sim_params_buf.as_entire_binding(),
				},
			],
		});

		let particle_render_bgl =
			device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
				label: Some("[particles] render bgl"),
				entries: &[
					wgpu::BindGroupLayoutEntry {
						binding: 0,
						visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Uniform,
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
						},
						count: None,
					},
					wgpu::BindGroupLayoutEntry {
						binding: 1,
						visibility: wgpu::ShaderStages::VERTEX,
						ty: wgpu::BindingType::Buffer {
							ty: wgpu::BufferBindingType::Storage { read_only: true },
							has_dynamic_offset: false,
							min_binding_size: wgpu::BufferSize::new(PARTICLE_STRIDE),
						},
						count: None,
					},
				],
			});

		let particle_render_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[particles] render bg"),
			layout: &particle_render_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: globals_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: particle_buf.as_entire_binding(),
				},
			],
		});

		let particle_sim_shader = make_shader!(device, shader_passthrough, "[particles] sim shader", PARTICLE_SIM_SHADER_SRC, "particle_sim.spv");
		let particle_sim_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[particles] sim pipeline layout"),
				bind_group_layouts: &[Some(&particle_sim_bgl)],
				immediate_size: 0,
			});
		let particle_sim_pipeline =
			device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
				label: Some("[particles] sim compute pipeline"),
				layout: Some(&particle_sim_pipeline_layout),
				module: &particle_sim_shader,
				entry_point: Some("cs_simulate"),
				compilation_options: wgpu::PipelineCompilationOptions::default(),
				cache: pipeline_cache_ref,
			});

		let particle_render_shader = make_shader!(device, shader_passthrough, "[particles] render shader", PARTICLE_RENDER_SHADER_SRC, "particle_render.spv");
		let particle_render_pipeline_layout =
			device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("[particles] render pipeline layout"),
				bind_group_layouts: &[Some(&particle_render_bgl)],
				immediate_size: 0,
			});
		let particle_render_pipeline =
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("[particles] render pipeline"),
				layout: Some(&particle_render_pipeline_layout),
				vertex: wgpu::VertexState {
					module: &particle_render_shader,
					entry_point: Some("vs_main"),
					buffers: &[],
					compilation_options: wgpu::PipelineCompilationOptions::default(),
				},
				fragment: Some(wgpu::FragmentState {
					module: &particle_render_shader,
					entry_point: Some("fs_main"),
					targets: &[Some(wgpu::ColorTargetState {
						format: hdr_format,
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
				multisample: wgpu::MultisampleState {
					count: msaa_samples,
					..Default::default()
				},
				cache: pipeline_cache_ref,
				multiview_mask: None,
			});

		let particle_cpu: Vec<CpuParticle> =
			(0..particle_cap).map(|_| CpuParticle::dead()).collect();

		// ── sky meshes ─────────────────────────────────────────────────────

		let dome_mesh = Self::upload_mesh_data(&device, &queue, &sphere_mesh(SKY_RADIUS, 32, 16));
		let sun_mesh = Self::upload_mesh_data(&device, &queue, &quad_mesh(40.0, 40.0));

		log::info!(
			"lunar-render-3d initialized: {}×{}, vsync={}, tier={:?}",
			config.width,
			config.height,
			config.vsync,
			render_tier,
		);

		// finalize taa bind groups now that gtao_depth_tex is available
		// even frame: reads history_a, writes to [swapchain, history_b]
		let staa_bg_even = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[staa] bg even"),
			layout: &staa_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: staa_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&staa_history_a_view),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 5,
					resource: wgpu::BindingResource::Sampler(&staa_nearest_sampler),
				},
			],
		});
		// odd frame: reads history_b, writes to [swapchain, history_a]
		let staa_bg_odd = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("[staa] bg odd"),
			layout: &staa_bgl,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: staa_params_buf.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view),
				},
				wgpu::BindGroupEntry {
					binding: 2,
					resource: wgpu::BindingResource::TextureView(&gtao_depth_tex),
				},
				wgpu::BindGroupEntry {
					binding: 3,
					resource: wgpu::BindingResource::TextureView(&staa_history_b_view),
				},
				wgpu::BindGroupEntry {
					binding: 4,
					resource: wgpu::BindingResource::Sampler(&post_sampler),
				},
				wgpu::BindGroupEntry {
					binding: 5,
					resource: wgpu::BindingResource::Sampler(&staa_nearest_sampler),
				},
			],
		});
		drop(staa_bg_placeholder);

		// clone before move into struct — wgpu::Device is Arc-backed, clone is cheap
		#[cfg(not(target_arch = "wasm32"))]
		let device_for_belt = device.clone();

		Self {
			device,
			queue,
			surface,
			msaa_samples,
			msaa_color_view,
			surface_config,
			render_tier,
			hdr_format,
			render_w: config.width,
			render_h: config.height,
			render_scale: 1.0,
			depth_view,
			globals_buf,
			globals_bg,
			globals_bgl,
			material_bgl,
			material_buf,
			material_bg,
			material_staging,
			mesh_bgl,
			entity_buf,
			entity_bg,
			entity_capacity,
			opaque_pipeline,
			sky_pipeline,
			zprepass_pipeline,
			lights_bgl,
			lights_buf,
			lights_bg,
			shadow_map_view,
			shadow_sampler,
			shadow_globals_buf,
			shadow_globals_bgl,
			shadow_globals_bg,
			shadow_pipeline,
			shadow_cascade_views,
			shadow_cascade_dirty: [true; 3],
			shadow_last_dir: Vec3::ZERO,
			shadow_last_draw_count: 0,
			shadow_entities_scratch: HashSet::default(),
			shadow_list_scratch: Vec::new(),
			point_shadow_tex,
			point_shadow_face_views,
			point_shadow_array_view,
			point_shadow_globals_bgl,
			point_shadow_globals_buf,
			point_shadow_globals_bg,
			point_shadow_pipeline,
			point_shadow_dirty: [[true; 6]; MAX_POINT_SHADOW_LIGHTS],
			point_shadow_last_positions: [Vec3::ZERO; MAX_POINT_SHADOW_LIGHTS],
			point_shadow_last_draw_count: 0,
			cluster_shader_src_loaded: true,
			cluster_bgl_compute,
			cluster_bgl_render,
			cluster_pipeline,
			cluster_params_buf,
			light_list_buf,
			cluster_counts_buf,
			cluster_indices_buf,
			cluster_bg_compute,
			cluster_bg_render,
			surface_bgl,
			surface_pipeline,
			surface_fallback_tex,
			surface_fallback_view,
			surface_sampler,
			surface_params_buf,
			surface_tex_cache: HashMap::default(),
			surface_bg_cache: HashMap::default(),
			surface_scratch: Vec::new(),
			mesh_gpu: HashMap::default(),
			dome_mesh,
			sun_mesh,
			hdr_texture,
			hdr_view,
			bloom_enabled,
			bloom_mip_views,
			bloom_mip_sizes,
			bloom_params_buf,
			bloom_downsample_bgl,
			bloom_downsample_bgs,
			bloom_upsample_bgs,
			bloom_downsample_pipeline,
			bloom_upsample_pipeline,
			composite_params_buf,
			composite_bgl,
			composite_bg,
			composite_pipeline,
			post_sampler,
			ssao_enabled,
			gtao_depth_view: gtao_depth_tex,
			gtao_ao_a,
			gtao_ao_b,
			gtao_ao_view_a,
			gtao_ao_view_b,
			gtao_params_buf,
			gtao_bgl,
			gtao_point_sampler,
			gtao_main_bg,
			gtao_blur_h_bg,
			gtao_blur_v_bg,
			gtao_pipeline,
			gtao_blur_h_pipeline,
			gtao_blur_v_pipeline,
			zprepass_nonmsaa_pipeline,
			transparent_pipeline,
			transparent_scratch: Vec::new(),
			transparent_last_depths: Vec::new(),
			transparent_depths_scratch: Vec::new(),
			transparent_last_cam_fwd: Vec3::ZERO,
			upscale_active: false,
			fsr_ldr_texture: None,
			fsr_ldr_view: None,
			fsr_mid_texture: None,
			fsr_mid_view: None,
			upscale_nearest_pipeline: None,
			upscale_linear_pipeline: None,
			upscale_lanczos_pipeline: None,
			upscale_bicubic_pipeline: None,
			fsr_easu_pipeline: None,
			fsr_rcas_pipeline: None,
			fsr_bgl: None,
			fsr_easu_bg: None,
			fsr_rcas_bg: None,
			fsr_params_buf: None,
			fxaa_enabled,
			fxaa_ldr_texture,
			fxaa_ldr_view,
			fxaa_bgl,
			fxaa_bg,
			fxaa_params_buf,
			fxaa_pipeline,
			staa_enabled,
			staa_frame_index: 0,
			staa_prev_vp_jittered: Mat4::IDENTITY,
			staa_prev_jitter: Vec2::ZERO,
			staa_history_a_texture,
			staa_history_a_view,
			staa_history_b_texture,
			staa_history_b_view,
			staa_bgl,
			staa_bg_even,
			staa_bg_odd,
			staa_params_buf,
			staa_pipeline,
			staa_nearest_sampler,
			staa_ping: false,
			ssr_enabled,
			ssr_texture,
			ssr_view,
			ssr_bgl0,
			ssr_bgl1,
			ssr_bg0,
			ssr_bg1,
			ssr_params_buf,
			ssr_pipeline,
			fog_enabled,
			fog_texture,
			fog_view,
			fog_bgl0,
			fog_bgl1,
			fog_bg0,
			fog_bg1,
			fog_params_buf,
			fog_pipeline,
			atmos_bgl0,
			atmos_bgl1,
			atmos_bg0,
			atmos_bg1,
			atmos_params_buf,
			atmos_pipeline,
			water_params_buf,
			water_bgl0,
			water_bgl1,
			water_bg0,
			water_bg1,
			water_pipeline,
			decal_params_buf,
			decal_bgl0,
			decal_bgl1,
			decal_bg0,
			decal_bg1,
			decal_pipeline,
			particles_enabled,
			particle_cap,
			particle_buf,
			particle_sim_params_buf,
			particle_sim_bgl,
			particle_sim_bg,
			particle_sim_pipeline,
			particle_render_bgl,
			particle_render_bg,
			particle_render_pipeline,
			particle_cpu,
			particle_alive_count: 0,
			particle_spawn_scratch: Vec::new(),
			particle_gpu_writes: Vec::new(),
			particle_upload_scratch: Vec::new(),
			terrain_pipeline,
			terrain_globals_bgl,
			terrain_globals_bg,
			terrain_params_bgl,
			terrain_gpu: HashMap::default(),
			msaa_main_shader: shader,
			msaa_surface_shader: surface_shader_module,
			msaa_water_shader: water_shader,
			msaa_terrain_shader: terrain_shader,
			msaa_particle_render_shader: particle_render_shader,
			#[cfg(not(target_arch = "wasm32"))]
			pipeline_cache,
			#[cfg(not(target_arch = "wasm32"))]
			pipeline_cache_path,
			// 4 MiB chunk — larger than any single write, handles most scene sizes
			#[cfg(not(target_arch = "wasm32"))]
			staging_belt: wgpu::util::StagingBelt::new(device_for_belt, 4 * 1024 * 1024),
			frame_time_ema_ms: 16.67,
			resolution_scale: 1.0,
			frame_time_budget_ms: 14.0,
			auto_quality_over_frames: 0,
			auto_quality_under_frames: 0,
			static_bundle: None,
			static_draw_list: Vec::new(),
			static_list_scratch: Vec::new(),
			static_bundle_params: (wgpu::TextureFormat::Rgba16Float, 0),
			static_entity_count: 0,
			static_entity_slots: HashMap::default(),
			lightmap_bgl,
			lightmap_sampler,
			lightmap_fallback_tex,
			lightmap_fallback_view,
			dir_lm_fallback_tex,
			dir_lm_fallback_view,
			lightmap_fallback_bg,
			lm_tex_cache: HashMap::default(),
			dir_lm_tex_cache: HashMap::default(),
			lightmap_bg_cache: HashMap::default(),
			atlas_tex: None,
			atlas_view: None,
			atlas_bg: None,
			atlas_lm_uvs: HashMap::default(),
			atlas_lm_ids: Vec::new(),
			mega_vbuf: None,
			mega_ibuf: None,
			mega_vbuf_bytes: 0,
			mega_ibuf_bytes: 0,
			mega_mesh_entries: HashMap::default(),
			entity_draw_params_buf: None,
			frustum_visible: HashSet::default(),
			frustum_flags_scratch: Vec::new(),
			raw_scratch: Vec::new(),
			draw_scratch: Vec::new(),
			draw_sort_keys: Vec::new(),
			draw_sorted_scratch: Vec::new(),
			impostor_scratch: Vec::new(),
			uniform_staging,
			point_light_scratch: Vec::new(),
			bsp_visible_scratch: HashSet::default(),
			bsp_visible_active: false,
			portal_visible_scratch: HashSet::default(),
			portal_visible_active: false,
			static_entities_scratch: HashSet::default(),
			cull_aabb_scratch: Vec::new(),
			light_data_scratch: Vec::new(),
			cluster_counts_scratch: Vec::new(),
			cluster_indices_scratch: Vec::new(),
			cpu_cluster_last_count: usize::MAX,
			late_aabb_scratch: Vec::new(),
			dp_data_scratch: Vec::new(),
			mesh_evict_scratch: Vec::new(),
			coverage_hints_scratch: Vec::new(),
			shadow_indices_scratch: Vec::new(),
			lm_needed_scratch: Vec::new(),
			lm_evict_scratch: Vec::new(),
			surface_evict_scratch: Vec::new(),

			render_graph: Self::build_render_graph(
				render_tier,
				bloom_enabled,
				ssr_enabled,
				fog_enabled,
				fxaa_enabled,
				ssao_enabled,
				staa_enabled,
			),

			has_indirect,
			shader_passthrough,
			indirect_buf: None,
			indirect_args: Vec::new(),

			gpu_cull_enabled: render_tier == RenderTier::High,
			cull_aabb_buf: None,
			cull_frustum_buf: None,
			cull_flags_buf: None,
			cull_flags_staging: None,
			cull_count_buf: None,
			cull_bgl: None,
			cull_pipeline: None,
			cull_bg: None,
			gpu_cull_flags: Vec::new(),
			cull_entity_capacity: 0,
			cull_staging_pending: false,
			cull_pending_entity_count: 0,
			cull_indirect_bgl: None,
			cull_indirect_pipeline: None,
			cull_draw_params_buf: None,
			cull_indirect_count_buf: None,
			late_cull_frustum_buf: None,

			hzb_enabled: render_tier == RenderTier::High,
			hzb_texture: None,
			hzb_mip_views: Vec::new(),
			hzb_src_view: None,
			hzb_width: config.width,
			hzb_height: config.height,
			hzb_mip_count: 0,
			hzb_downsample_bgl: None,
			hzb_downsample_pipeline: None,
			hzb_copy_bgl: None,
			hzb_copy_pipeline: None,
			hzb_cull_bgl: None,
			hzb_cull_pipeline: None,
			hzb_copy_bg: None,
			hzb_downsample_bgs: Vec::new(),
			hzb_cull_bg: None,
			hzb_depth_src: None,
			hzb_depth_src_view: None,
			hzb_occ_flags: Vec::new(),
			hzb_occ_buf: None,
			hzb_occ_staging: None,
			hzb_cull_aabb_buf: None,
			hzb_cull_params_buf: None,
			hzb_staging_pending: false,
			hzb_pending_entity_count: 0,
			cull_staging_ready: Arc::new(AtomicBool::new(false)),
			hzb_staging_ready: Arc::new(AtomicBool::new(false)),

			contact_shadow_tex: None,
			contact_shadow_view: None,
			contact_shadow_bgl: None,
			contact_shadow_pipeline: None,
			contact_shadow_params_buf: None,
			contact_shadow_bg: None,
			contact_shadow_fallback_tex,
			contact_shadow_fallback_view,
			composite_bg_dirty: false,


			reflection_tex: None,
			reflection_view: None,
			reflection_depth_tex: None,
			reflection_depth_view: None,
			reflection_globals_buf: None,
			reflection_globals_bg: None,
			reflection_fallback_tex,
			reflection_fallback_view,
			water_bg_dirty: false,

			detail_sprite_bgl: None,
			detail_sprite_pipeline: None,
			detail_sprite_compute_bgl: None,
			detail_sprite_compute_pipeline: None,
			detail_sprite_cache: HashMap::default(),

			lod_select_bgl: None,
			lod_select_pipeline: None,
			lod_select_bg: None,
			lod_params_buf: None,
			lod_indices_buf: None,
			lod_indices_staging: None,
			gpu_lod_indices: HashMap::default(),
			lod_staging_pending: false,
			lod_pending_entity_count: 0,
			lod_staging_ready: Arc::new(AtomicBool::new(false)),
		}
	}
	/// load the 3d pipeline cache from disk if available (Vulkan/DX12 only).
	#[cfg(not(target_arch = "wasm32"))]
	/// disk path for this adapter's pipeline cache, or `None` if the device lacks the
	/// `PIPELINE_CACHE` feature. keyed on vendor/device/backend so different GPUs or a
	/// driver that reports a different backend don't overwrite each other's blob.
	#[cfg(not(target_arch = "wasm32"))]
	pub(crate) fn pipeline_cache_path(adapter: &wgpu::Adapter) -> Option<std::path::PathBuf> {
		if !adapter.features().contains(wgpu::Features::PIPELINE_CACHE) {
			return None;
		}
		let info = adapter.get_info();
		// short stable key; wgpu also validates an internal header on load (fallback=true),
		// so this is just to avoid cross-GPU churn, not the correctness guard.
		let key = format!("{:04x}_{:04x}_{:?}", info.vendor, info.device, info.backend);
		Some(std::path::PathBuf::from(format!(".pipeline_cache_3d_{key}.bin")))
	}
	/// load (or bootstrap) the pipeline cache. returns `None` when the feature is off or no
	/// path was resolved. when the file is absent it still creates an empty cache so the
	/// driver has somewhere to accumulate PSOs that `save_pipeline_cache` can persist.
	pub(crate) fn load_pipeline_cache(
		device: &wgpu::Device,
		path: Option<&std::path::Path>,
	) -> Option<wgpu::PipelineCache> {
		let path = path?;
		// reading a wrong-GPU blob is harmless: create_pipeline_cache validates the header and,
		// with fallback=true, silently starts fresh. so we always pass whatever data we have.
		let data = std::fs::read(path).ok();
		match &data {
			Some(bytes) => log::info!("[render-3d] loaded pipeline cache ({} bytes)", bytes.len()),
			None => log::info!("[render-3d] no pipeline cache yet — bootstrapping empty"),
		}
		// SAFETY: fallback=true so wgpu rebuilds a fresh cache if validation fails; only runs
		// on Vulkan/DX12 (gated by the PIPELINE_CACHE feature) where the format is stable.
		Some(unsafe {
			device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
				label: Some("[render-3d] pipeline cache"),
				data: data.as_deref(),
				fallback: true,
			})
		})
	}
	/// persist pipeline cache to disk. call before engine shutdown to speed up
	/// shader compilation on the next launch (Vulkan/DX12 only).
	#[cfg(not(target_arch = "wasm32"))]
	pub fn save_pipeline_cache(&self) {
		if let Some(ref cache) = self.pipeline_cache
			&& let Some(ref path) = self.pipeline_cache_path
			&& let Some(data) = cache.get_data()
		{
			match std::fs::write(path, &data) {
				Ok(()) => log::info!("[render-3d] saved pipeline cache ({} bytes)", data.len()),
				Err(err) => log::warn!("[render-3d] pipeline cache save failed: {err}"),
			}
		}
	}

	// ── helpers ────────────────────────────────────────────────────────────
}

/// persist the pipeline cache on shutdown so the next launch skips PSO recompilation.
/// mirrors the 2D `RenderEngine` Drop; no-op when the feature is unavailable (path is None).
#[cfg(not(target_arch = "wasm32"))]
impl Drop for RenderEngine3d {
	fn drop(&mut self) {
		self.save_pipeline_cache();
	}
}
