//! Backward-compatibility bridge for TickScope contracts.
//!
//! Prefer importing directly from `crate::core`, `crate::config`, or `crate::protocol`.

pub mod config {
    pub use crate::config::*;
}
pub mod crc32c {
    pub use crate::protocol::crc32c::*;
}
pub mod models {
    pub use crate::core::models::*;
}
pub mod ports {
    pub use crate::core::ports::*;
}
pub mod types {
    pub use crate::core::types::*;
}

pub use config::*;
pub use crc32c::*;
pub use models::*;
pub use ports::*;
pub use types::*;
