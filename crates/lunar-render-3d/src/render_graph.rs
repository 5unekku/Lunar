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
		self.passes.push(PassNode {
			name: name.to_string(),
			reads,
			writes,
		});
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
		let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
		let mut order = Vec::with_capacity(n);
		while let Some(i) = queue.pop_front() {
			order.push(PassId(i as u32));
			for &j in &adj[i] {
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

#[cfg(test)]
mod tests {
	use super::*;

	/// position of `id` in the sorted order; panics if missing.
	fn position(order: &[PassId], id: PassId) -> usize {
		order.iter().position(|&p| p == id).expect("pass missing from sorted order")
	}

	#[test]
	fn writer_sorts_before_reader_regardless_of_registration_order() {
		let mut graph = RenderGraph::new();
		let depth = graph.texture("depth");
		let hdr = graph.texture("hdr");
		// register in reverse dependency order on purpose
		let post = graph.add_pass("post", vec![hdr], vec![]);
		let opaque = graph.add_pass("opaque", vec![depth], vec![hdr]);
		let zprepass = graph.add_pass("zprepass", vec![], vec![depth]);
		let order = graph.sorted_pass_ids().to_vec();
		assert!(position(&order, zprepass) < position(&order, opaque));
		assert!(position(&order, opaque) < position(&order, post));
	}

	#[test]
	fn diamond_dependency_resolves() {
		let mut graph = RenderGraph::new();
		let t1 = graph.texture("t1");
		let t2 = graph.texture("t2");
		let t3 = graph.texture("t3");
		let top = graph.add_pass("top", vec![], vec![t1]);
		let left = graph.add_pass("left", vec![t1], vec![t2]);
		let right = graph.add_pass("right", vec![t1], vec![t3]);
		let bottom = graph.add_pass("bottom", vec![t2, t3], vec![]);
		let order = graph.sorted_pass_ids().to_vec();
		assert_eq!(order.len(), 4);
		assert!(position(&order, top) < position(&order, left));
		assert!(position(&order, top) < position(&order, right));
		assert!(position(&order, left) < position(&order, bottom));
		assert!(position(&order, right) < position(&order, bottom));
	}

	#[test]
	fn independent_passes_keep_registration_order() {
		let mut graph = RenderGraph::new();
		let a = graph.add_pass("a", vec![], vec![]);
		let b = graph.add_pass("b", vec![], vec![]);
		let c = graph.add_pass("c", vec![], vec![]);
		assert_eq!(graph.sorted_pass_ids(), &[a, b, c]);
	}

	#[test]
	fn multiple_writers_all_sort_before_reader() {
		let mut graph = RenderGraph::new();
		// depth is written by both shadow and zprepass (clear + draw), read by gtao
		let depth = graph.texture("depth");
		let gtao = graph.add_pass("gtao", vec![depth], vec![]);
		let shadow = graph.add_pass("shadow", vec![], vec![depth]);
		let zprepass = graph.add_pass("zprepass", vec![], vec![depth]);
		let order = graph.sorted_pass_ids().to_vec();
		assert!(position(&order, shadow) < position(&order, gtao));
		assert!(position(&order, zprepass) < position(&order, gtao));
	}

	#[test]
	fn cycle_falls_back_to_registration_order() {
		let mut graph = RenderGraph::new();
		let t1 = graph.texture("t1");
		let t2 = graph.texture("t2");
		// a and b form a 2-cycle; the guard must emit every pass exactly once
		let a = graph.add_pass("a", vec![t2], vec![t1]);
		let b = graph.add_pass("b", vec![t1], vec![t2]);
		assert_eq!(graph.sorted_pass_ids(), &[a, b]);
	}

	#[test]
	fn pass_reading_its_own_write_gets_no_self_edge() {
		let mut graph = RenderGraph::new();
		let hdr = graph.texture("hdr");
		// e.g. an in-place post effect; the src != i guard must skip the self edge
		let inplace = graph.add_pass("inplace", vec![hdr], vec![hdr]);
		let reader = graph.add_pass("reader", vec![hdr], vec![]);
		let order = graph.sorted_pass_ids().to_vec();
		assert_eq!(order.len(), 2);
		assert!(position(&order, inplace) < position(&order, reader));
	}

	#[test]
	fn texture_handles_are_idempotent_by_name() {
		let mut graph = RenderGraph::new();
		let first = graph.texture("depth");
		let again = graph.texture("depth");
		let other = graph.texture("hdr");
		assert_eq!(first, again);
		assert_ne!(first, other);
	}

	#[test]
	fn sorted_order_refreshes_after_adding_a_pass() {
		let mut graph = RenderGraph::new();
		let depth = graph.texture("depth");
		let reader = graph.add_pass("reader", vec![depth], vec![]);
		assert_eq!(graph.sorted_pass_ids().len(), 1);
		// adding a writer afterwards must invalidate the cached order
		let writer = graph.add_pass("writer", vec![], vec![depth]);
		let order = graph.sorted_pass_ids().to_vec();
		assert_eq!(order.len(), 2);
		assert!(position(&order, writer) < position(&order, reader));
	}

	#[test]
	fn pass_names_and_count_round_trip() {
		let mut graph = RenderGraph::new();
		let a = graph.add_pass("alpha", vec![], vec![]);
		let b = graph.add_pass("beta", vec![], vec![]);
		assert_eq!(graph.pass_count(), 2);
		assert_eq!(graph.pass_name(a), "alpha");
		assert_eq!(graph.pass_name(b), "beta");
	}
}
