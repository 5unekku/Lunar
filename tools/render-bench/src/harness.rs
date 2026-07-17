//! the measurement loop: boot a scene headless, warm up, capture/check a golden
//! frame, then time a fixed number of ticks across several fresh-boot runs.
//!
//! every run boots a brand-new engine + app from a shared wgpu instance, so the
//! per-run steady-state numbers are directly comparable; only run 0 carries a
//! meaningful cold-start number (later runs boot warm in-process).

use std::path::{Path, PathBuf};
use std::time::Instant;

use lunar::lunar_render_3d::RenderEngine3d;

use crate::golden::{self, GoldenDiff};
use crate::metrics::{frame_stats, median, GoldenSummary, SceneReport};
use crate::scenes::{Dim, Scene};

/// fixed logic tick used for every scene. deterministic time keeps STAA jitter,
/// film grain, and particle spawn accumulation identical across runs.
pub const TICK_DELTA: f32 = 1.0 / 60.0;
/// warm-up ticks before the golden frame and the measured window: lets pipeline
/// creation, first-frame uploads, and temporal history settle.
pub const WARMUP_FRAMES: usize = 128;

pub struct Opts {
	pub width: u32,
	pub height: u32,
	pub runs: usize,
	pub frames: usize,
	/// "off" | "capture" | "check"
	pub golden_mode: String,
	pub adapter_key: String,
	/// repo root for resolving the golden directory.
	pub golden_dir: PathBuf,
}

impl Opts {
	fn golden_path(&self, scene: &str) -> PathBuf {
		self.golden_dir.join(&self.adapter_key).join(format!("{scene}.png"))
	}
}

/// per-boot readback of the golden frame, already converted bgra -> rgba.
struct RunResult {
	stats_samples: Vec<f64>,
	golden_rgba: Option<(Vec<u8>, u32, u32)>,
	cold_start_ms: f64,
	first_frame_ms: f64,
}

/// run one scene end to end and produce its report row.
pub fn run_scene(instance: &wgpu::Instance, scene: &Scene, opts: &Opts) -> SceneReport {
	let mut runs: Vec<RunResult> = Vec::with_capacity(opts.runs);
	for run_index in 0..opts.runs {
		let want_golden = opts.golden_mode != "off" && run_index < 2;
		runs.push(match scene.dim {
			Dim::D3 => boot_and_measure_3d(instance, scene, opts, want_golden),
			Dim::D2 => boot_and_measure_2d(instance, scene, opts, want_golden),
		});
	}

	// per-stat median across runs; cold-start / first-frame from run 0 only.
	let mut means = Vec::new();
	let mut p50s = Vec::new();
	let mut p99s = Vec::new();
	let mut mins = Vec::new();
	let mut maxs = Vec::new();
	for run in &mut runs {
		let stats = frame_stats(&mut run.stats_samples);
		means.push(stats.mean_ms);
		p50s.push(stats.p50_ms);
		p99s.push(stats.p99_ms);
		mins.push(stats.min_ms);
		maxs.push(stats.max_ms);
	}

	let golden = if opts.golden_mode == "off" {
		None
	} else {
		Some(golden_step(scene, opts, &runs))
	};

	SceneReport {
		scene: scene.name.to_string(),
		cold_start_ms: runs[0].cold_start_ms,
		first_frame_ms: runs[0].first_frame_ms,
		mean_ms: median(&means),
		p50_ms: median(&p50s),
		p99_ms: median(&p99s),
		min_ms: median(&mins),
		max_ms: median(&maxs),
		frames_per_run: opts.frames,
		runs: opts.runs,
		golden,
	}
}

/// capture, or check against, the committed reference for this scene+adapter.
fn golden_step(scene: &Scene, opts: &Opts, runs: &[RunResult]) -> GoldenSummary {
	let path = opts.golden_path(scene.name);
	let Some((rgba, width, height)) = runs[0].golden_rgba.clone() else {
		return missing(&path, "no-readback");
	};
	let total = (width * height) as usize;

	// self-consistency: run 0 vs run 1 at the same fixed frame must match, or the
	// scene is nondeterministic and its golden is meaningless.
	if let Some((rgba1, _, _)) = runs.get(1).and_then(|r| r.golden_rgba.clone()) {
		let diff = golden::compare(&rgba, &rgba1);
		if !diff.passed() {
			return GoldenSummary {
				status: "capture-unstable",
				max_channel_diff: diff.max_channel_diff,
				differing_pixels: diff.differing_pixels,
				total_pixels: total,
				path: path.display().to_string(),
			};
		}
	}

	if opts.golden_mode == "capture" {
		if let Err(e) = golden::write_png(&path, &rgba, width, height) {
			eprintln!("golden write failed for {}: {e}", scene.name);
			return missing(&path, "write-failed");
		}
		return GoldenSummary {
			status: "captured",
			max_channel_diff: 0,
			differing_pixels: 0,
			total_pixels: total,
			path: path.display().to_string(),
		};
	}

	// check mode
	match golden::read_png(&path) {
		Err(_) => missing(&path, "missing-reference"),
		Ok((_reference, rw, rh)) if (rw, rh) != (width, height) => GoldenSummary {
			status: "failed",
			max_channel_diff: 255,
			differing_pixels: total,
			total_pixels: total,
			path: path.display().to_string(),
		},
		Ok((reference, _, _)) => {
			let GoldenDiff { max_channel_diff, differing_pixels, total_pixels } =
				golden::compare(&reference, &rgba);
			GoldenSummary {
				status: if differing_pixels == 0 { "passed" } else { "failed" },
				max_channel_diff,
				differing_pixels,
				total_pixels,
				path: path.display().to_string(),
			}
		}
	}
}

