//! Fake clock implementation for deterministic testing.

use tick_compare::contracts::ports::ClockPort;
use tick_compare::contracts::types::{ClockReading, MonoNs, RunId};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

#[allow(dead_code)]
pub struct FakeClock {
    run_id: RunId,
    mono_ns: AtomicU64,
    unix_ns: AtomicI64,
}

impl FakeClock {
    pub fn new(initial_mono_ns: u64, initial_unix_ns: i64) -> Self {
        Self {
            run_id: RunId([1u8; 16]),
            mono_ns: AtomicU64::new(initial_mono_ns),
            unix_ns: AtomicI64::new(initial_unix_ns),
        }
    }

    pub fn advance_mono_ns(&self, delta: u64) {
        self.mono_ns.fetch_add(delta, Ordering::SeqCst);
    }

    pub fn advance_mono_ms(&self, delta_ms: u64) {
        self.advance_mono_ns(delta_ms * 1_000_000);
    }

    pub fn set_mono_ns(&self, mono: u64) {
        self.mono_ns.store(mono, Ordering::SeqCst);
    }
}

impl ClockPort for FakeClock {
    fn sample(&self) -> ClockReading {
        ClockReading {
            run_id: self.run_id,
            mono_ns: MonoNs(self.mono_ns.load(Ordering::SeqCst)),
            unix_ns: Some(self.unix_ns.load(Ordering::SeqCst)),
        }
    }
}
