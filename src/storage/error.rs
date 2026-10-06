//! Storage errors.

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("Invalid storage file magic, expected TLOG")]
    InvalidMagic,

    #[error("Unsupported storage version: {0}")]
    UnsupportedVersion(u16),

    #[error("Invalid file header length: {0}")]
    InvalidHeaderLength(u16),

    #[error("Record CRC32C mismatch: expected {expected:#x}, got {actual:#x}")]
    CrcMismatch { expected: u32, actual: u32 },

    #[error("Corrupt storage data: {0}")]
    Corrupt(String),

    #[error("Log queue is full")]
    QueueFull,

    #[error("Storage fault: {0}")]
    Fault(String),

    #[error("Invalid storage capacity: {0}")]
    InvalidCapacity(String),
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<StorageError> for String {
    fn from(e: StorageError) -> Self {
        e.to_string()
    }
}
