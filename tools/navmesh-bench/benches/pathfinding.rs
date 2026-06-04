// benchmarks navmesh (pure rust) vs rerecast (rust recast port) for path queries
// and dodgy_3d (orca) for crowd avoidance, on geometry representative of a
// simplified dust2 floor plan.
//
// metrics:
//   navmesh:   construction time, single path query, 12 concurrent queries
//   rerecast:  full bake pipeline (rasterise → compact → regions → contours → polymesh → detail)
//   dodgy_3d:  12-agent orca step
//   accuracy:  path verified to detour around a known obstacle

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use navmesh::{NavMesh, NavPathMode, NavQuery, NavTriangle, NavVec3};
use std::borrow::Cow;

// ── geometry helpers ──────────────────────────────────────────────────────────

fn push_floor_quad(
    verts: &mut Vec<NavVec3>,
    tris: &mut Vec<NavTriangle>,
    x0: f32, z0: f32,
    x1: f32, z1: f32,
) {
    let base = verts.len() as u32;
    verts.extend([
        NavVec3::new(x0, 0.0, z0),
        NavVec3::new(x1, 0.0, z0),
        NavVec3::new(x1, 0.0, z1),
        NavVec3::new(x0, 0.0, z1),
    ]);
    tris.extend([
        NavTriangle::from([base, base + 1, base + 2]),
        NavTriangle::from([base, base + 2, base + 3]),
    ]);
}

/// dust2 floor plan approximation (flat, y=0, metres)
///
///   [t-spawn]──[long A]──[A site]
///                  │
///                 [mid]
///                  │
///   [ct-spawn]──[B site]──[b tunnel]──[b-spawn]
fn dust2_navmesh() -> NavMesh {
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    push_floor_quad(&mut verts, &mut tris,  0.0,  0.0, 15.0, 15.0); // T spawn
    push_floor_quad(&mut verts, &mut tris, 15.0,  5.0, 45.0, 10.0); // long A
    push_floor_quad(&mut verts, &mut tris, 45.0,  0.0, 60.0, 20.0); // A site
    push_floor_quad(&mut verts, &mut tris, 20.0, 10.0, 30.0, 35.0); // mid
    push_floor_quad(&mut verts, &mut tris, 20.0, 35.0, 30.0, 45.0); // mid-to-B link
    push_floor_quad(&mut verts, &mut tris,  5.0, 40.0, 25.0, 45.0); // B tunnel
    push_floor_quad(&mut verts, &mut tris,  0.0, 45.0, 20.0, 60.0); // B site
    push_floor_quad(&mut verts, &mut tris,  0.0, 60.0, 15.0, 75.0); // CT spawn
    NavMesh::new(verts, tris).expect("dust2 navmesh construction failed")
}

/// room split by a gap — correct path must detour over the top section.
fn obstacle_navmesh() -> NavMesh {
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    push_floor_quad(&mut verts, &mut tris,  0.0, 0.0, 10.0, 5.0); // left half
    push_floor_quad(&mut verts, &mut tris, 15.0, 0.0, 25.0, 5.0); // right half
    push_floor_quad(&mut verts, &mut tris,  8.0, 5.0, 17.0, 10.0); // detour above
    NavMesh::new(verts, tris).expect("obstacle navmesh construction failed")
}

// ── navmesh benchmarks ────────────────────────────────────────────────────────

fn bench_navmesh_construction(c: &mut Criterion) {
    c.bench_function("navmesh/construction", |b| {
        b.iter(|| {
            let mut verts = Vec::new();
            let mut tris = Vec::new();
            push_floor_quad(&mut verts, &mut tris,  0.0,  0.0, 15.0, 15.0);
            push_floor_quad(&mut verts, &mut tris, 15.0,  5.0, 45.0, 10.0);
            push_floor_quad(&mut verts, &mut tris, 45.0,  0.0, 60.0, 20.0);
            push_floor_quad(&mut verts, &mut tris, 20.0, 10.0, 30.0, 35.0);
            push_floor_quad(&mut verts, &mut tris, 20.0, 35.0, 30.0, 45.0);
            push_floor_quad(&mut verts, &mut tris,  5.0, 40.0, 25.0, 45.0);
            push_floor_quad(&mut verts, &mut tris,  0.0, 45.0, 20.0, 60.0);
            push_floor_quad(&mut verts, &mut tris,  0.0, 60.0, 15.0, 75.0);
            black_box(NavMesh::new(verts, tris).unwrap())
        })
    });
}