fn missing(path: &Path, status: &'static str) -> GoldenSummary {
	GoldenSummary {
		status,
		max_channel_diff: 0,
		differing_pixels: 0,
		total_pixels: 0,
		path: path.display().to_string(),
	}
}

/// boot a 3d scene, warm up, grab the golden frame, then time the measured window.
fn boot_and_measure_3d(
	instance: &wgpu::Instance,
	scene: &Scene,
	opts: &Opts,
	want_golden: bool,
) -> RunResult {
	use lunar::lunar_assets::AssetPlugin;
	use lunar::lunar_3d::Plugin3d;
	use lunar::lunar_render_3d::{RenderConfig3d, RenderPlugin3d};
	use lunar::prelude::*;

	let boot_start = Instant::now();
	let config = RenderConfig3d {
		width: opts.width,
		height: opts.height,
		vsync: false,
		..Default::default()
	};
	let engine = RenderEngine3d::headless(instance, &config);

	let mut app = App::new();
	app.insert_resource(WindowSettings::new(opts.width, opts.height, false));
	app.insert_resource(engine);
	app.add_plugin(Plugin3d);
	app.add_plugin(RenderPlugin3d);
	app.add_plugin(AssetPlugin);
	app.add_startup_system(pin_max_quality);
	(scene.register)(&mut app);

	// first tick builds plugins, runs startup, and pays first-frame pipeline cost.
	let first_frame_start = Instant::now();
	app.tick(TICK_DELTA);
	let first_frame_ms = first_frame_start.elapsed().as_secs_f64() * 1000.0;
	let cold_start_ms = boot_start.elapsed().as_secs_f64() * 1000.0;

	for _ in 1..WARMUP_FRAMES {
		app.tick(TICK_DELTA);
	}

	let golden_rgba = if want_golden {
		app.engine()
			.world()
			.get_resource::<RenderEngine3d>()
			.and_then(RenderEngine3d::read_headless_rgba)
			.map(to_rgba)
	} else {
		None
	};

	let mut stats_samples = Vec::with_capacity(opts.frames);
	for _ in 0..opts.frames {
		let t = Instant::now();
		app.tick(TICK_DELTA);
		stats_samples.push(t.elapsed().as_secs_f64() * 1000.0);
	}

	RunResult { stats_samples, golden_rgba, cold_start_ms, first_frame_ms }
}

/// boot a 2d scene into an offscreen render target, warm up, grab the golden
/// frame, then time the measured window.
fn boot_and_measure_2d(
	instance: &wgpu::Instance,
	scene: &Scene,
	opts: &Opts,
	want_golden: bool,
) -> RunResult {
	use lunar::lunar_2d::Plugin2d;
	use lunar::lunar_assets::AssetPlugin;
	use lunar::prelude::*;
	use lunar::lunar_render::{RenderConfig, RenderPlugin, RenderTargetStore};

	let boot_start = Instant::now();
	let mut engine = RenderEngine::headless(
		instance,
		RenderConfig { width: opts.width, height: opts.height, vsync: false, ..Default::default() },
	);
	// render into an offscreen target; the 2d path only writes the swapchain when
	// a real window exists, so headless rendering must be routed to a target.
	let mut store = RenderTargetStore::default();
	let (target_id, _handle) = engine.create_render_target(&mut store, opts.width, opts.height);

	let mut camera = Camera::new();
	camera.target = Some(target_id);

	let mut app = App::new();
	app.insert_resource(WindowSettings::new(opts.width, opts.height, false));
	app.insert_resource(engine);
	app.insert_resource(camera);
	app.insert_resource(store);
	app.add_plugin(Plugin2d);
	app.add_plugin(RenderPlugin);
	app.add_plugin(AssetPlugin);
	(scene.register)(&mut app);

	let first_frame_start = Instant::now();
	app.tick(TICK_DELTA);
	let first_frame_ms = first_frame_start.elapsed().as_secs_f64() * 1000.0;
	let cold_start_ms = boot_start.elapsed().as_secs_f64() * 1000.0;

	for _ in 1..WARMUP_FRAMES {
		app.tick(TICK_DELTA);
	}

	let golden_rgba = if want_golden {
		app.engine()
			.world()
			.get_resource::<RenderEngine>()
			.and_then(|e| e.read_target_rgba(target_id))
			.map(to_rgba)
	} else {
		None
	};

	let mut stats_samples = Vec::with_capacity(opts.frames);
	for _ in 0..opts.frames {
		let t = Instant::now();
		app.tick(TICK_DELTA);
		stats_samples.push(t.elapsed().as_secs_f64() * 1000.0);
	}

	RunResult { stats_samples, golden_rgba, cold_start_ms, first_frame_ms }
}

