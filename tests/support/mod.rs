//! Test support modules and fakes.

pub mod fake_clock;
pub mod fake_sink;

pub use fake_clock::FakeClock;
pub use fake_sink::{FakeIngressSink, FakeLogSink};