fn bench_navmesh_single_query(c: &mut Criterion) {
    let mesh = dust2_navmesh();
    let from = NavVec3::new(7.0, 0.0, 7.0);   // T spawn centre
    let to   = NavVec3::new(7.0, 0.0, 67.0);  // CT spawn centre

    c.bench_function("navmesh/query_single", |b| {
        b.iter(|| {
            black_box(mesh.find_path(
                black_box(from),
                black_box(to),
                NavQuery::Accuracy,
                NavPathMode::Accuracy,
            ))
        })
    });
}

fn bench_navmesh_12_queries(c: &mut Criterion) {
    let mesh = dust2_navmesh();
    let queries: &[(NavVec3, NavVec3)] = &[
        (NavVec3::new( 3.0, 0.0,  3.0), NavVec3::new(52.0, 0.0, 10.0)), // T → A site
        (NavVec3::new( 5.0, 0.0,  7.0), NavVec3::new(52.0, 0.0, 15.0)),
        (NavVec3::new( 8.0, 0.0,  5.0), NavVec3::new(10.0, 0.0, 52.0)), // T → B site
        (NavVec3::new(10.0, 0.0,  3.0), NavVec3::new( 7.0, 0.0, 52.0)),
        (NavVec3::new( 4.0, 0.0, 10.0), NavVec3::new(25.0, 0.0, 20.0)), // T → mid
        (NavVec3::new( 7.0, 0.0,  8.0), NavVec3::new(52.0, 0.0,  5.0)),
        (NavVec3::new( 7.0, 0.0, 67.0), NavVec3::new(52.0, 0.0, 10.0)), // CT → A site
        (NavVec3::new( 5.0, 0.0, 70.0), NavVec3::new(10.0, 0.0, 52.0)), // CT → B site
        (NavVec3::new(10.0, 0.0, 65.0), NavVec3::new(25.0, 0.0, 20.0)),
        (NavVec3::new( 8.0, 0.0, 68.0), NavVec3::new( 5.0, 0.0,  5.0)),
        (NavVec3::new( 3.0, 0.0, 63.0), NavVec3::new(52.0, 0.0, 15.0)),
        (NavVec3::new( 6.0, 0.0, 72.0), NavVec3::new( 8.0, 0.0,  8.0)),
    ];

    c.bench_function("navmesh/query_12_agents", |b| {
        b.iter(|| {
            for &(from, to) in queries {
                black_box(mesh.find_path(from, to, NavQuery::Accuracy, NavPathMode::Accuracy));
            }
        })
    });
}

// ── rerecast bake benchmark ───────────────────────────────────────────────────

