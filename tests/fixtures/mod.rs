//! Independent golden test fixtures and oracle vectors.
//! Reference: docs/blueprint/wire-format.md, storage-format.md, validation.md

#![allow(dead_code)]


/// Golden Header 40 bytes for minimal TickBatch:
/// broker=1, session=1, seq=0, tick_count=1, flags=0, payload_len=72
pub const GOLDEN_HEADER_BYTES: [u8; 40] = [
    0x4B, 0x43, 0x49, 0x54, // magic: 0x5449434B
    0x01, 0x00,             // version: 1
    0x01, 0x00,             // msg_type: 1 (TICK_BATCH)
    0x28, 0x00,             // header_len: 40
    0x00, 0x00,             // header_flags: 0
    0x01, 0x00, 0x00, 0x00, // broker_id: 1
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // session_id: 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sequence_start: 0
    0x01, 0x00, 0x00, 0x00, // tick_count: 1
    0x48, 0x00, 0x00, 0x00, // payload_length: 72
];

/// Golden TickRecord 72 bytes:
/// seq=0, time_msc=1000, ea_elapsed_us=10, bid=1.0, ask=2.0, last=0, volume=1, volume_real=1.0, flags=0, reserved=0
pub const GOLDEN_TICK_RECORD_BYTES: [u8; 72] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sequence: 0
    0xE8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // broker_time_msc: 1000
    0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // ea_elapsed_us: 10
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // bid: 1.0 (IEEE-754 binary64)
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, // ask: 2.0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // last: 0.0
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // volume: 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // volume_real: 1.0
    0x00, 0x00, 0x00, 0x00,                         // flags: 0
    0x00, 0x00, 0x00, 0x00,                         // reserved: 0
];

/// Standard CRC32C test vector:
/// ASCII "123456789" -> 0xE3069283
pub const CRC32C_INPUT_BYTES: &[u8] = b"123456789";
pub const CRC32C_EXPECTED_U32: u32 = 0xE3069283;

/// Storage File Header Magic: literal bytes "TLOG"
pub const STORAGE_MAGIC_BYTES: [u8; 4] = [0x54, 0x4C, 0x4F, 0x47];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_header_dimensions() {
        assert_eq!(GOLDEN_HEADER_BYTES.len(), 40);
        let magic = u32::from_le_bytes(GOLDEN_HEADER_BYTES[0..4].try_into().unwrap());
        assert_eq!(magic, 0x5449434B);
    }

    #[test]
    fn test_golden_tick_dimensions() {
        assert_eq!(GOLDEN_TICK_RECORD_BYTES.len(), 72);
        let time_msc = i64::from_le_bytes(GOLDEN_TICK_RECORD_BYTES[8..16].try_into().unwrap());
        assert_eq!(time_msc, 1000);
        let bid = f64::from_le_bytes(GOLDEN_TICK_RECORD_BYTES[24..32].try_into().unwrap());
        assert_eq!(bid, 1.0);
        let ask = f64::from_le_bytes(GOLDEN_TICK_RECORD_BYTES[32..40].try_into().unwrap());
        assert_eq!(ask, 2.0);
    }

    #[test]
    fn test_crc32c_vector() {
        let res = tick_compare::contracts::crc32c::crc32c(CRC32C_INPUT_BYTES);
        assert_eq!(res, CRC32C_EXPECTED_U32);
    }
}


