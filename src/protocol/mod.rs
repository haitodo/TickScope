//! Binary wire protocol codecs, packet framing, and CRC32C checksums.

pub mod codec;
pub mod crc32c;
pub mod packet;

pub use codec::*;
pub use crc32c::*;
pub use packet::*;
