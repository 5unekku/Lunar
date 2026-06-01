//! render graph DAG: models pass dependencies via declared texture reads and writes.
//!
//! modeled on bevy's extract→prepare→queue→render→cleanup design.
//! each pass declares which logical texture resources it reads and writes.
//! the graph builds a dependency DAG and returns passes in topological order,
//! so all writes happen before the reads that depend on them.
//!
//! this replaces the hardcoded sequential pass order in `render_frame`:
//! instead of a fixed list, passes register themselves and the graph resolves
//! the correct execution order automatically from resource dependencies.
//!
//! # usage
//!
//! ```ignore
//! let mut graph = RenderGraph::new();
//! let depth   = graph.texture("depth");
//! let hdr     = graph.texture("hdr");
//! let bloom   = graph.texture("bloom");
//! let ldr     = graph.texture("ldr");
//!
//! let _shadow  = graph.add_pass("shadow",   vec![],      vec![depth]);
//! let _zprepass = graph.add_pass("zprepass", vec![],     vec![depth]);
//! let _opaque  = graph.add_pass("opaque",   vec![depth], vec![hdr]);
//! let _bloom   = graph.add_pass("bloom",    vec![hdr],   vec![bloom]);
//! let _composite = graph.add_pass("composite", vec![hdr, bloom], vec![ldr]);
//!
//! for pass_id in graph.sorted_pass_ids() {
//!     match graph.pass_name(pass_id) {
//!         "shadow"    => run_shadow_pass(...),
//!         "opaque"    => run_opaque_pass(...),
//!         "bloom"     => run_bloom_pass(...),
//!         "composite" => run_composite_pass(...),
//!         _ => {}
//!     }
//! }
//! ```

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::VecDeque;

/// handle to a logical render texture resource declared in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureRef(pub u32);

/// handle to a registered render pass node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PassId(pub u32);

struct PassNode {
    name: String,
    reads: Vec<TextureRef>,
    writes: Vec<TextureRef>,
}

/// render graph DAG tracking texture dependencies between passes.
///
/// an edge exists from pass A to pass B when A writes a texture that B reads.
/// `sorted_pass_ids` returns a valid topological execution order via Kahn's algorithm.
///
/// the graph is built once per render tier and cached; `sorted_pass_ids` is called
/// each frame to drive pass dispatch in `render_frame`.
pub struct RenderGraph {
    passes: Vec<PassNode>,
    textures: HashMap<String, TextureRef>,
    sorted: Vec<PassId>,
    dirty: bool,
    next_texture_id: u32,
}

impl RenderGraph {
    #[must_use]
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            textures: HashMap::default(),
            sorted: Vec::new(),
            dirty: false,
            next_texture_id: 0,
        }
    }

    /// declare a logical texture resource.
    ///
    /// idempotent: returns the same `TextureRef` for the same name.
    pub fn texture(&mut self, name: &str) -> TextureRef {
        if let Some(&t) = self.textures.get(name) {
            return t;
        }
        let t = TextureRef(self.next_texture_id);
        self.next_texture_id += 1;
        self.textures.insert(name.to_string(), t);
        t
    }

    /// register a pass with declared texture reads and writes.
    ///
    /// an edge is added from every pass that writes a texture this pass reads.
    /// returns the `PassId` assigned to this pass.
    pub fn add_pass(
        &mut self,
        name: &str,
        reads: Vec<TextureRef>,
        writes: Vec<TextureRef>,
    ) -> PassId {
        let id = PassId(self.passes.len() as u32);
        self.passes.push(PassNode { name: name.to_string(), reads, writes });
        self.dirty = true;
        id
    }

    /// compute (or return cached) topological execution order.
    ///
    /// uses Kahn's algorithm. an edge exists from A to B when A writes a texture
    /// that B reads. if the graph has a cycle (not possible in a valid render graph
    /// but guarded against), any remaining passes are appended in registration order.
    pub fn sorted_pass_ids(&mut self) -> &[PassId] {
        if !self.dirty {
            return &self.sorted;
        }
        self.sorted = self.compute_order();
        self.dirty = false;
        &self.sorted
    }

    fn compute_order(&self) -> Vec<PassId> {
        let n = self.passes.len();

        // for each texture, collect all passes that write it (last writer wins for ordering).
        // multiple writers to the same texture are valid (e.g. depth cleared by z-prepass, read by gtao).
        let mut writers: HashMap<TextureRef, Vec<usize>> = HashMap::default();
        for (i, pass) in self.passes.iter().enumerate() {
            for &tex in &pass.writes {
                writers.entry(tex).or_default().push(i);
            }
        }

        // build dependency edges: writer → reader
        let mut adj: Vec<HashSet<usize>> = vec![HashSet::default(); n];
        let mut in_degree: Vec<usize> = vec![0; n];
        for (i, pass) in self.passes.iter().enumerate() {
            for &tex in &pass.reads {
                if let Some(srcs) = writers.get(&tex) {
                    for &src in srcs {
                        if src != i && adj[src].insert(i) {
                            in_degree[i] += 1;
                        }
                    }
                }
            }
        }

        // Kahn's topological sort
        let mut queue: VecDeque<usize> =
            (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(i) = queue.pop_front() {
            order.push(PassId(i as u32));
            for j in adj[i].iter().copied().collect::<Vec<_>>() {
                in_degree[j] -= 1;
                if in_degree[j] == 0 {
                    queue.push_back(j);
                }
            }
        }
        // cycle guard: append unordered passes in registration order
        if order.len() < n {
            for i in 0..n {
                if !order.contains(&PassId(i as u32)) {
                    order.push(PassId(i as u32));
                }
            }
        }
        order
    }

    /// name of the pass, for dispatch matching in `render_frame`.
    #[must_use]
    pub fn pass_name(&self, id: PassId) -> &str {
        &self.passes[id.0 as usize].name
    }

    /// number of registered passes.
    #[must_use]
    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }
}

impl Default for RenderGraph {
    fn default() -> Self {
        Self::new()
    }
}