/// convert a raw (bgra, w, h) readback to (rgba, w, h) for golden io.
fn to_rgba((mut bytes, width, height): (Vec<u8>, u32, u32)) -> (Vec<u8>, u32, u32) {
	golden::bgra_to_rgba_in_place(&mut bytes);
	(bytes, width, height)
}

/// startup system pinning every quality knob to maximum. runs after
/// `RenderPlugin3d::build` inserted `QualitySettings::from_tier(tier)` and the
/// default `DevRenderProfile::classic()`, so it overrides both before the first
/// rendered frame — the harness always measures the full feature set.
fn pin_max_quality(mut commands: lunar::prelude::Commands) {
	use lunar::lunar_render_3d::{DevRenderProfile, QualitySettings};
	commands.insert_resource(QualitySettings::maximum());
	commands.insert_resource(DevRenderProfile::full());
}

/// pipeline-cache probe: measure boot-to-first-frame with the on-disk cache
/// deleted (cold) versus present (warm). returns `None` if the engine exposes no
/// cache file on this adapter (e.g. dx12-via-proton, or a driver without
/// PIPELINE_CACHE).
pub fn cache_probe(instance: &wgpu::Instance, opts: &Opts) -> Option<(f64, f64)> {
	// learn the cache path by booting once and letting Drop write it.
	let cache_path = {
		let config = boot_config_3d(opts);
		let engine = RenderEngine3d::headless(instance, &config);
		let path = engine.pipeline_cache_file().map(Path::to_path_buf);
		drive_once(instance, opts, engine);
		path?
	};

	// cold: delete the cache, then time a fresh boot-to-first-frame.
	let _ = std::fs::remove_file(&cache_path);
	let cold = boot_to_first_frame_3d(instance, opts); // this boot rewrites the cache on Drop
	// warm: the cache now exists; time another fresh boot.
	let warm = boot_to_first_frame_3d(instance, opts);
	Some((cold, warm))
}

fn boot_config_3d(opts: &Opts) -> lunar::lunar_render_3d::RenderConfig3d {
	lunar::lunar_render_3d::RenderConfig3d {
		width: opts.width,
		height: opts.height,
		vsync: false,
		..Default::default()
	}
}

/// boot a bare 3d engine+app (no scene content) and time the first tick.
fn boot_to_first_frame_3d(instance: &wgpu::Instance, opts: &Opts) -> f64 {
	use lunar::lunar_assets::AssetPlugin;
	use lunar::lunar_3d::Plugin3d;
	use lunar::lunar_render_3d::RenderPlugin3d;
	use lunar::prelude::*;

	let start = Instant::now();
	let engine = RenderEngine3d::headless(instance, &boot_config_3d(opts));
	let mut app = App::new();
	app.insert_resource(WindowSettings::new(opts.width, opts.height, false));
	app.insert_resource(engine);
	app.add_plugin(Plugin3d);
	app.add_plugin(RenderPlugin3d);
	app.add_plugin(AssetPlugin);
	app.add_startup_system(pin_max_quality);
	app.tick(TICK_DELTA);
	start.elapsed().as_secs_f64() * 1000.0
}

/// tick a pre-built engine a few frames through a throwaway app so pipelines are
/// actually exercised and the cache is populated before Drop writes it.
fn drive_once(_instance: &wgpu::Instance, opts: &Opts, engine: RenderEngine3d) {
	use lunar::lunar_assets::AssetPlugin;
	use lunar::lunar_3d::Plugin3d;
	use lunar::lunar_render_3d::RenderPlugin3d;
	use lunar::prelude::*;

	let mut app = App::new();
	app.insert_resource(WindowSettings::new(opts.width, opts.height, false));
	app.insert_resource(engine);
	app.add_plugin(Plugin3d);
	app.add_plugin(RenderPlugin3d);
	app.add_plugin(AssetPlugin);
	app.add_startup_system(pin_max_quality);
	for _ in 0..8 {
		app.tick(TICK_DELTA);
	}
	// app (and the engine resource) drop here, flushing the pipeline cache.
}
