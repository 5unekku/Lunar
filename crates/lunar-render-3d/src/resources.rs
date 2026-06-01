//! `RenderEngine3d` — gpu resource creation: ensure_*, make_*, build_*_resources, render graph, mega buffers.
//!
//! split out of `lib.rs`; methods stay on `RenderEngine3d` (one type, many
//! `impl` blocks across sibling modules — all share the struct's private fields).

use super::*;

impl RenderEngine3d {
    pub(crate) fn build_render_graph(
        tier: RenderTier,
        bloom: bool,
        ssr: bool,
        fog: bool,
        fxaa: bool,
        ssao: bool,
        staa: bool,
    ) -> render_graph::RenderGraph {
        let mut g = render_graph::RenderGraph::new();
        let shadow   = g.texture("shadow_map");
        let depth    = g.texture("depth");
        let hdr      = g.texture("hdr");
        let ao       = g.texture("ao");
        let ssr_tex  = g.texture("ssr");
        let fog_tex  = g.texture("fog");
        let bloom_tex = g.texture("bloom");
        let ldr      = g.texture("ldr");
        let swapchain = g.texture("swapchain");

        // passes in dependency order. the graph's topological sort will produce the
        // same ordering from the declared resource edges, demonstrating the DAG works.
        g.add_pass("shadow",    vec![],                         vec![shadow]);
        if tier != RenderTier::LowGles {
            g.add_pass("zprepass", vec![],                      vec![depth]);
        }
        if ssao || staa {
            // taa needs non-msaa depth for reprojection; share the gtao depth prepass
            g.add_pass("gtao",     vec![depth],                 vec![ao]);
        }
        if tier == RenderTier::High {
            g.add_pass("hzb_build",  vec![depth],               vec![]);
            g.add_pass("hzb_cull",   vec![],                    vec![]);
        }
        g.add_pass("opaque",    vec![shadow, depth],            vec![hdr]);
        g.add_pass("sky",       vec![],                         vec![hdr]);
        g.add_pass("particles", vec![depth],                    vec![hdr]);
        g.add_pass("decals",    vec![depth],                    vec![hdr]);
        g.add_pass("water",     vec![depth, hdr],               vec![hdr]);
        g.add_pass("transparent", vec![depth],                  vec![hdr]);
        if ssr { g.add_pass("ssr", vec![hdr, depth], vec![ssr_tex]); }
        if fog { g.add_pass("volumetric_fog", vec![depth], vec![fog_tex]); }
        if bloom { g.add_pass("bloom", vec![hdr], vec![bloom_tex]); }
        let composite_reads = {
            let mut r = vec![hdr];
            if ssao   { r.push(ao); }
            if ssr    { r.push(ssr_tex); }
            if fog    { r.push(fog_tex); }
            if bloom  { r.push(bloom_tex); }
            r
        };
        g.add_pass("composite", composite_reads, vec![ldr]);
        if staa {
            // taa and fxaa are mutually exclusive; taa replaces fxaa on mid/high tier
            g.add_pass("staa", vec![ldr, depth], vec![swapchain]);
        } else if fxaa {
            g.add_pass("fxaa", vec![ldr], vec![swapchain]);
        } else {
            g.add_pass("present", vec![ldr], vec![swapchain]);
        }
        g
    }
    /// append a mesh to the mega vertex/index buffers (all indices converted to u32).
    /// records base_vertex and first_index for the mesh in mega_mesh_entries.
    pub(crate) fn append_to_mega_buffers(&mut self, mesh_id: u32, data: &MeshData) {
        let vertex_bytes = data.vertices.len() as u64 * VERTEX_STRIDE;
        let index_bytes = (data.indices.len() * 4) as u64; // always u32 in mega-IBO

        // lazy init mega-buffers
        if self.mega_vbuf.is_none() {
            self.mega_vbuf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[mega] vbuf"),
                size: MEGA_VBUF_INIT,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.mega_ibuf.is_none() {
            self.mega_ibuf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[mega] ibuf"),
                size: MEGA_IBUF_INIT,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        // grow mega-VBO if needed
        if self.mega_vbuf_bytes + vertex_bytes > self.mega_vbuf.as_ref().unwrap().size() {
            let new_size = (self.mega_vbuf.as_ref().unwrap().size() * 2).max(self.mega_vbuf_bytes + vertex_bytes);
            let new_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[mega] vbuf"),
                size: new_size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            // mark entries dirty — caller will re-upload when these meshes are next needed
            // simpler: just mark all mega entries dirty — they'll be re-uploaded by caller
            self.mega_mesh_entries.clear();
            self.mega_vbuf_bytes = 0;
            self.mega_ibuf_bytes = 0;
            self.mega_vbuf = Some(new_buf);
        }

        // grow mega-IBO if needed
        if self.mega_ibuf_bytes + index_bytes > self.mega_ibuf.as_ref().unwrap().size() {
            let new_size = (self.mega_ibuf.as_ref().unwrap().size() * 2).max(self.mega_ibuf_bytes + index_bytes);
            let new_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[mega] ibuf"),
                size: new_size,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.mega_mesh_entries.clear();
            self.mega_vbuf_bytes = 0;
            self.mega_ibuf_bytes = 0;
            self.mega_ibuf = Some(new_buf);
        }

        let base_vertex = (self.mega_vbuf_bytes / VERTEX_STRIDE) as u32;
        let first_index = (self.mega_ibuf_bytes / 4) as u32;
        let index_count = data.indices.len() as u32;
        let _ = index_count; // stored in mega_mesh_entries below

        // upload vertices (quantized)
        let qn = |f: f32| -> i8 { (f * 127.0).round().clamp(-127.0, 127.0) as i8 };
        let qu = |f: f32| -> u16 { (f.clamp(0.0, 1.0) * 65535.0).round() as u16 };
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_verts: Vec<GpuVertex3d> = {
            use rayon::prelude::*;
            data.vertices.par_iter().map(|v| GpuVertex3d {
                position:    [v.position.x, v.position.y, v.position.z],
                normal:      [qn(v.normal.x), qn(v.normal.y), qn(v.normal.z), 0],
                tangent:     [qn(v.tangent[0]), qn(v.tangent[1]), qn(v.tangent[2]), qn(v.tangent[3])],
                uv:          [qu(v.uv.x), qu(v.uv.y)],
                uv_lightmap: [qu(v.uv_lightmap.x), qu(v.uv_lightmap.y)],
                color:       v.color,
            }).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let gpu_verts: Vec<GpuVertex3d> = data.vertices.iter().map(|v| GpuVertex3d {
            position:    [v.position.x, v.position.y, v.position.z],
            normal:      [qn(v.normal.x), qn(v.normal.y), qn(v.normal.z), 0],
            tangent:     [qn(v.tangent[0]), qn(v.tangent[1]), qn(v.tangent[2]), qn(v.tangent[3])],
            uv:          [qu(v.uv.x), qu(v.uv.y)],
            uv_lightmap: [qu(v.uv_lightmap.x), qu(v.uv_lightmap.y)],
            color:       v.color,
        }).collect();
        self.queue.write_buffer(
            self.mega_vbuf.as_ref().unwrap(),
            self.mega_vbuf_bytes,
            bytemuck::cast_slice(&gpu_verts),
        );
        self.mega_vbuf_bytes += vertex_bytes;

        // upload indices as u32 (convert u16 → u32 if needed)
        let idx32: Vec<u32> = match &data.indices {
            #[cfg(not(target_arch = "wasm32"))]
            IndexBuffer::U16(v) => { use rayon::prelude::*; v.par_iter().map(|&x| x as u32).collect() }
            #[cfg(target_arch = "wasm32")]
            IndexBuffer::U16(v) => v.iter().map(|&x| x as u32).collect(),
            IndexBuffer::U32(v) => v.clone(),
        };
        self.queue.write_buffer(
            self.mega_ibuf.as_ref().unwrap(),
            self.mega_ibuf_bytes,
            bytemuck::cast_slice(&idx32),
        );
        self.mega_ibuf_bytes += index_bytes;

        // store [first_index, index_count, base_vertex_as_bits]
        self.mega_mesh_entries.insert(mesh_id, [first_index, index_count, base_vertex]);
    }
    /// lazily create (or grow) GPU frustum cull buffers and pipeline.
    pub(crate) fn ensure_gpu_cull_resources(&mut self, entity_count: usize) {
        if entity_count == 0 { return; }
        let needs_rebuild = self.cull_pipeline.is_none() || entity_count > self.cull_entity_capacity;
        if needs_rebuild {
            let cap = entity_count.next_power_of_two().max(256);
            self.cull_entity_capacity = cap;

            // aabb input buffer: 32 bytes per entry (center vec3+pad + half_extent vec3+pad)
            self.cull_aabb_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] aabb buf"),
                size: (cap * 32) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            // frustum params: 6×vec4 planes + u32 count + 3 pad = 112 bytes, padded to 128
            self.cull_frustum_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] frustum buf"),
                size: 128,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            // visible flags: one u32 per entity
            self.cull_flags_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] flags buf"),
                size: (cap * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.cull_flags_staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] flags staging"),
                size: (cap * 4) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.gpu_cull_flags.resize(cap, 0);

            if self.cull_pipeline.is_none() {
                let bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("[cull] bgl"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false, min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false, min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2, visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false, min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
                let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("[cull] pipeline layout"),
                    bind_group_layouts: &[Some(&bgl)],
                    immediate_size: 0,
                });
                let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("[cull] shader"),
                    source: shader_source!(CULL_SHADER_SRC, "cull.spv"),
                });
                self.cull_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("[cull] pipeline"),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some("cs_cull"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                }));
                self.cull_bgl = Some(bgl);

                // create indirect cull pipeline (6 bindings) when has_indirect
                if self.has_indirect && self.cull_indirect_pipeline.is_none() {
                    let storage_ro = |binding: u32| wgpu::BindGroupLayoutEntry {
                        binding, visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false, min_binding_size: None,
                        },
                        count: None,
                    };
                    let storage_rw = |binding: u32| wgpu::BindGroupLayoutEntry {
                        binding, visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false, min_binding_size: None,
                        },
                        count: None,
                    };
                    let indirect_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("[cull indirect] bgl"),
                        entries: &[
                            storage_ro(0),
                            wgpu::BindGroupLayoutEntry {
                                binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: false, min_binding_size: None,
                                },
                                count: None,
                            },
                            storage_rw(2),
                            storage_ro(3),
                            storage_rw(4),
                            storage_rw(5),
                        ],
                    });
                    let indirect_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("[cull indirect] pipeline layout"),
                        bind_group_layouts: &[Some(&indirect_bgl)],
                        immediate_size: 0,
                    });
                    let indirect_module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("[cull indirect] shader"),
                        source: shader_source!(CULL_INDIRECT_SHADER_SRC, "cull_indirect.spv"),
                    });
                    self.cull_indirect_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some("[cull indirect] pipeline"),
                        layout: Some(&indirect_layout),
                        module: &indirect_module,
                        entry_point: Some("cs_cull"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        cache: None,
                    }));
                    self.cull_indirect_bgl = Some(indirect_bgl);
                }
            }

            // grow per-entity draw params and indirect output buffers when has_indirect
            if self.has_indirect {
                self.cull_draw_params_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("[cull] draw params"),
                    size: (cap * 16) as u64, // 4 u32s per entity
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                // indirect_out: 20 bytes per entry, needs both INDIRECT and STORAGE
                if self.indirect_buf.as_ref().map(|b| b.size() < (cap * 20) as u64).unwrap_or(true) {
                    self.indirect_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("[indirect] opaque draw args"),
                        size: (cap * 20) as u64,
                        usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
                if self.cull_indirect_count_buf.is_none() {
                    self.cull_indirect_count_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("[cull] indirect count"),
                        size: 4,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
                if self.late_cull_frustum_buf.is_none() {
                    self.late_cull_frustum_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("[late cull] frustum"),
                        size: 128,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
            }
        }
    }
    /// lazily create HZB texture (R32Float mip chain) and pipelines.
    pub(crate) fn ensure_hzb_resources(&mut self) {
        if self.hzb_texture.is_some() { return; }

        let w = self.hzb_width;
        let h = self.hzb_height;
        let mip_count = (f32::max(w as f32, h as f32).log2().floor() as u32 + 1).max(1);
        self.hzb_mip_count = mip_count;

        // R32Float texture with all mip levels. storage usage required for compute writes.
        let hzb_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[hzb] texture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                 | wgpu::TextureUsages::TEXTURE_BINDING
                 | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // per-mip views for storage writes; full view for sampling in HZB cull
        self.hzb_mip_views = (0..mip_count)
            .map(|mip| hzb_tex.create_view(&wgpu::TextureViewDescriptor {
                label: Some(&format!("[hzb] mip {mip}")),
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            }))
            .collect();
        self.hzb_src_view = Some(hzb_tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("[hzb] full view"),
            ..Default::default()
        }));
        self.hzb_texture = Some(hzb_tex);

        // non-MSAA depth texture as HZB source (depth-only prepass writes here on high tier)
        let depth_src = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[hzb] depth src"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.hzb_depth_src_view = Some(depth_src.create_view(&wgpu::TextureViewDescriptor::default()));
        self.hzb_depth_src = Some(depth_src);

        // depth-copy bgl: group 0 binding 0 = depth_src, binding 1 = hzb_mip0 (storage)
        let copy_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[hzb] copy bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::R32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        // downsample bgl: group 1 binding 0 = src texture_2d, binding 1 = dst storage_2d
        let ds_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[hzb] downsample bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::R32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let hzb_module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[hzb] shader"),
            source: shader_source!(HZB_SHADER_SRC, "hzb.spv"),
        });
        let copy_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[hzb] copy layout"),
            bind_group_layouts: &[Some(&copy_bgl)],
            immediate_size: 0,
        });
        self.hzb_copy_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[hzb] copy pipeline"),
            layout: Some(&copy_layout),
            module: &hzb_module,
            entry_point: Some("cs_copy_depth"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }));
        self.hzb_copy_bgl = Some(copy_bgl);

        let ds_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[hzb] downsample layout"),
            bind_group_layouts: &[Some(&ds_bgl)],
            immediate_size: 0,
        });
        self.hzb_downsample_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[hzb] downsample pipeline"),
            layout: Some(&ds_layout),
            module: &hzb_module,
            entry_point: Some("cs_downsample"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }));
        self.hzb_downsample_bgl = Some(ds_bgl);

        // hzb occlusion cull bgl: group 2
        let cull_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[hzb] cull bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let cull_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[hzb] cull layout"),
            bind_group_layouts: &[Some(&cull_bgl)],
            immediate_size: 0,
        });
        self.hzb_cull_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[hzb] cull pipeline"),
            layout: Some(&cull_layout),
            module: &hzb_module,
            entry_point: Some("cs_cull_hzb"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }));
        self.hzb_cull_bgl = Some(cull_bgl);
    }
    /// grow HZB per-entity occlusion buffers if needed.
    pub(crate) fn ensure_hzb_cull_buffers(&mut self, entity_count: usize) {
        let cap = entity_count.next_power_of_two().max(256);
        let needs = self.hzb_occ_buf
            .as_ref()
            .is_none_or(|b| b.size() < (cap * 4) as u64);
        if !needs { return; }

        self.hzb_occ_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] occ flags buf"),
            size: (cap * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.hzb_occ_staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] occ staging"),
            size: (cap * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));
        self.hzb_cull_aabb_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] cull aabb buf"),
            size: (cap * 32) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        // hzb cull params: mat4 (64) + vec2 viewport (8) + u32 mip_count (4) + u32 count (4) = 80 bytes
        self.hzb_cull_params_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] cull params buf"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.hzb_occ_flags.resize(cap, 0);
    }
    pub(crate) fn ensure_contact_shadow_resources(&mut self, width: u32, height: u32, inv_proj: &Mat4) {
        let qw = (width  / 2).max(1);
        let qh = (height / 2).max(1);
        let needs_tex = self.contact_shadow_tex.is_none();
        if needs_tex {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("[contact shadow] tex"),
                size: wgpu::Extent3d { width: qw, height: qh, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.contact_shadow_view = Some(tex.create_view(&wgpu::TextureViewDescriptor::default()));
            self.contact_shadow_tex  = Some(tex);
            self.composite_bg_dirty  = true;
        }

        if self.contact_shadow_bgl.is_none() {
            let sampler_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            };
            let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            };
            let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(CONTACT_SHADOW_PARAMS_SIZE),
                },
                count: None,
            };
            let bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("[contact shadow] bgl"),
                entries: &[uniform_entry(0), tex_entry(1), sampler_entry(2)],
            });
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("[contact shadow] layout"),
                bind_group_layouts: &[Some(&bgl)],
                immediate_size: 0,
            });
            let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("[contact shadow] shader"),
                source: shader_source!(CONTACT_SHADOW_SHADER_SRC, "contact_shadow.spv"),
            });
            self.contact_shadow_pipeline = Some(self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("[contact shadow] pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader, entry_point: Some("vs_main"),
                    buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader, entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                cache: None,
                multiview_mask: None,
            }));
            self.contact_shadow_bgl = Some(bgl);
            let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[contact shadow] params"), size: CONTACT_SHADOW_PARAMS_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.contact_shadow_params_buf = Some(params_buf);
        }

        // upload params: inv_proj(64) + light_dir_vs(12) + step_count(4) + step_size(4) + w(4) + h(4) + pad(4) = 96
        let mut data = [0f32; 24];
        let inv_proj_cols = inv_proj.to_cols_array();
        data[..16].copy_from_slice(&inv_proj_cols);
        data[16] = 0.0;  // light_dir_vs.x (placeholder — set each frame)
        data[17] = -1.0; // light_dir_vs.y (pointing down as default)
        data[18] = 0.0;  // light_dir_vs.z
        data[19] = f32::from_bits(8u32); // step_count
        data[20] = 0.08; // step_size
        data[21] = width  as f32;
        data[22] = height as f32;
        if let Some(buf) = self.contact_shadow_params_buf.as_ref() {
            self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&data));
        }
    }
    pub(crate) fn ensure_motion_vector_resources(&mut self, width: u32, height: u32) {
        if self.motion_vec_tex.is_none() {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("[motion vec] tex"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.motion_vec_view = Some(tex.create_view(&wgpu::TextureViewDescriptor::default()));
            self.motion_vec_tex  = Some(tex);
        }

        if self.motion_vec_bgl.is_none() {
            let bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("[motion vec] bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(MOTION_VECTOR_PARAMS_SIZE),
                        }, count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        }, count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("[motion vec] layout"),
                bind_group_layouts: &[Some(&bgl)],
                immediate_size: 0,
            });
            let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("[motion vec] shader"),
                source: shader_source!(MOTION_VECTOR_SHADER_SRC, "motion_vector.spv"),
            });
            self.motion_vec_pipeline = Some(self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("[motion vec] pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader, entry_point: Some("vs_main"),
                    buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader, entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rg16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                cache: None,
                multiview_mask: None,
            }));
            self.motion_vec_bgl = Some(bgl);
            let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[motion vec] params"), size: MOTION_VECTOR_PARAMS_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.motion_vec_params_buf = Some(params_buf);
        }
    }
    pub(crate) fn ensure_detail_sprite_resources(&mut self) {
        if self.detail_sprite_bgl.is_some() { return; }

        let storage_ro = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding, visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false, min_binding_size: None,
            }, count: None,
        };
        let storage_rw = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding, visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false, min_binding_size: None,
            }, count: None,
        };
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding, visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2, multisampled: false,
            }, count: None,
        };
        let smp_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding, visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let uniform_entry_compute = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding, visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false, min_binding_size: None,
            }, count: None,
        };

        let compute_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[detail sprite] compute bgl"),
            entries: &[uniform_entry_compute(0), tex_entry(1), smp_entry(2), storage_rw(3), storage_rw(4)],
        });
        let compute_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[detail sprite] compute layout"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });
        let compute_shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[detail sprite] compute shader"),
            source: shader_source!(DETAIL_SPRITE_SHADER_SRC, "detail_sprite.spv"),
        });
        self.detail_sprite_compute_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[detail sprite] compute pipeline"),
            layout: Some(&compute_layout),
            module: &compute_shader,
            entry_point: Some("cs_generate_instances"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }));
        self.detail_sprite_compute_bgl = Some(compute_bgl);

        // render pipeline — binds globals + texture atlas + instance buffer
        let render_bgl_0 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[detail sprite] render bgl0"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2, multisampled: false,
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                storage_ro(3),
            ],
        });
        let render_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[detail sprite] render layout"),
            bind_group_layouts: &[Some(&render_bgl_0)],
            immediate_size: 0,
        });
        let render_shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[detail sprite] render shader"),
            source: shader_source!(DETAIL_SPRITE_SHADER_SRC, "detail_sprite.spv"),
        });
        self.detail_sprite_pipeline = Some(self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[detail sprite] pipeline"),
            layout: Some(&render_layout),
            vertex: wgpu::VertexState {
                module: &render_shader, entry_point: Some("vs_sprite"),
                buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader, entry_point: Some("fs_sprite"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.hdr_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: self.msaa_samples, ..Default::default() },
            cache: None,
            multiview_mask: None,
        }));
        self.detail_sprite_bgl = Some(render_bgl_0);
    }
    pub(crate) fn ensure_lod_select_resources(&mut self, entity_count: usize) {
        if entity_count == 0 { return; }
        let cap = entity_count.next_power_of_two().max(256);

        let needs_rebuild = self.lod_indices_buf.is_none() || cap > self.cull_entity_capacity;
        if needs_rebuild {
            self.lod_indices_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[lod] indices buf"),
                size: (cap * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.lod_indices_staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[lod] indices staging"),
                size: (cap * 4) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
        }

        if self.lod_select_bgl.is_none() {
            let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding, visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            };
            let storage_ro = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding, visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            };
            let storage_rw = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding, visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            };
            let bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("[lod select] bgl"),
                entries: &[uniform_entry(0), storage_ro(1), storage_rw(2)],
            });
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("[lod select] layout"),
                bind_group_layouts: &[Some(&bgl)],
                immediate_size: 0,
            });
            // LOD selection compute shader (matches the external-file convention
            // used by every other shader in this crate).
            let lod_wgsl = include_str!("lod_select.wgsl");
            let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("[lod select] shader"),
                source: wgpu::ShaderSource::Wgsl(lod_wgsl.into()),
            });
            self.lod_select_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("[lod select] pipeline"),
                layout: Some(&layout),
                module: &shader,
                entry_point: Some("cs_lod_select"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            }));
            self.lod_select_bgl = Some(bgl);
            self.lod_params_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[lod] params"), size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }
    pub(crate) fn make_depth_view(device: &wgpu::Device, width: u32, height: u32, sample_count: u32) -> wgpu::TextureView {
        // non-MSAA depth also gets TEXTURE_BINDING so GTAO can sample it
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | if sample_count == 1 { wgpu::TextureUsages::TEXTURE_BINDING } else { wgpu::TextureUsages::empty() };
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("[depth] attachment"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    }
    pub(crate) fn make_msaa_color_view(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Option<wgpu::TextureView> {
        if sample_count <= 1 {
            return None;
        }
        Some(
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("[msaa] color attachment"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default()),
        )
    }
    pub(crate) fn make_hdr_texture(device: &wgpu::Device, width: u32, height: u32, hdr_format: wgpu::TextureFormat) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[hdr] color attachment"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: hdr_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }
    /// creates the bloom mip chain texture, per-mip views, and per-step bind groups.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub(crate) fn build_bloom_resources(
        device: &wgpu::Device,
        hdr_texture: &wgpu::Texture,
        params_buf: &wgpu::Buffer,
        bgl: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        width: u32,
        height: u32,
        mip_count: usize,
        hdr_format: wgpu::TextureFormat,
    ) -> (
        Vec<wgpu::TextureView>,
        Vec<(u32, u32)>,
        Vec<wgpu::BindGroup>,
        Vec<wgpu::BindGroup>,
    ) {
        let actual_mips = mip_count.clamp(1, MAX_BLOOM_MIPS);

        // one bloom texture with mip_count mip levels
        let bloom_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[bloom] mip chain"),
            size: wgpu::Extent3d { width: width / 2, height: height / 2, depth_or_array_layers: 1 },
            mip_level_count: actual_mips as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: hdr_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let mut mip_views = Vec::with_capacity(actual_mips);
        let mut mip_sizes = Vec::with_capacity(actual_mips);
        let mut w = width / 2;
        let mut h = height / 2;
        for i in 0..actual_mips {
            mip_views.push(bloom_tex.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: i as u32,
                mip_level_count: Some(1),
                ..Default::default()
            }));
            mip_sizes.push((w.max(1), h.max(1)));
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }

        // hdr view (full texture, mip 0) for the first downsample source
        let hdr_full_view = hdr_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // downsample bind groups: step 0 reads hdr, step i reads bloom mip i-1
        let mut ds_bgs = Vec::with_capacity(actual_mips);
        for i in 0..actual_mips {
            let src_view = if i == 0 { &hdr_full_view } else { &mip_views[i - 1] };
            let (src_w, src_h) = if i == 0 { (width, height) } else { mip_sizes[i - 1] };
            let _ = (src_w, src_h);  // sizes used at frame time for param upload
            ds_bgs.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("[bloom] downsample bg"),
                layout: bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: params_buf, offset: (i * UNIFORM_STRIDE as usize) as u64, size: wgpu::BufferSize::new(BLOOM_PARAMS_SIZE) }) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            }));
        }

        // upsample bind groups: render pass j reads mip[n-1-j] and writes to mip[n-2-j]
        let mut us_bgs = Vec::with_capacity(actual_mips.saturating_sub(1));
        for i in 0..actual_mips.saturating_sub(1) {
            let src_view = &mip_views[actual_mips - 1 - i];
            us_bgs.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("[bloom] upsample bg"),
                layout: bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: params_buf, offset: ((actual_mips + i) * UNIFORM_STRIDE as usize) as u64, size: wgpu::BufferSize::new(BLOOM_PARAMS_SIZE) }) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            }));
        }

        (mip_views, mip_sizes, ds_bgs, us_bgs)
    }
    pub(crate) fn make_entity_buf(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[draw] entity storage buffer"),
            size: (capacity * UNIFORM_STRIDE as usize) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    pub(crate) fn make_entity_bg(
        device: &wgpu::Device,
        mesh_bgl: &wgpu::BindGroupLayout,
        entity_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[mesh] entity bg"),
            layout: mesh_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: entity_buf.as_entire_binding(),
            }],
        })
    }
    /// create or recreate FSR resources for the given render/display dimensions.
    /// idempotent: no-op if dimensions haven't changed and resources exist.
    pub(crate) fn ensure_upscale_resources(&mut self, render_w: u32, render_h: u32, display_w: u32, display_h: u32) {
        let needs_tex_rebuild = self.fsr_ldr_texture.as_ref().map(|t| {
            let s = t.size(); s.width != render_w || s.height != render_h
        }).unwrap_or(true)
        || self.fsr_mid_texture.as_ref().map(|t| {
            let s = t.size(); s.width != display_w || s.height != display_h
        }).unwrap_or(true);

        let format = self.surface_config.format;

        if needs_tex_rebuild {
            let make_tex = |device: &wgpu::Device, w: u32, h: u32, label: &'static str| {
                let t = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1, sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let v = t.create_view(&wgpu::TextureViewDescriptor::default());
                (t, v)
            };
            let (ldr_tex, ldr_view) = make_tex(&self.device, render_w, render_h, "[upscale] ldr");
            let (mid_tex, mid_view) = make_tex(&self.device, display_w, display_h, "[upscale] mid");
            self.fsr_ldr_texture = Some(ldr_tex);
            self.fsr_ldr_view    = Some(ldr_view);
            self.fsr_mid_texture = Some(mid_tex);
            self.fsr_mid_view    = Some(mid_view);
        }

        let params_buf = self.fsr_params_buf.get_or_insert_with(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[upscale] params"),
                size: FSR_PARAMS_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        let bgl = self.fsr_bgl.get_or_insert_with(|| {
            self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("[upscale] bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(FSR_PARAMS_SIZE) },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            })
        });

        // rebuild bind groups whenever textures are rebuilt
        if needs_tex_rebuild {
            let ldr_view = self.fsr_ldr_view.as_ref().unwrap();
            let mid_view = self.fsr_mid_view.as_ref().unwrap();
            let make_bg = |device: &wgpu::Device, bgl: &wgpu::BindGroupLayout, params: &wgpu::Buffer, view: &wgpu::TextureView, sampler: &wgpu::Sampler, label: &'static str| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                    ],
                })
            };
            self.fsr_easu_bg = Some(make_bg(&self.device, bgl, params_buf, ldr_view, &self.post_sampler, "[upscale] easu/single bg"));
            self.fsr_rcas_bg = Some(make_bg(&self.device, bgl, params_buf, mid_view, &self.post_sampler, "[upscale] rcas bg"));
        }

        // create all upscale pipelines once (shared layout, different entry points)
        if self.fsr_easu_pipeline.is_none() {
            let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("[upscale] shader"),
                source: shader_source!(UPSCALE_SHADER_SRC, "upscale.spv"),
            });
            let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("[upscale] pipeline layout"),
                bind_group_layouts: &[Some(bgl)],
                immediate_size: 0,
            });
            let make_pp = |entry: &'static str, label: &'static str| -> wgpu::RenderPipeline {
                self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader, entry_point: Some("vs_upscale"),
                        buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader, entry_point: Some(entry),
                        targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    cache: None,
                    multiview_mask: None,
                })
            };
            self.upscale_nearest_pipeline = Some(make_pp("fs_nearest", "[upscale] nearest"));
            self.upscale_linear_pipeline  = Some(make_pp("fs_linear",  "[upscale] linear"));
            self.upscale_lanczos_pipeline = Some(make_pp("fs_lanczos", "[upscale] lanczos"));
            self.upscale_bicubic_pipeline = Some(make_pp("fs_bicubic", "[upscale] bicubic"));
            self.fsr_easu_pipeline        = Some(make_pp("fs_easu",    "[upscale] fsr easu"));
            self.fsr_rcas_pipeline        = Some(make_pp("fs_rcas",    "[upscale] fsr rcas"));
        }
    }
}
