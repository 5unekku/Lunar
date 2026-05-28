//! game loop
//!
//! fixed tickrate correlated to frame cap with three buckets:
//! - frame cap 1-60: 60hz tick
//! - frame cap 61-120: 120hz tick
//! - frame cap 121+: 240hz tick, ceiling regardless of frame cap
//!
//! rendering runs uncapped and should feel smooth at high framerates.
//!
//! # fixed timestep
//!
//! the game loop uses an accumulator-based fixed timestep to ensure
//! deterministic physics and game logic. if the frame takes longer
//! than the tick interval, multiple ticks may run (capped at 5 to
//! prevent spiral of death).

use std::time::{Duration, Instant};

/// tick rate buckets based on frame cap.
///
/// determines how often the ECS schedule runs, independent of render framerate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickRate {
    /// 60hz tick for frame cap 1-60
    Low,
    /// 120hz tick for frame cap 61-120
    Medium,
    /// 240hz tick for frame cap 121+
    High,
}

impl TickRate {
    /// get the tick interval for this rate
    #[must_use]
    pub fn interval(&self) -> Duration {
        match self {
            Self::Low => Duration::from_secs_f64(1.0 / 60.0),
            Self::Medium => Duration::from_secs_f64(1.0 / 120.0),
            Self::High => Duration::from_secs_f64(1.0 / 240.0),
        }
    }

    /// determine tick rate from frame cap
    #[must_use]
    pub const fn from_frame_cap(frame_cap: u32) -> Self {
        match frame_cap {
            0..=60 => Self::Low,
            61..=120 => Self::Medium,
            _ => Self::High,
        }
    }
}

/// game loop configuration and state.
///
/// manages the fixed timestep accumulator and frame rate limiting.
/// call [`GameLoop::tick`] each frame to get the number of ECS ticks to run.
pub struct GameLoop {
    /// target frame cap (0 = uncapped)
    frame_cap: u32,
    /// current tick rate
    tick_rate: TickRate,
    /// accumulator for fixed timestep
    accumulator: Duration,
    /// last frame time
    last_frame: Instant,
    /// whether the loop should continue
    running: bool,
}

impl GameLoop {
    /// create a new game loop with the given frame cap
    #[must_use]
    pub fn new(frame_cap: u32) -> Self {
        let tick_rate = TickRate::from_frame_cap(frame_cap);
        log::info!("game loop initialized: frame_cap={frame_cap}, tick_rate={tick_rate:?}");
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

    /// set the frame cap, tick rate will update automatically
    pub fn set_frame_cap(&mut self, frame_cap: u32) {
        self.frame_cap = frame_cap;
        self.tick_rate = TickRate::from_frame_cap(frame_cap);
        log::info!(
            "frame cap changed to {}, tick_rate={:?}",
            frame_cap,
            self.tick_rate
        );
    }

    /// get the current tick rate
    #[must_use]
    pub const fn tick_rate(&self) -> TickRate {
        self.tick_rate
    }

    /// check if the loop should continue
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// stop the game loop
    pub const fn stop(&mut self) {
        self.running = false;
    }

    /// advance the loop by one frame
    /// returns the number of ticks that should be processed this frame
    pub fn tick(&mut self) -> u32 {
        let now = Instant::now();
        let delta = now - self.last_frame;
        self.last_frame = now;

        // frame_cap=0 means vsync inside the render stage acts as the natural limiter.
        // always return exactly 1 tick — the accumulator would just add jitter.
        if self.frame_cap == 0 {
            return 1;
        }

        self.accumulator += delta;

        let tick_interval = self.tick_rate.interval();
        let mut ticks = 0;

        while self.accumulator >= tick_interval {
            self.accumulator -= tick_interval;
            ticks += 1;
        }

        // cap ticks to prevent spiral of death
        ticks.min(5)
    }

    /// apply frame rate limiting if frame cap is set.
    /// uses a hybrid approach: sleep for most of the wait time, then spin-wait
    /// the last ~1ms for better precision and reduced frame pacing jitter.
    ///
    /// # Panics
    /// panics if the elapsed time exceeds the frame duration unexpectedly during frame cap calculation.
    pub fn apply_frame_cap(&self) {
        if self.frame_cap == 0 {
            return;
        }

        let frame_duration = Duration::from_secs_f64(1.0 / f64::from(self.frame_cap));
        let elapsed = self.last_frame.elapsed();

        if elapsed < frame_duration {
            let remaining = frame_duration.checked_sub(elapsed).unwrap();
            // sleep for all but the last 1ms, then spin-wait for precision
            if remaining > Duration::from_millis(1) {
                std::thread::sleep(remaining.checked_sub(Duration::from_millis(1)).unwrap());
            }
            // spin-wait the remaining time
            while self.last_frame.elapsed() < frame_duration {
                std::hint::spin_loop();
            }
        }
    }
}
