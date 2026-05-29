// billboard particle renderer.
//
// one instance per live particle; six vertices per instance form a camera-aligned quad.
// reads particle SoA from a storage buffer (mid+ tier compute path) OR from a plain
// vertex buffer (low tier CPU path — same struct layout in both cases).

struct Globals {
    view_proj:    mat4x4<f32>,
    cam_pos:      vec3<f32>,
    elapsed_secs: f32,
    delta_secs:   f32,
    _pad0: f32, _pad1: f32, _pad2: f32,
}

struct Particle {
    position:     vec3<f32>,
    lifetime:     f32,
    velocity:     vec3<f32>,
    max_lifetime: f32,
    color_start:  vec4<f32>,
    color_end:    vec4<f32>,
    size_start:   f32,
    size_end:     f32,
    _pad0:        f32,
    _pad1:        f32,
}

@group(0) @binding(0) var<uniform>            globals:   Globals;
@group(0) @binding(1) var<storage, read>      particles: array<Particle>;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       color:    vec4<f32>,
    @location(1)       uv:       vec2<f32>,
}

// six vertex positions for a unit quad (two CCW triangles)
fn quad_offset(vi: u32) -> vec2<f32> {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5),
        vec2<f32>( 0.5, -0.5),
        vec2<f32>( 0.5,  0.5),
        vec2<f32>(-0.5, -0.5),
        vec2<f32>( 0.5,  0.5),
        vec2<f32>(-0.5,  0.5),
    );
    return corners[vi % 6u];
}
fn quad_uv(vi: u32) -> vec2<f32> {
    let uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 0.0),
    );
    return uvs[vi % 6u];
}

@vertex
fn vs_main(
    @builtin(vertex_index)   vi: u32,
    @builtin(instance_index) ii: u32,
) -> VertOut {
    var out: VertOut;
    let p = particles[ii];

    // dead particle: collapse to degenerate quad at clip-w=0 (culled)
    if p.lifetime <= 0.0 {
        out.clip_pos = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        out.color    = vec4<f32>(0.0);
        out.uv       = vec2<f32>(0.0);
        return out;
    }

    let t = 1.0 - clamp(p.lifetime / p.max_lifetime, 0.0, 1.0);
    let size = mix(p.size_start, p.size_end, t);
    let color = mix(p.color_start, p.color_end, t);

    // camera-aligned billboard: derive right/up from view matrix columns
    let right = vec3<f32>(globals.view_proj[0][0], globals.view_proj[1][0], globals.view_proj[2][0]);
    let up    = vec3<f32>(globals.view_proj[0][1], globals.view_proj[1][1], globals.view_proj[2][1]);
    let offset = quad_offset(vi);
    let world_pos = p.position + (right * offset.x + up * offset.y) * size;

    out.clip_pos = globals.view_proj * vec4<f32>(world_pos, 1.0);
    out.color    = color;
    out.uv       = quad_uv(vi);
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // soft circular mask: discard corners for a round particle
    let d = length(in.uv - vec2<f32>(0.5)) * 2.0;
    if d > 1.0 { discard; }
    let alpha = in.color.a * (1.0 - d * d);
    return vec4<f32>(in.color.rgb, alpha);
}
