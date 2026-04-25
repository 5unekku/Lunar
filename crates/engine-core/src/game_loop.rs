//! game loop
//!
//! fixed tickrate correlated to frame cap with three buckets:
//! - frame cap 1-60: 60hz tick
//! - frame cap 61-120: 120hz tick
//! - frame cap 121+: 240hz tick, ceiling regardless of frame cap
//!
//! rendering runs uncapped and should feel smooth at high framerates.

use std::time::{Duration, Instant};

/// tick rate buckets based on frame cap
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
    pub fn interval(&self) -> Duration {
        match self {
            TickRate::Low => Duration::from_secs_f64(1.0 / 60.0),
            TickRate::Medium => Duration::from_secs_f64(1.0 / 120.0),
            TickRate::High => Duration::from_secs_f64(1.0 / 240.0),
        }
    }

    /// determine tick rate from frame cap
    pub fn from_frame_cap(frame_cap: u32) -> Self {
        match frame_cap {
            0..=60 => TickRate::Low,
            61..=120 => TickRate::Medium,
            _ => TickRate::High,
        }
    }
}

/// game loop configuration and state
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
    pub fn new(frame_cap: u32) -> Self {
        let tick_rate = TickRate::from_frame_cap(frame_cap);
        log::info!(
            "game loop initialized: frame_cap={}, tick_rate={:?}",
            frame_cap,
            tick_rate
        );
        GameLoop {
            frame_cap,
            tick_rate,
            accumulator: Duration::ZERO,
            last_frame: Instant::now(),
            running: true,
        }
    }

    /// get the current frame cap
    pub fn frame_cap(&self) -> u32 {
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
    pub fn tick_rate(&self) -> TickRate {
        self.tick_rate
    }

    /// check if the loop should continue
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// stop the game loop
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// advance the loop by one frame
    /// returns the number of ticks that should be processed this frame
    pub fn tick(&mut self) -> u32 {
        let now = Instant::now();
        let delta = now - self.last_frame;
        self.last_frame = now;

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

    /// apply frame rate limiting if frame cap is set
    pub fn apply_frame_cap(&self) {
        if self.frame_cap == 0 {
            return;
        }

        let frame_duration = Duration::from_secs_f64(1.0 / self.frame_cap as f64);
        let elapsed = self.last_frame.elapsed();

        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }
}
