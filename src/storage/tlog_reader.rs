//! Binary log verification reader.

use crate::core::types::{LogRecord, RunId, BrokerId, MonoNs, SequenceDisposition, LogRawFrame, LogMetadata, DiagnosticSeverity, Diagnostic};
use crate::protocol::bytes::{le_i64, le_u16, le_u32, le_u64};
use crate::protocol::crc32c::crc32c;
use crate::storage::error::StorageError;
use crate::storage::tlog_writer::{FILE_HEADER_LEN, STORAGE_MAGIC, STORAGE_VERSION, RECORD_ENVELOPE_LEN, RECORD_KIND_RAW_FRAME, RECORD_KIND_METADATA, RECORD_KIND_DIAGNOSTIC};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

#[derive(Debug, PartialEq)]
pub enum ReadResult {
    Record(LogRecord),
    CleanEof,
    TruncatedTail(usize),
    Corrupt(String),
}

pub struct LogFileReader<R: Read + Seek> {
    reader: R,
    run_id: RunId,
    file_id: u64,
    broker_id: BrokerId,
    current_index: u64,
}

impl<R: Read + Seek> LogFileReader<R> {
    /// Read and validate a table-log file header.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Io`] when the header cannot be read, [`StorageError::InvalidMagic`]
    /// when the file is not a tlog, [`StorageError::UnsupportedVersion`] for an unknown storage
    /// version, and [`StorageError::InvalidHeaderLength`] when the declared header size is wrong.
    pub fn new(mut reader: R) -> Result<Self, StorageError> {
        let mut hdr_buf = [0u8; FILE_HEADER_LEN];
        reader.read_exact(&mut hdr_buf)?;

        if hdr_buf[0..4] != STORAGE_MAGIC {
            return Err(StorageError::InvalidMagic);
        }

        let version = le_u16(&hdr_buf, 4);
        if version != STORAGE_VERSION {
            return Err(StorageError::UnsupportedVersion(version));
        }

        let header_len = le_u16(&hdr_buf, 6);
        if header_len as usize != FILE_HEADER_LEN {
            return Err(StorageError::InvalidHeaderLength(header_len));
        }

        let mut run_id_bytes = [0u8; 16];
        run_id_bytes.copy_from_slice(&hdr_buf[8..24]);
        let file_id = le_u64(&hdr_buf, 24);
        let broker_id = le_u32(&hdr_buf, 32);

        Ok(Self {
            reader,
            run_id: RunId(run_id_bytes),
            file_id,
            broker_id,
            current_index: 0,
        })
    }

    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    pub const fn file_id(&self) -> u64 {
        self.file_id
    }

    pub const fn broker_id(&self) -> BrokerId {
        self.broker_id
    }

    pub fn next_record(&mut self) -> ReadResult {
        let mut envelope = [0u8; RECORD_ENVELOPE_LEN];
        match self.reader.read_exact(&mut envelope) {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // If 0 bytes were read, it's clean EOF!
                let current_pos = self.reader.stream_position().unwrap_or(0);
                let end_pos = self.reader.seek(SeekFrom::End(0)).unwrap_or(0);
                if current_pos == end_pos {
                    return ReadResult::CleanEof;
                }
                return ReadResult::TruncatedTail((end_pos - current_pos) as usize);
            }
            Err(e) => return ReadResult::Corrupt(format!("I/O error reading envelope: {e}")),
        }

        let total_length = le_u32(&envelope, 0) as usize;
        let kind = le_u16(&envelope, 4);
        let flags = le_u16(&envelope, 6);
        let record_index = le_u64(&envelope, 8);
        let stored_crc = le_u32(&envelope, 16);

        if total_length < RECORD_ENVELOPE_LEN {
            return ReadResult::Corrupt(format!(
                "total_length {total_length} is smaller than envelope header {RECORD_ENVELOPE_LEN}"
            ));
        }