fn bench_rerecast_bake(c: &mut Criterion) {
    use glam::{UVec3, Vec3A};
    use rerecast::{
        Aabb3d, AreaType, BuildContoursFlags, ConfigBuilder, DetailNavmesh, HeightfieldBuilder,
        TriMesh,
    };

    let mut trimesh = TriMesh { vertices: Vec::new(), indices: Vec::new(), area_types: Vec::new() };

    {
        let mut push = |x0: f32, z0: f32, x1: f32, z1: f32| {
            let base = trimesh.vertices.len() as u32;
            trimesh.vertices.extend([
                Vec3A::new(x0, 0.0, z0), Vec3A::new(x1, 0.0, z0),
                Vec3A::new(x1, 0.0, z1), Vec3A::new(x0, 0.0, z1),
            ]);
            trimesh.indices.extend([
                UVec3::new(base, base + 1, base + 2),
                UVec3::new(base, base + 2, base + 3),
            ]);
            trimesh.area_types.extend([AreaType::DEFAULT_WALKABLE; 2]);
        };
        push( 0.0,  0.0, 15.0, 15.0);
        push(15.0,  5.0, 45.0, 10.0);
        push(45.0,  0.0, 60.0, 20.0);
        push(20.0, 10.0, 30.0, 35.0);
        push(20.0, 35.0, 30.0, 45.0);
        push( 5.0, 40.0, 25.0, 45.0);
        push( 0.0, 45.0, 20.0, 60.0);
        push( 0.0, 60.0, 15.0, 75.0);
    }

    let aabb = trimesh.compute_aabb().expect("trimesh has no vertices");
    let config = ConfigBuilder {
        aabb,
        ..ConfigBuilder::default()
    }.build();

    c.bench_function("rerecast/bake_pipeline", |b| {
        b.iter(|| {
            let mut hf = HeightfieldBuilder {
                aabb: config.aabb,
                cell_size: config.cell_size,
                cell_height: config.cell_height,
            }.build().unwrap();

            hf.populate_from_trimesh(trimesh.clone(), config.walkable_height, config.walkable_climb)
                .unwrap();

            let mut chf = hf.into_compact(config.walkable_height, config.walkable_climb).unwrap();

            chf.erode_walkable_area(config.walkable_radius);
            chf.build_distance_field();
            chf.build_regions(config.border_size, config.min_region_area, config.merge_region_area)
                .unwrap();

            let contours = chf.build_contours(
                config.max_simplification_error,
                config.max_edge_len,
                BuildContoursFlags::default(),
            );

            let poly = contours
                .into_polygon_mesh(config.max_vertices_per_polygon)
                .unwrap();

            black_box(DetailNavmesh::new(
                &poly, &chf,
                config.detail_sample_dist,
                config.detail_sample_max_error,
            ).unwrap())
        })
    });
}

// ── dodgy_3d crowd avoidance benchmark ───────────────────────────────────────

fn bench_dodgy_crowd(c: &mut Criterion) {
    use dodgy_3d::{Agent, AvoidanceOptions, Vec3};

    // 12 agents in a circle, all crossing to the opposite side simultaneously
    let positions: Vec<Vec3> = (0..12)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / 12.0;
            Vec3::new(angle.cos() * 10.0, 0.0, angle.sin() * 10.0)
        })
        .collect();

    let agents: Vec<Agent> = positions
        .iter()
        .map(|p| Agent {
            position: *p,
            velocity: Vec3::ZERO,
            radius: 0.4,
            avoidance_responsibility: 0.5,
        })
        .collect();

    let preferred: Vec<Vec3> = positions
        .iter()
        .map(|p| (*p * -1.0).normalize() * 5.0)
        .collect();

    let options = AvoidanceOptions { time_horizon: 2.0 };

    c.bench_function("dodgy_3d/orca_12_agents", |b| {
        b.iter(|| {
            for (i, agent) in agents.iter().enumerate() {
                let neighbours: Vec<Cow<Agent>> = agents
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .map(|(_, a)| Cow::Borrowed(a))
                    .collect();
                black_box(agent.compute_avoiding_velocity(
                    &neighbours,
                    black_box(preferred[i]),
                    5.0,
                    10.0,
                    &options,
                ));
            }
        })
    });
}

// ── accuracy check ────────────────────────────────────────────────────────────

fn bench_navmesh_accuracy(c: &mut Criterion) {
    let mesh = obstacle_navmesh();

    c.bench_function("navmesh/accuracy_obstacle_detour", |b| {
        b.iter(|| {
            let path = mesh.find_path(
                black_box(NavVec3::new(2.0, 0.0, 2.0)),
                black_box(NavVec3::new(22.0, 0.0, 2.0)),
                NavQuery::Accuracy,
                NavPathMode::Accuracy,
            );
            if let Some(ref waypoints) = path {
                for wp in waypoints {
                    assert!(
                        !(wp.x > 10.0 && wp.x < 15.0 && wp.z < 5.0),
                        "path cut through obstacle gap at {wp:?}"
                    );
                }
            }
            black_box(path)
        })
    });
}

criterion_group!(
    benches,
    bench_navmesh_construction,
    bench_navmesh_single_query,
    bench_navmesh_12_queries,
    bench_rerecast_bake,
    bench_dodgy_crowd,
    bench_navmesh_accuracy,
);
criterion_main!(benches);
