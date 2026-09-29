//! Test support modules and fakes.

#![allow(unused_imports, dead_code)]

pub mod fake_clock;
pub mod fake_sink;

pub use fake_clock::FakeClock;
pub use fake_sink::{FakeIngressSink, FakeLogSink};
