  # bsp and lightmap wiring

  these are the two missing wires between existing systems and the renderer.
  both are self-contained changes to `lunar-render-3d`. no new crates needed.

  ---

  ## task 1 — wire bsp pvs into the renderer gather pass

  ### what it does

  when `BspLevel` is loaded, the gather pass should filter ECS entities to only
  those whose `Area` id is reachable from the camera leaf's PVS, replacing the
  ECS BFS portal traversal for level geometry.

  ### acceptance criteria

  - if `BspLevel::is_loaded()` returns false, the gather pass is unchanged (falls
    through to existing `VisibleAreas` + `BvhVisible` filtering)
  - if loaded, the gather pass calls `camera_leaf(cam_pos)` and
    `visible_leaves(camera_leaf)` to build a `HashSet<u32>` of visible area ids
    for this frame
  - entities with an `Area` component are only drawn if their area id is in that set
  - entities without an `Area` component are always drawn (same as current behaviour)
  - the `VisibleAreas` resource is updated to reflect the PVS result so portal-aware
    game code (e.g. AI line-of-sight queries) still reads a correct value

  ### where to change

  **`crates/lunar-render-3d/src/lib.rs`**

  in the `render()` call, after reading `cam_pos` and before the gather loop:

  ```rust
  // read BspLevel from world; build area visibility set for this frame
  let bsp_visible_areas: Option<std::collections::HashSet<u32>> = world
      .get_resource::<lunar_bsp::BspLevel>()
      .filter(|level| level.is_loaded())
      .map(|level| {
          let leaf = level.camera_leaf(cam_pos);
          let visible = level.visible_leaves(leaf);
          let area_map = level.area_map();
          let mut areas = std::collections::HashSet::new();
          for leaf_idx in visible {
              // find area_id for this leaf (area_map is sorted by leaf_index)
              if let Some(&(_, area_id)) = area_map.iter().find(|(li, _)| *li == leaf_idx as u32) {
                  areas.insert(area_id);
              }
          }
          areas
      });
  ```

  in the gather filter closure (currently `vis.0 && (...frustum_visible.contains...)`):

  ```rust
  .filter(|(entity, _, _, _, vis, aabb, _, _)| {
      if !vis.0 { return false; }
      // BSP PVS area filter (only when a compiled level is loaded)
      // area component is fetched inline — add Area to the query tuple
      if let Some(ref visible_areas) = bsp_visible_areas {
          if let Some(area) = entity_area { // Area from query
              if !visible_areas.contains(&area.0) { return false; }
          }
      }
      // existing frustum / BVH filter
      aabb.is_none() || self.frustum_visible.contains(entity)
  })
  ```

  add `Option<&Area>` to the gather query. `Area` is in `lunar_bsp::portal`.

  after building `bsp_visible_areas`, write it back to the `VisibleAreas` resource:

  ```rust
  if let Some(ref areas) = bsp_visible_areas {
      if let Some(mut vis_res) = world.get_resource_mut::<lunar_bsp::VisibleAreas>() {
          vis_res.area_ids.clone_from(areas);
          vis_res.active = true;
      }
  }
  ```

  **`crates/lunar/Cargo.toml`**

  `lunar-render-3d` already depends on `lunar-3d`; add `lunar-bsp` as a dependency
  of `lunar-render-3d` if it isn't already.

  ### expected impact

  indoor scenes: 80–95% reduction in draw calls submitted to the GPU. everything
  downstream (shadow cascade cost, lighting, overdraft) scales down with it.

  ---

  ## task 2 — wire lightmaps into the pbr shader

  ### what it does

  entities with a `Lightmap` component get their diffuse lighting replaced by a
  precomputed baked texture, skipping runtime directional light evaluation for
  those fragments. dynamic geometry (characters, projectiles) continues using
  full PBR.

  ### acceptance criteria

  - a mesh with `Lightmap { texture, intensity }` samples `uv_lightmap` and uses
    the result instead of the directional shadow + diffuse term
  - ambient and point lights still contribute (lightmap replaces only the
    directional baked term)
  - meshes without a `Lightmap` component behave exactly as before
  - the renderer does not crash when some entities have lightmaps and others do not

  ### where to change

  #### shader — `crates/lunar-render-3d/src/shader.wgsl`

  add a lightmap texture and sampler at group 4 (new bind group):

  ```wgsl
  @group(4) @binding(0) var lightmap_tex: texture_2d<f32>;
  @group(4) @binding(1) var lightmap_sampler: sampler;
  ```

  add `has_lightmap: u32` to `MaterialUniforms` (group 1, binding 0):

  ```wgsl
  struct MaterialUniforms {
      base_color:   vec4<f32>,
      metallic:     f32,
      roughness:    f32,
      flags:        u32,
      has_lightmap: u32,   // new field
  };
  ```

  in the fragment shader, after computing PBR diffuse:

  ```wgsl
  if (material.has_lightmap != 0u) {
      // replace baked-light directional contribution with lightmap sample
      let lm = textureSample(lightmap_tex, lightmap_sampler, in.uv_lightmap).rgb;
      // lm already encodes irradiance; use it as the diffuse term
      out_color = vec4(lm * albedo.rgb + point_contribution + ambient_term, albedo.a);
  } else {
      // existing full PBR path
      out_color = vec4(existing_pbr_result, albedo.a);
  }
  ```

  #### bind group layout — `crates/lunar-render-3d/src/lib.rs`

  add a 5th bind group layout (`lightmap_bgl`):

  ```rust
  let lightmap_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("lightmap_bgl"),
      entries: &[
          // texture
          wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
              ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true },
                  view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
          // sampler
          wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
              ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
      ],
  });
  ```

  update the pipeline layout to include `lightmap_bgl`:

  ```rust
  bind_group_layouts: &[
      Some(&globals_bgl), Some(&material_bgl), Some(&mesh_bgl),
      Some(&lights_bgl), Some(&lightmap_bgl),
  ],
  ```

  create a **fallback lightmap bind group** (1×1 white texture) used for all
  entities without a lightmap:

  ```rust
  let white_px = device.create_texture(&wgpu::TextureDescriptor { /* 1×1 R8G8B8A8 */ });
  // write [255,255,255,255] via queue.write_texture
  let white_view = white_px.create_view(&Default::default());
  let lightmap_fallback_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
      layout: &lightmap_bgl,
      entries: &[
          wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&white_view) },
          wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&linear_sampler) },
      ],
      label: Some("lightmap_fallback_bg"),
  });
  ```

  #### draw loop

  in the gather pass, also query `Option<&Lightmap>` from entities.
  in the draw loop, for each entity:

  ```rust
  // resolve lightmap bind group for this entity
  let lightmap_bg = entity_lightmap
      .and_then(|lm| self.texture_gpu.get(&lm.texture.id()))
      .map(|gpu_tex| build_lightmap_bg(&device, &lightmap_bgl, &gpu_tex.view, &linear_sampler))
      .unwrap_or(&self.lightmap_fallback_bg);
  pass.set_bind_group(4, lightmap_bg, &[]);
  ```

  cache per-texture lightmap bind groups in a `HashMap<u64, wgpu::BindGroup>` on
  the renderer struct (keyed by texture asset id) so they are built once on first
  use and reused thereafter.

  also update `has_lightmap` in the material uniform push constant or uniform buffer
  for each draw call, or include it in the per-entity path.

  #### material uniforms

  `MaterialUniforms` is currently a uniform buffer shared across all draws. with
  per-entity `has_lightmap`, the simplest approach is to make `has_lightmap` part
  of the per-mesh push constants (group 2) rather than the material uniform, since
  it varies per entity even for the same material. alternatively, accept a small
  material bind group rebuild when the lightmap flag differs from the previous draw.

  ### expected impact

  static geometry stops paying for directional light evaluation per fragment.
  on a level where 80% of visible geometry is static, the lighting pass cost drops
  by ~80% for lit fragments. combines with BSP wiring for maximum gain.

  ---

  ## implementation order

  do BSP wiring first — it's a pure gather-pass filter with no shader changes and
  the payoff (80–95% draw call reduction) makes every subsequent task faster to test.
  lightmap wiring second — it changes the shader and pipeline layout, which requires
  more careful testing.
