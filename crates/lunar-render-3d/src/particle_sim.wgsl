// GPU particle simulation — compute shader.
//
// dispatched as ceil(particle_cap / 64) workgroups of 64 threads.
// each thread owns one particle slot. dead particles (lifetime <= 0) are re-emitted
// by the emitter system (CPU writes emitter params and seeds into spawn_buf each frame
// before dispatch). alive particles integrate velocity + gravity.
//
// requires COMPUTE_SHADERS (mid+ tier). on low tier the CPU simulates and writes
// directly to the render vertex buffer, bypassing this shader.

struct Particle {
    position:     vec3<f32>,
    lifetime:     f32,        // remaining life; <= 0 = dead
    velocity:     vec3<f32>,
    max_lifetime: f32,
    color_start:  vec4<f32>,
    color_end:    vec4<f32>,
    size_start:   f32,
    size_end:     f32,
    _pad0:        f32,
    _pad1:        f32,
}

struct SimParams {
    delta_time:  f32,
    gravity:     f32,
    alive_count: u32,  // filled by CPU, read-only in compute
    _pad:        f32,
}

@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(1) var<uniform>             params:    SimParams;

@compute @workgroup_size(64)
fn cs_simulate(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= params.alive_count { return; }

    var p = particles[index];
    if p.lifetime <= 0.0 { return; }

    p.velocity.y -= params.gravity * params.delta_time;
    p.position   += p.velocity * params.delta_time;
    p.lifetime   -= params.delta_time;

    particles[index] = p;
}
