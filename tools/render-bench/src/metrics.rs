//! frame timing stats + bench report output. json and markdown are hand-rolled
//! (flat structures, not worth a serde dependency in a tool crate).

/// summary stats over per-frame cpu times, in milliseconds.
pub struct FrameStats {
	pub mean_ms: f64,
	pub p50_ms: f64,
	pub p99_ms: f64,
	pub min_ms: f64,
	pub max_ms: f64,
}

/// sorts the samples in place and derives the summary stats.
pub fn frame_stats(samples: &mut [f64]) -> FrameStats {
	assert!(!samples.is_empty(), "frame_stats needs at least one sample");
	samples.sort_unstable_by(|a, b| a.partial_cmp(b).expect("frame time was NaN"));
	let quantile = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
	FrameStats {
		mean_ms: samples.iter().sum::<f64>() / samples.len() as f64,
		p50_ms: quantile(0.50),
		p99_ms: quantile(0.99),
		min_ms: samples[0],
		max_ms: samples[samples.len() - 1],
	}
}

/// median of already-computed per-run values (runs are few, clone-and-sort is fine).
pub fn median(values: &[f64]) -> f64 {
	let mut sorted = values.to_vec();
	sorted.sort_unstable_by(|a, b| a.partial_cmp(b).expect("value was NaN"));
	sorted[sorted.len() / 2]
}

/// outcome of the golden-frame step for one scene.
pub struct GoldenSummary {
	/// "captured", "passed", "failed", or "missing-reference"
	pub status: &'static str,
	pub max_channel_diff: u8,
	pub differing_pixels: usize,
	pub total_pixels: usize,
	pub path: String,
}

/// results for one scene: boot cost, first frame, steady-state medians across runs.
pub struct SceneReport {
	pub scene: String,
	/// engine + app construction through the end of the first tick (run 1 only —
	/// later in-process runs boot warm and are not comparable).
	pub cold_start_ms: f64,
	/// duration of the first tick alone (first-frame pipeline/bind-group creation).
	pub first_frame_ms: f64,
	/// per-stat median across runs.
	pub mean_ms: f64,
	pub p50_ms: f64,
	pub p99_ms: f64,
	pub min_ms: f64,
	pub max_ms: f64,
	pub frames_per_run: usize,
	pub runs: usize,
	pub golden: Option<GoldenSummary>,
}

/// the full report for one host + adapter, serialized to docs/bench/.
pub struct BenchReport {
	pub hostname: String,
	pub adapter: String,
	pub backend: String,
	pub driver: String,
	pub commit: String,
	pub unix_time: u64,
	/// pipeline-cache probe: boot time with the cache blob deleted vs present.
	pub cache_cold_boot_ms: Option<f64>,
	pub cache_warm_boot_ms: Option<f64>,
	pub scenes: Vec<SceneReport>,
}

fn json_escape(s: &str) -> String {
	s.chars()
		.flat_map(|c| match c {
			'"' => "\\\"".chars().collect::<Vec<_>>(),
			'\\' => "\\\\".chars().collect(),
			'\n' => "\\n".chars().collect(),
			c if (c as u32) < 0x20 => format!("\\u{:04x}", c as u32).chars().collect(),
			c => vec![c],
		})
		.collect()
}

fn fmt_ms(v: f64) -> String {
	format!("{v:.3}")
}

impl BenchReport {
	pub fn to_json(&self) -> String {
		let mut out = String::from("{\n");
		out += &format!("  \"hostname\": \"{}\",\n", json_escape(&self.hostname));
		out += &format!("  \"adapter\": \"{}\",\n", json_escape(&self.adapter));
		out += &format!("  \"backend\": \"{}\",\n", json_escape(&self.backend));
		out += &format!("  \"driver\": \"{}\",\n", json_escape(&self.driver));
		out += &format!("  \"commit\": \"{}\",\n", json_escape(&self.commit));
		out += &format!("  \"unix_time\": {},\n", self.unix_time);
		out += &format!(
			"  \"cache_cold_boot_ms\": {},\n",
			self.cache_cold_boot_ms.map_or("null".into(), fmt_ms)
		);
		out += &format!(
			"  \"cache_warm_boot_ms\": {},\n",
			self.cache_warm_boot_ms.map_or("null".into(), fmt_ms)
		);
		out += "  \"scenes\": [\n";
		for (i, s) in self.scenes.iter().enumerate() {
			out += "    {\n";
			out += &format!("      \"scene\": \"{}\",\n", json_escape(&s.scene));
			out += &format!("      \"cold_start_ms\": {},\n", fmt_ms(s.cold_start_ms));
			out += &format!("      \"first_frame_ms\": {},\n", fmt_ms(s.first_frame_ms));
			out += &format!("      \"mean_ms\": {},\n", fmt_ms(s.mean_ms));
			out += &format!("      \"p50_ms\": {},\n", fmt_ms(s.p50_ms));
			out += &format!("      \"p99_ms\": {},\n", fmt_ms(s.p99_ms));
			out += &format!("      \"min_ms\": {},\n", fmt_ms(s.min_ms));
			out += &format!("      \"max_ms\": {},\n", fmt_ms(s.max_ms));
			out += &format!("      \"frames_per_run\": {},\n", s.frames_per_run);
			out += &format!("      \"runs\": {},\n", s.runs);
			match &s.golden {
				Some(g) => {
					out += "      \"golden\": {\n";
					out += &format!("        \"status\": \"{}\",\n", g.status);
					out += &format!("        \"max_channel_diff\": {},\n", g.max_channel_diff);
					out += &format!("        \"differing_pixels\": {},\n", g.differing_pixels);
					out += &format!("        \"total_pixels\": {},\n", g.total_pixels);
					out += &format!("        \"path\": \"{}\"\n", json_escape(&g.path));
					out += "      }\n";
				}
				None => out += "      \"golden\": null\n",
			}
			out += if i + 1 == self.scenes.len() { "    }\n" } else { "    },\n" };
		}
		out += "  ]\n}\n";
		out
	}

	pub fn to_markdown(&self) -> String {
		let mut out = String::new();
		out += &format!(
			"# render bench — {} / {} ({})\n\n",
			self.hostname, self.adapter, self.backend
		);
		out += &format!(
			"driver: {} · commit: {} · unix time: {}\n\n",
			self.driver, self.commit, self.unix_time
		);
		if let (Some(cold), Some(warm)) = (self.cache_cold_boot_ms, self.cache_warm_boot_ms) {
			out += &format!(
				"pipeline cache: cold boot {} ms → warm boot {} ms\n\n",
				fmt_ms(cold),
				fmt_ms(warm)
			);
		}
		out += "| scene | cold start (ms) | first frame (ms) | mean (ms) | p50 (ms) | p99 (ms) | frames × runs | golden |\n";
		out += "|---|---|---|---|---|---|---|---|\n";
		for s in &self.scenes {
			let golden = s
				.golden
				.as_ref()
				.map_or("—".to_string(), |g| g.status.to_string());
			out += &format!(
				"| {} | {} | {} | {} | {} | {} | {}×{} | {} |\n",
				s.scene,
				fmt_ms(s.cold_start_ms),
				fmt_ms(s.first_frame_ms),
				fmt_ms(s.mean_ms),
				fmt_ms(s.p50_ms),
				fmt_ms(s.p99_ms),
				s.frames_per_run,
				s.runs,
				golden
			);
		}
		out += "\nmethodology: per-scene stats are the per-stat median across runs; cold start is run 1 only (later in-process runs boot warm). frame time is cpu-side tick duration (headless, no vsync).\n";
		out
	}
}