        let payload_len = total_length - RECORD_ENVELOPE_LEN;
        let mut payload = vec![0u8; payload_len];
        if let Err(e) = self.reader.read_exact(&mut payload) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return ReadResult::TruncatedTail(payload_len);
            }
            return ReadResult::Corrupt(format!("I/O error reading record payload: {e}"));
        }

        // Verify CRC32C: envelope with checksum 0 + payload
        let mut crc_check_buf = Vec::with_capacity(total_length);
        crc_check_buf.extend_from_slice(&envelope);
        crc_check_buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // Zero out checksum field
        crc_check_buf.extend_from_slice(&payload);

        let calculated_crc = crc32c(&crc_check_buf);
        if calculated_crc != stored_crc {
            return ReadResult::Corrupt(format!(
                "CRC32C mismatch at record {record_index}: stored 0x{stored_crc:08X}, calculated 0x{calculated_crc:08X}"
            ));
        }

        // Decode payload
        match kind {
            RECORD_KIND_RAW_FRAME => {
                if payload.len() < 60 {
                    return ReadResult::Corrupt("RawFrame payload too short".to_string());
                }
                let broker_id = le_u32(&payload, 0);
                if self.broker_id != 0 && broker_id != self.broker_id {
                    return ReadResult::Corrupt(format!(
                        "RawFrame broker {} does not match file broker {}",
                        broker_id, self.broker_id
                    ));
                }
                let connection_generation = le_u64(&payload, 4);
                let frame_index = le_u64(&payload, 12);
                let rx_mono_ns = MonoNs(le_u64(&payload, 20));
                let rx_unix = le_i64(&payload, 28);
                let rx_unix_ns = if (flags & 1) != 0 { Some(rx_unix) } else { None };
                let config_epoch = le_u64(&payload, 36);
                let analysis_segment = le_u64(&payload, 44);
                let wire_length = le_u32(&payload, 52) as usize;
                let disp_count = le_u32(&payload, 56) as usize;

                if payload.len() < 60 + wire_length + disp_count {
                    return ReadResult::Corrupt("RawFrame wire/dispositions truncated".to_string());
                }

                let raw_wire_bytes = payload[60..60 + wire_length].to_vec();
                let mut dispositions = Vec::with_capacity(disp_count);
                for &b in &payload[60 + wire_length..60 + wire_length + disp_count] {
                    let d = match b {
                        0 => SequenceDisposition::New,
                        1 => SequenceDisposition::DuplicateExact,
                        2 => SequenceDisposition::IdentityConflict,
                        _ => SequenceDisposition::OutOfOrderUnverified,
                    };
                    dispositions.push(d);
                }

                self.current_index += 1;
                ReadResult::Record(LogRecord::RawFrame(LogRawFrame {
                    broker_id,
                    connection_generation,
                    frame_index,
                    rx_mono_ns,
                    rx_unix_ns,
                    config_epoch,
                    analysis_segment,
                    raw_wire_bytes: Arc::new(raw_wire_bytes),
                    dispositions,
                }))
            }
            RECORD_KIND_METADATA => {
                if payload.len() < 20 {
                    return ReadResult::Corrupt("Metadata payload too short".to_string());
                }
                let config_epoch = le_u64(&payload, 0);
                let observed_mono_ns = MonoNs(le_u64(&payload, 8));
                let text_len = le_u32(&payload, 16) as usize;
                if payload.len() < 20 + text_len {
                    return ReadResult::Corrupt("Metadata text truncated".to_string());
                }
                let toml_text = String::from_utf8_lossy(&payload[20..20 + text_len]).to_string();

                self.current_index += 1;
                ReadResult::Record(LogRecord::Metadata(LogMetadata {
                    config_epoch,
                    observed_mono_ns,
                    toml_text,
                }))
            }
            RECORD_KIND_DIAGNOSTIC => {
                if payload.len() < 64 {
                    return ReadResult::Corrupt("Diagnostic payload too short".to_string());
                }
                let mono_ns = MonoNs(le_u64(&payload, 0));
                let broker_id = le_u32(&payload, 8);
                if self.broker_id != 0 && broker_id != self.broker_id {
                    return ReadResult::Corrupt(format!(
                        "Diagnostic broker {} does not match file broker {}",
                        broker_id, self.broker_id
                    ));
                }
                let severity_num = le_u16(&payload, 12);
                let d_flags = le_u16(&payload, 14);
                let session_raw = le_u64(&payload, 16);
                let session_id = if (d_flags & (1 << 0)) != 0 { Some(session_raw) } else { None };

                let first = le_u64(&payload, 24);
                let last = le_u64(&payload, 32);
                let sequence_range = if (d_flags & (1 << 1)) != 0 { Some((first, last)) } else { None };

                let count_raw = le_u64(&payload, 40);
                let known_count = if (d_flags & (1 << 2)) != 0 { Some(count_raw) } else { None };
                let detail_value = le_i64(&payload, 48);
                let code_len = le_u32(&payload, 56) as usize;
                let msg_len = le_u32(&payload, 60) as usize;

                if payload.len() < 64 + code_len + msg_len {
                    return ReadResult::Corrupt("Diagnostic text truncated".to_string());
                }

                let code = String::from_utf8_lossy(&payload[64..64 + code_len]).to_string();
                let message = String::from_utf8_lossy(&payload[64 + code_len..64 + code_len + msg_len]).to_string();

                let severity = match severity_num {
                    1 => DiagnosticSeverity::Error,
                    2 => DiagnosticSeverity::Warn,
                    3 => DiagnosticSeverity::Info,
                    4 => DiagnosticSeverity::Debug,
                    _ => DiagnosticSeverity::Trace,
                };

                self.current_index += 1;
                ReadResult::Record(LogRecord::Diagnostic(Diagnostic {
                    code,
                    severity,
                    broker_id,
                    session_id,
                    mono_ns,
                    sequence_range,
                    known_count,
                    detail_value,
                    message,
                }))
            }
            _ => ReadResult::Corrupt(format!("Unknown record kind {kind}")),
        }
    }
}
