//! game loop
//!
//! logic tick rate is set independently of frame cap and can only be
//! 30, 60, 120, or 240 hz. 30hz is a low-end fallback only; 60hz is the
//! standard minimum. rendering runs uncapped (or at the configured frame cap)
//! and is decoupled from logic entirely.
//!
//! # fixed timestep
//!
//! the game loop uses an accumulator-based fixed timestep so physics and game
//! logic always see a constant `time.delta_seconds()` equal to `1 / tick_hz`.
//! if a frame takes longer than the tick interval, multiple ticks run that frame
//! (capped at 5 to prevent spiral of death). wall-clock elapsed time per render
//! frame is available via `time.real_delta_seconds()` for animation blending.

use std::time::{Duration, Instant};

/// logic tick rate. only these four values are valid.
///
/// choose the highest rate the target hardware can sustain at full load.
/// 30hz is a last-resort for potato hardware — prefer 60hz as the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickRate {
    /// 30hz — emergency low-end mode only
    Hz30,
    /// 60hz — standard minimum
    Hz60,
    /// 120hz — for competitive or highly responsive games
    Hz120,
    /// 240hz — for games that need sub-frame input precision
    Hz240,
}

impl TickRate {
    /// fixed tick interval for this rate
    #[must_use]
    pub fn interval(&self) -> Duration {
        match self {
            Self::Hz30  => Duration::from_secs_f64(1.0 / 30.0),
            Self::Hz60  => Duration::from_secs_f64(1.0 / 60.0),
            Self::Hz120 => Duration::from_secs_f64(1.0 / 120.0),
            Self::Hz240 => Duration::from_secs_f64(1.0 / 240.0),
        }
    }

    /// fixed delta in seconds — what `Time::delta_seconds()` returns each tick
    #[must_use]
    pub const fn delta_seconds(&self) -> f32 {
        match self {
            Self::Hz30  => 1.0 / 30.0,
            Self::Hz60  => 1.0 / 60.0,
            Self::Hz120 => 1.0 / 120.0,
            Self::Hz240 => 1.0 / 240.0,
        }
    }
}

/// game loop state — manages the fixed-step accumulator and frame rate limiting.
///
/// call [`GameLoop::tick`] each render frame to get:
/// - how many logic ticks to run (0-5)
/// - the wall-clock time since the last render frame (for rendering interpolation)
///
/// then advance `Time` by `tick_rate.delta_seconds()` per tick.
pub struct GameLoop {
    /// target frame cap (0 = uncapped / vsync-limited)
    frame_cap: u32,
    /// logic tick rate — independent of frame cap
    tick_rate: TickRate,
    /// accumulator for fixed timestep
    accumulator: Duration,
    /// last render frame timestamp
    last_frame: Instant,
    /// whether the loop should continue
    running: bool,
}

impl GameLoop {
    /// create a new game loop.
    ///
    /// `frame_cap` is the render frame cap (0 = uncapped). `tick_rate` is the
    /// fixed logic rate and is completely independent of the render rate.
    #[must_use]
    pub fn new(frame_cap: u32, tick_rate: TickRate) -> Self {
        log::info!("game loop: frame_cap={frame_cap}, tick_rate={tick_rate:?}");
        Self {
            frame_cap,
            tick_rate,
            accumulator: Duration::ZERO,
            last_frame: Instant::now(),
            running: true,
        }
    }

    /// get the current frame cap
    #[must_use]
    pub const fn frame_cap(&self) -> u32 {
        self.frame_cap
    }

    /// set the render frame cap without changing the tick rate
    pub fn set_frame_cap(&mut self, frame_cap: u32) {
        self.frame_cap = frame_cap;
        log::info!("game loop: frame_cap changed to {frame_cap}");
    }

    /// get the current tick rate
    #[must_use]
    pub const fn tick_rate(&self) -> TickRate {
        self.tick_rate
    }

    /// change the logic tick rate at runtime.
    ///
    /// the accumulator is reset to avoid a burst of ticks after the change.
    pub fn set_tick_rate(&mut self, tick_rate: TickRate) {
        self.tick_rate = tick_rate;
        self.accumulator = Duration::ZERO;
        log::info!("game loop: tick_rate changed to {tick_rate:?}");
    }

    /// check if the loop should continue
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// stop the game loop
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// advance the loop by one render frame.
    ///
    /// returns `(ticks, frame_delta)`:
    /// - `ticks`: how many logic ticks to run this frame (0-5)
    /// - `frame_delta`: wall-clock seconds since last render frame
    ///
    /// advance `Time` by `tick_rate.delta_seconds()` per tick, and by
    /// `frame_delta` for `real_delta_seconds` (once per render frame).
    pub fn tick(&mut self) -> (u32, f32) {
        let now = Instant::now();
        let delta = now - self.last_frame;
        self.last_frame = now;
        let frame_delta = delta.as_secs_f32();

        self.accumulator += delta;

        let tick_interval = self.tick_rate.interval();
        let mut ticks = 0u32;
        while self.accumulator >= tick_interval {
            self.accumulator -= tick_interval;
            ticks += 1;
        }

        (ticks.min(5), frame_delta)
    }

    /// apply render frame rate limiting.
    ///
    /// uses a hybrid sleep + spin-wait: sleep for all but the last 1ms,
    /// then spin-wait for precision. no-op when frame_cap is 0 (vsync-limited).
    pub fn apply_frame_cap(&self) {
        if self.frame_cap == 0 {
            return;
        }
        let frame_duration = Duration::from_secs_f64(1.0 / f64::from(self.frame_cap));
        let elapsed = self.last_frame.elapsed();
        if elapsed < frame_duration {
            let remaining = frame_duration - elapsed;
            if remaining > Duration::from_millis(1) {
                std::thread::sleep(remaining - Duration::from_millis(1));
            }
            while self.last_frame.elapsed() < frame_duration {
                std::hint::spin_loop();
            }
        }
    }
}
