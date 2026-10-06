//! UI implementation using egui and Painter.

pub mod chart;
pub mod dashboard;
pub mod dpi;
pub mod fonts;
pub mod icon;
pub mod settings;
pub mod shared;
pub(crate) mod style;

#[cfg(test)]
pub(crate) mod test_support;

pub use chart::*;
pub use dashboard::*;
pub use dpi::*;
pub use fonts::*;
pub use icon::*;
pub use settings::*;
