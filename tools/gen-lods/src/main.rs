//! gen-lods: generate LOD meshes for the lunar engine.
//!
//! reads a binary mesh file (flat layout: [u32 vertex_count, u32 index_count, vertex bytes...,
//! index bytes...]) and writes 4 simplified LOD levels plus a `.lod.ron` descriptor.
//!
//! # usage
//!
//! ```
//! gen-lods path/to/mesh.bin
//! gen-lods assets/meshes/*.bin
//! gen-lods --thresholds 10,40,120,350 path/to/mesh.bin
//! ```
//!
//! # output
//!
//! for `mesh.bin` the tool writes:
//! - `mesh_lod1.bin`, `mesh_lod2.bin`, `mesh_lod3.bin`, `mesh_lod4.bin`
//! - `mesh.lod.ron` (descriptor listing thresholds and file names)
//!
//! the lod.ron is a simple RON file game startup code reads to build `MeshLod` components.
//! lod0 = original mesh, lod1-4 = generated simplified meshes.

use std::io::Write;
use std::path::{Path, PathBuf};

// vertex stride in bytes matching GpuVertex3d: position(12) + normal(4) + tangent(4) + uv(4) + uv_lm(4) + color(4) = 32
const VERTEX_STRIDE: usize = 32;

/// default LOD distance thresholds in world units (squared in the descriptor)
const DEFAULT_THRESHOLDS: [f32; 5] = [15.0, 50.0, 150.0, 400.0, f32::INFINITY];
/// simplification ratios per LOD level (lod0 = 1.0 = source)
const RATIOS: [f32; 4] = [0.5, 0.25, 0.10, 0.05];

#[derive(Debug, Clone)]
struct MeshBin {
    indices: Vec<u32>,
    /// full vertex bytes including all attributes
    vertex_bytes: Vec<u8>,
}

/// read a binary mesh file: [u32 vertex_count, u32 index_count, vertex_bytes..., index_bytes...]
///
/// index format: if index_count * 2 bytes matches, u16; else u32.
fn read_mesh(path: &Path) -> MeshBin {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(bytes.len() >= 8, "mesh file too short");

    let vertex_count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let index_count  = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;

    let vb_start = 8;
    let vb_end   = vb_start + vertex_count * VERTEX_STRIDE;
    let vertex_bytes = bytes[vb_start..vb_end].to_vec();

    // detect u16 vs u32 indices by remaining byte count
    let ib_bytes = &bytes[vb_end..];
    let indices: Vec<u32> = if ib_bytes.len() == index_count * 2 {
        ib_bytes.chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()) as u32)
            .collect()
    } else {
        ib_bytes.chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    };
    assert_eq!(indices.len(), index_count);

    MeshBin { indices, vertex_bytes }
}

/// write a binary mesh file in the same format as the input
fn write_mesh(path: &Path, vertex_bytes: &[u8], indices: &[u32]) {
    let vertex_count = vertex_bytes.len() / VERTEX_STRIDE;
    let index_count  = indices.len();

    let use_u16 = vertex_count <= 65535;
    let mut out = Vec::with_capacity(8 + vertex_bytes.len() + index_count * if use_u16 { 2 } else { 4 });

    out.extend_from_slice(&(vertex_count as u32).to_le_bytes());
    out.extend_from_slice(&(index_count  as u32).to_le_bytes());
    out.extend_from_slice(vertex_bytes);

    if use_u16 {
        for &idx in indices {
            out.extend_from_slice(&(idx as u16).to_le_bytes());
        }
    } else {
        for &idx in indices {
            out.extend_from_slice(&idx.to_le_bytes());
        }
    }

    let mut file = std::fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    file.write_all(&out).unwrap();
}

