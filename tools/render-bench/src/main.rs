//! render-bench: a headless, deterministic benchmark harness for the Lunar 3d
//! and 2d render paths. boots each scene off-screen, measures cold-start,
//! pipeline-cache warm/cold boot, and steady-state frame time, and captures or
//! checks a per-adapter golden frame. results are keyed by hostname + adapter
//! and written to docs/bench/.
//!
//! usage:
//!   cargo run --release -p render-bench -- [flags]
//!
//! flags:
//!   --scene <name>       run only one scene (static-city|dynamic-swarm|feature-reel|sprite-storm)
//!   --runs <n>           fresh-boot runs per scene (default 3)
//!   --frames <n>         measured ticks per run (default 500)
//!   --golden <mode>      off | capture | check   (default check)
//!   --cache-probe        additionally measure pipeline-cache cold vs warm boot
//!   --out <dir>          output directory (default docs/bench)

mod common;
mod golden;
mod harness;
mod metrics;
mod scenes;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use harness::Opts;
use metrics::BenchReport;

struct Cli {
	scene: Option<String>,
	runs: usize,
	frames: usize,
	golden_mode: String,
	cache_probe: bool,
	out: PathBuf,
	width: u32,
	height: u32,
}

fn parse_cli() -> Result<Cli, String> {
	let mut cli = Cli {
		scene: None,
		runs: 3,
		frames: 500,
		golden_mode: "check".into(),
		cache_probe: false,
		out: PathBuf::from("docs/bench"),
		width: 1280,
		height: 720,
	};
	let mut args = std::env::args().skip(1);
	while let Some(arg) = args.next() {
		let mut next = || args.next().ok_or_else(|| format!("{arg} needs a value"));
		match arg.as_str() {
			"--scene" => cli.scene = Some(next()?),
			"--runs" => cli.runs = next()?.parse().map_err(|_| "bad --runs")?,
			"--frames" => cli.frames = next()?.parse().map_err(|_| "bad --frames")?,
			"--golden" => cli.golden_mode = next()?,
			"--cache-probe" => cli.cache_probe = true,
			"--out" => cli.out = PathBuf::from(next()?),
			"--width" => cli.width = next()?.parse().map_err(|_| "bad --width")?,
			"--height" => cli.height = next()?.parse().map_err(|_| "bad --height")?,
			"-h" | "--help" => return Err("help".into()),
			other => return Err(format!("unknown flag: {other}")),
		}
	}
	if !matches!(cli.golden_mode.as_str(), "off" | "capture" | "check") {
		return Err(format!("--golden must be off|capture|check, got {}", cli.golden_mode));
	}
	Ok(cli)
}

fn main() {
	env_logger::init();
	let cli = match parse_cli() {
		Ok(c) => c,
		Err(msg) => {
			if msg != "help" {
				eprintln!("error: {msg}\n");
			}
			eprintln!("{}", include_str!("usage.txt"));
			std::process::exit(if msg == "help" { 0 } else { 2 });
		}
	};

	let instance = wgpu::Instance::default();

	// probe the adapter up front so results are keyed correctly even if a scene
	// fails to produce frames.
	let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
		power_preference: wgpu::PowerPreference::HighPerformance,
		force_fallback_adapter: false,
		compatible_surface: None,
	}))
	.expect("no usable gpu adapter");
	let info = adapter.get_info();
	let adapter_key = adapter_key(&info);
	println!(
		"adapter: {} ({:?}, {}) -> key {adapter_key}",
		info.name, info.backend, info.driver
	);

	let selected: Vec<scenes::Scene> = match &cli.scene {
		Some(name) => vec![scenes::by_name(name).unwrap_or_else(|| {
			eprintln!("unknown scene: {name}");
			std::process::exit(2);
		})],
		None => scenes::all(),
	};

	let opts = Opts {
		width: cli.width,
		height: cli.height,
		runs: cli.runs,
		frames: cli.frames,
		golden_mode: cli.golden_mode.clone(),
		adapter_key: adapter_key.clone(),
		golden_dir: cli.out.join("golden"),
	};

	let (cache_cold, cache_warm) = if cli.cache_probe {
		match harness::cache_probe(&instance, &opts) {
			Some((cold, warm)) => {
				println!("pipeline cache: cold {cold:.1} ms -> warm {warm:.1} ms");
				(Some(cold), Some(warm))
			}
			None => {
				println!("pipeline cache: not exposed on this adapter, skipping probe");
				(None, None)
			}
		}
	} else {
		(None, None)
	};

	let mut report = BenchReport {
		hostname: hostname(),
		adapter: info.name.clone(),
		backend: format!("{:?}", info.backend),
		driver: format!("{} {}", info.driver, info.driver_info),
		commit: git_commit(),
		unix_time: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
		cache_cold_boot_ms: cache_cold,
		cache_warm_boot_ms: cache_warm,
		scenes: Vec::new(),
	};

	for scene in &selected {
		println!("running scene {} ({} runs × {} frames)...", scene.name, cli.runs, cli.frames);
		let row = harness::run_scene(&instance, scene, &opts);
		println!(
			"  {} : cold {:.1} ms, first-frame {:.1} ms, steady mean {:.3} ms (p99 {:.3}) {}",
			row.scene,
			row.cold_start_ms,
			row.first_frame_ms,
			row.mean_ms,
			row.p99_ms,
			row.golden.as_ref().map_or("", |g| g.status),
		);
		report.scenes.push(row);
	}

	write_report(&cli.out, &adapter_key, &report);
}

/// write the json + markdown report, keyed by hostname + adapter.
fn write_report(out: &std::path::Path, adapter_key: &str, report: &BenchReport) {
	if let Err(e) = std::fs::create_dir_all(out) {
		eprintln!("could not create {}: {e}", out.display());
		return;
	}
	let stem = format!("{}_{}", sanitize(&report.hostname), adapter_key);
	let json_path = out.join(format!("{stem}.json"));
	let md_path = out.join(format!("{stem}.md"));
	if let Err(e) = std::fs::write(&json_path, report.to_json()) {
		eprintln!("could not write {}: {e}", json_path.display());
	}
	if let Err(e) = std::fs::write(&md_path, report.to_markdown()) {
		eprintln!("could not write {}: {e}", md_path.display());
	}
	println!("wrote {} and {}", json_path.display(), md_path.display());
}

/// stable per-adapter key: `{vendor:04x}_{device:04x}_{backend}`. baselines and
/// golden frames are keyed on it so results never mix across gpus.
fn adapter_key(info: &wgpu::AdapterInfo) -> String {
	format!("{:04x}_{:04x}_{}", info.vendor, info.device, sanitize(&format!("{:?}", info.backend)))
}

/// lowercase alnum + underscore, for filesystem-safe keys.
fn sanitize(s: &str) -> String {
	s.chars()
		.map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
		.collect()
}

fn hostname() -> String {
	std::env::var("HOSTNAME")
		.or_else(|_| std::env::var("COMPUTERNAME"))
		.ok()
		.filter(|s| !s.is_empty())
		.or_else(|| {
			std::process::Command::new("hostname")
				.output()
				.ok()
				.filter(|o| o.status.success())
				.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
		})
		.filter(|s| !s.is_empty())
		.unwrap_or_else(|| "unknown-host".into())
}

fn git_commit() -> String {
	std::process::Command::new("git")
		.args(["rev-parse", "--short", "HEAD"])
		.output()
		.ok()
		.filter(|o| o.status.success())
		.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
		.filter(|s| !s.is_empty())
		.unwrap_or_else(|| "unknown".into())
}
