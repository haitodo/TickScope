//! Virtual time clock for historical tick replay.
//! Implements `ClockPort` to allow transparent injection into `TickScope`'s runtime.

use crate::core::ports::ClockPort;
use crate::core::types::{ClockReading, MonoNs, RunId};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
struct ClockState {
    /// Current virtual UTC timestamp in milliseconds.
    virtual_utc_ms: i64,
    /// Virtual monotonic timestamp in nanoseconds.
    virtual_mono_ns: u64,
    /// Whether replay playback is actively advancing.
    is_playing: bool,
    /// Playback speed multiplier (e.g. 1.0, 2.0, 5.0, 10.0).
    multiplier: f64,
    /// Real-world Instant when the state was last updated or sampled.
    last_real_instant: Instant,
}

/// A high-precision virtual clock that simulates time progression for historical replay.
///
/// When paused (`is_playing == false`), the clock is completely frozen:
/// successive samples return the exact same timestamp, ensuring that broker quotes
/// do not turn stale and health states remain `Live`.
#[derive(Clone)]
pub struct VirtualClock {
    run_id: RunId,
    state: Arc<RwLock<ClockState>>,
}

impl VirtualClock {
    pub fn new(run_id: RunId, initial_utc_ms: i64) -> Self {
        Self {
            run_id,
            state: Arc::new(RwLock::new(ClockState {
                virtual_utc_ms: initial_utc_ms,
                virtual_mono_ns: (initial_utc_ms.max(0) as u64).saturating_mul(1_000_000),
                is_playing: false,
                multiplier: 1.0,
                last_real_instant: Instant::now(),
            })),
        }
    }

    /// Set virtual UTC timestamp immediately (e.g. upon SEEK or rewind).
    pub fn set_time(&self, utc_ms: i64) {
        let mut state = self.state.write();
        state.virtual_utc_ms = utc_ms;
        state.virtual_mono_ns = (utc_ms.max(0) as u64).saturating_mul(1_000_000);
        state.last_real_instant = Instant::now();
    }

    /// Toggle play / pause state.
    /// When set to false, virtual time freezes instantly.
    pub fn set_playing(&self, playing: bool) {
        let mut state = self.state.write();
        if state.is_playing != playing {
            if playing {
                state.last_real_instant = Instant::now();
            }
            state.is_playing = playing;
        }
    }

    /// Update the playback speed multiplier.
    pub fn set_multiplier(&self, multiplier: f64) {
        let mut state = self.state.write();
        if (state.multiplier - multiplier).abs() > 1e-6 {
            // Apply elapsed before changing speed
            if state.is_playing {
                let now = Instant::now();
                let elapsed = now.duration_since(state.last_real_instant);
                let virtual_advance_ns = (elapsed.as_nanos() as f64 * state.multiplier) as u64;
                state.virtual_mono_ns = state.virtual_mono_ns.saturating_add(virtual_advance_ns);
                state.virtual_utc_ms = (state.virtual_mono_ns / 1_000_000) as i64;
                state.last_real_instant = now;
            }
            state.multiplier = multiplier.max(0.01);
        }
    }

    /// Smoothly advance virtual time to a target UTC millisecond timestamp.
    pub fn advance_to(&self, target_utc_ms: i64) {
        let mut state = self.state.write();
        if target_utc_ms >= state.virtual_utc_ms {
            state.virtual_utc_ms = target_utc_ms;
            state.virtual_mono_ns = (target_utc_ms.max(0) as u64).saturating_mul(1_000_000);
            state.last_real_instant = Instant::now();
        }
    }

    /// Synchronize phase with an authoritative external clock (e.g. `TickReplay`).
    /// Locks phase to the authoritative master clock to eliminate cumulative drift during playback.
    /// Absorbs drift within the forward threshold (±5000ms), maintaining strict sync with `TickReplay`.
    pub fn sync_phase(&self, target_utc_ms: i64) {
        let mut state = self.state.write();
        if !state.is_playing {
            state.virtual_utc_ms = target_utc_ms;
            state.virtual_mono_ns = (target_utc_ms.max(0) as u64).saturating_mul(1_000_000);
            state.last_real_instant = Instant::now();
            return;
        }

        // Apply elapsed real time first to evaluate true drift at this moment
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_real_instant);
        let advance_ns = (elapsed.as_nanos() as f64 * state.multiplier) as u64;
        state.virtual_mono_ns = state.virtual_mono_ns.saturating_add(advance_ns);
        state.virtual_utc_ms = (state.virtual_mono_ns / 1_000_000) as i64;
        state.last_real_instant = now;

        let drift_ms = target_utc_ms - state.virtual_utc_ms;
        if drift_ms.abs() <= 5000 {
            state.virtual_utc_ms = target_utc_ms;
            state.virtual_mono_ns = (target_utc_ms.max(0) as u64).saturating_mul(1_000_000);
        }
    }

    /// Get current virtual UTC millisecond timestamp.
    pub fn current_utc_ms(&self) -> i64 {
        let state = self.state.read();
        if state.is_playing {
            let elapsed = state.last_real_instant.elapsed();
            let advance_ms = (elapsed.as_secs_f64() * 1000.0 * state.multiplier) as i64;
            state.virtual_utc_ms.saturating_add(advance_ms)
        } else {
            state.virtual_utc_ms
        }
    }

    /// Check if replay is currently playing.
    pub fn is_playing(&self) -> bool {
        self.state.read().is_playing
    }

    /// Current speed multiplier.
    pub fn multiplier(&self) -> f64 {
        self.state.read().multiplier
    }
}

impl ClockPort for VirtualClock {
    fn sample(&self) -> ClockReading {
        let mut state = self.state.write();
        if state.is_playing {
            let now = Instant::now();
            let elapsed = now.duration_since(state.last_real_instant);
            let advance_ns = (elapsed.as_nanos() as f64 * state.multiplier) as u64;
            state.virtual_mono_ns = state.virtual_mono_ns.saturating_add(advance_ns);
            state.virtual_utc_ms = (state.virtual_mono_ns / 1_000_000) as i64;
            state.last_real_instant = now;
        }

        ClockReading {
            run_id: self.run_id,
            mono_ns: MonoNs(state.virtual_mono_ns),
            unix_ns: Some(state.virtual_utc_ms.saturating_mul(1_000_000)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_virtual_clock_pause_freezes_time() {
        let clock = VirtualClock::new(RunId::new_random(), 1_700_000_000_000);
        let sample1 = clock.sample();
        thread::sleep(Duration::from_millis(50));
        let sample2 = clock.sample();

        // While paused, monotonic time and unix time must be completely frozen!
        assert_eq!(sample1.mono_ns, sample2.mono_ns);
        assert_eq!(sample1.unix_ns, sample2.unix_ns);
    }

    #[test]
    fn test_virtual_clock_advances_when_playing() {
        let clock = VirtualClock::new(RunId::new_random(), 1_700_000_000_000);
        clock.set_playing(true);
        clock.set_multiplier(2.0);

        let sample1 = clock.sample();
        thread::sleep(Duration::from_millis(50));
        let sample2 = clock.sample();

        assert!(sample2.mono_ns > sample1.mono_ns);
        assert!(sample2.unix_ns.unwrap() > sample1.unix_ns.unwrap());
    }

    #[test]
    fn test_virtual_clock_set_time_seek() {
        let clock = VirtualClock::new(RunId::new_random(), 1_700_000_000_000);
        clock.set_time(1_800_000_000_000);
        let sample = clock.sample();
        assert_eq!(sample.unix_ns.unwrap(), 1_800_000_000_000 * 1_000_000);
    }
}