/// remap vertices to only the ones referenced by `new_indices`, preserving attribute bytes
fn remap_vertices(vertex_bytes: &[u8], indices: &[u32]) -> (Vec<u8>, Vec<u32>) {
    let vertex_count = vertex_bytes.len() / VERTEX_STRIDE;
    let mut remap = vec![u32::MAX; vertex_count];
    let mut new_vertices: Vec<u8> = Vec::new();
    let mut next_idx = 0u32;

    let remapped: Vec<u32> = indices.iter().map(|&old| {
        if remap[old as usize] == u32::MAX {
            remap[old as usize] = next_idx;
            let start = old as usize * VERTEX_STRIDE;
            new_vertices.extend_from_slice(&vertex_bytes[start..start + VERTEX_STRIDE]);
            next_idx += 1;
        }
        remap[old as usize]
    }).collect();

    (new_vertices, remapped)
}

fn process_mesh(path: &Path, thresholds: &[f32; 5]) {
    let stem = path.file_stem().unwrap().to_string_lossy();
    let parent = path.parent().unwrap_or(Path::new("."));

    let source = read_mesh(path);
    let vertex_count = source.vertex_bytes.len() / VERTEX_STRIDE;

    println!("{}: {} verts, {} tris", path.display(), vertex_count, source.indices.len() / 3);

    let mut lod_files: Vec<String> = vec![path.file_name().unwrap().to_string_lossy().into_owned()];

    for (level, &ratio) in RATIOS.iter().enumerate() {
        let target_count = ((source.indices.len() as f32 * ratio) as usize).max(3) / 3 * 3;

        // simplify using meshopt — position is at offset 0, stride = VERTEX_STRIDE
        let adapter = meshopt::VertexDataAdapter::new(&source.vertex_bytes, VERTEX_STRIDE, 0)
            .expect("vertex data adapter");
        let simplified = meshopt::simplify(
            &source.indices,
            &adapter,
            target_count,
            1e-2,
            meshopt::SimplifyOptions::None,
            None,
        );

        // optimize vertex cache on the simplified indices
        let optimized = meshopt::optimize_vertex_cache(&simplified, vertex_count);

        let (lod_verts, lod_indices) = remap_vertices(&source.vertex_bytes, &optimized);

        let lod_name = format!("{stem}_lod{}.bin", level + 1);
        let lod_path = parent.join(&lod_name);
        write_mesh(&lod_path, &lod_verts, &lod_indices);

        let lod_vcount = lod_verts.len() / VERTEX_STRIDE;
        let lod_tcount = lod_indices.len() / 3;
        println!("  lod{}: {} verts, {} tris ({:.0}% of source)", level + 1, lod_vcount, lod_tcount, lod_tcount as f32 / (source.indices.len() / 3) as f32 * 100.0);

        lod_files.push(lod_name);
    }

    // write .lod.ron descriptor
    let descriptor_path = parent.join(format!("{stem}.lod.ron"));
    let mut ron = String::new();
    ron.push_str("MeshLodDescriptor(\n    levels: [\n");
    for (i, file) in lod_files.iter().enumerate() {
        let dist = thresholds[i];
        let dist_str = if dist == f32::INFINITY { "null".to_string() } else { format!("{dist:.1}") };
        ron.push_str(&format!("        ( file: \"{file}\", max_dist: {dist_str} ),\n"));
    }
    ron.push_str("    ],\n)\n");

    std::fs::write(&descriptor_path, ron).unwrap_or_else(|e| panic!("write {}: {e}", descriptor_path.display()));
    println!("  wrote {}", descriptor_path.display());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut thresholds = DEFAULT_THRESHOLDS;
    let mut paths: Vec<PathBuf> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        if args[i] == "--thresholds" {
            i += 1;
            let parts: Vec<f32> = args[i].split(',').map(|s| s.parse().expect("threshold must be a float")).collect();
            assert!(parts.len() == 5, "--thresholds requires 5 comma-separated values (lod0..lod4)");
            thresholds = [parts[0], parts[1], parts[2], parts[3], parts[4]];
        } else {
            // expand globs
            for entry in glob::glob(&args[i]).expect("invalid glob pattern") {
                paths.push(entry.expect("glob error"));
            }
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!("usage: gen-lods [--thresholds d0,d1,d2,d3,d4] <mesh.bin> [...]");
        std::process::exit(1);
    }

    for path in &paths {
        process_mesh(path, &thresholds);
    }
}
