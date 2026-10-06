//! Little-endian field readers shared by the wire decoder and the tlog reader.
//!
//! These replace the `slice[a..b].try_into().unwrap()` spelling of the same
//! operation. Callers must already have validated that the buffer is long enough:
//! the helpers panic on a short buffer with exactly the same reachability as
//! before, so switching to them cannot change behaviour. Keeping the bounds
//! arithmetic in one place is what makes a future checked (`Result`-returning)
//! variant a one-file change instead of a 68-site change.

/// Reads a little-endian `u16` at `offset`.
///
/// # Panics
///
/// Panics if `buf.len() < offset + 2`.
#[inline]
#[must_use]
pub fn le_u16(buf: &[u8], offset: usize) -> u16 {
    let mut raw = [0u8; 2];
    raw.copy_from_slice(&buf[offset..offset + 2]);
    u16::from_le_bytes(raw)
}

/// Reads a little-endian `u32` at `offset`.
///
/// # Panics
///
/// Panics if `buf.len() < offset + 4`.
#[inline]
#[must_use]
pub fn le_u32(buf: &[u8], offset: usize) -> u32 {
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&buf[offset..offset + 4]);
    u32::from_le_bytes(raw)
}

/// Reads a little-endian `u64` at `offset`.
///
/// # Panics
///
/// Panics if `buf.len() < offset + 8`.
#[inline]
#[must_use]
pub fn le_u64(buf: &[u8], offset: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&buf[offset..offset + 8]);
    u64::from_le_bytes(raw)
}

/// Reads a little-endian `i32` at `offset`.
///
/// # Panics
///
/// Panics if `buf.len() < offset + 4`.
#[inline]
#[must_use]
pub fn le_i32(buf: &[u8], offset: usize) -> i32 {
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&buf[offset..offset + 4]);
    i32::from_le_bytes(raw)
}

/// Reads a little-endian `i64` at `offset`.
///
/// # Panics
///
/// Panics if `buf.len() < offset + 8`.
#[inline]
#[must_use]
pub fn le_i64(buf: &[u8], offset: usize) -> i64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&buf[offset..offset + 8]);
    i64::from_le_bytes(raw)
}

/// Reads a little-endian `f64` at `offset`.
///
/// # Panics
///
/// Panics if `buf.len() < offset + 8`.
#[inline]
#[must_use]
pub fn le_f64(buf: &[u8], offset: usize) -> f64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&buf[offset..offset + 8]);
    f64::from_le_bytes(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readers_decode_little_endian() {
        let buf: Vec<u8> = (0u8..16).collect();
        assert_eq!(le_u16(&buf, 0), 0x0100);
        assert_eq!(le_u32(&buf, 4), 0x0706_0504);
        assert_eq!(le_u64(&buf, 8), 0x0F0E_0D0C_0B0A_0908);
        assert_eq!(le_i32(&buf, 0), 0x0302_0100);
        assert_eq!(le_f64(&buf, 0), f64::from_le_bytes([0, 1, 2, 3, 4, 5, 6, 7]));
    }

    #[test]
    #[should_panic]
    fn test_reader_panics_on_short_buffer() {
        let buf = [0u8; 3];
        let _ = le_u32(&buf, 0);
    }
}
