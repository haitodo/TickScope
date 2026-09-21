//! Castagnoli CRC32C implementation as required by docs/blueprint/storage-format.md.
//! Polynomial: 0x82F63B78 (reversed), Initial: 0xFFFFFFFF, Final XOR: 0xFFFFFFFF.
//! Test vector: b"123456789" -> 0xE3069283

const CRC32C_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x82F63B78;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let index = ((crc ^ (byte as u32)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32C_TABLE[index];
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32c_standard_vector() {
        let input = b"123456789";
        let res = crc32c(input);
        assert_eq!(res, 0xE3069283, "CRC32C must match standard Castagnoli vector");
    }
}
