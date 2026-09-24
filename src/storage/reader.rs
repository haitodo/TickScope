//! Binary log verification reader.
//! Reference: docs/blueprint/storage-format.md

use crate::contracts::crc32c::crc32c;
use crate::contracts::types::*;
use crate::storage::logger::*;
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
    pub fn new(mut reader: R) -> Result<Self, String> {
        let mut hdr_buf = [0u8; FILE_HEADER_LEN];
        reader
            .read_exact(&mut hdr_buf)
            .map_err(|e| format!("Failed to read file header: {}", e))?;

        if hdr_buf[0..4] != STORAGE_MAGIC {
            return Err("Invalid file magic, expected TLOG".to_string());
        }

        let version = u16::from_le_bytes(hdr_buf[4..6].try_into().unwrap());
        if version != STORAGE_VERSION {
            return Err(format!("Unsupported storage version: {}", version));
        }

        let header_len = u16::from_le_bytes(hdr_buf[6..8].try_into().unwrap());
        if header_len as usize != FILE_HEADER_LEN {
            return Err(format!("Invalid file header length: {}", header_len));
        }

        let mut run_id_bytes = [0u8; 16];
        run_id_bytes.copy_from_slice(&hdr_buf[8..24]);
        let file_id = u64::from_le_bytes(hdr_buf[24..32].try_into().unwrap());
        let broker_id = u32::from_le_bytes(hdr_buf[32..36].try_into().unwrap());

        Ok(Self {
            reader,
            run_id: RunId(run_id_bytes),
            file_id,
            broker_id,
            current_index: 0,
        })
    }

    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    pub fn file_id(&self) -> u64 {
        self.file_id
    }

    pub fn broker_id(&self) -> BrokerId {
        self.broker_id
    }

    pub fn next_record(&mut self) -> ReadResult {
        let mut envelope = [0u8; RECORD_ENVELOPE_LEN];
        match self.reader.read_exact(&mut envelope) {
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // If 0 bytes were read, it's clean EOF!
                let current_pos = self.reader.stream_position().unwrap_or(0);
                let end_pos = self.reader.seek(SeekFrom::End(0)).unwrap_or(0);
                if current_pos == end_pos {
                    return ReadResult::CleanEof;
                } else {
                    return ReadResult::TruncatedTail((end_pos - current_pos) as usize);
                }
            }
            Err(e) => return ReadResult::Corrupt(format!("I/O error reading envelope: {}", e)),
        }

        let total_length = u32::from_le_bytes(envelope[0..4].try_into().unwrap()) as usize;
        let kind = u16::from_le_bytes(envelope[4..6].try_into().unwrap());
        let flags = u16::from_le_bytes(envelope[6..8].try_into().unwrap());
        let record_index = u64::from_le_bytes(envelope[8..16].try_into().unwrap());
        let stored_crc = u32::from_le_bytes(envelope[16..20].try_into().unwrap());

        if total_length < RECORD_ENVELOPE_LEN {
            return ReadResult::Corrupt(format!(
                "total_length {} is smaller than envelope header {}",
                total_length, RECORD_ENVELOPE_LEN
            ));
        }

        let payload_len = total_length - RECORD_ENVELOPE_LEN;
        let mut payload = vec![0u8; payload_len];
        if let Err(e) = self.reader.read_exact(&mut payload) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return ReadResult::TruncatedTail(payload_len);
            }
            return ReadResult::Corrupt(format!("I/O error reading record payload: {}", e));
        }

        // Verify CRC32C: envelope with checksum 0 + payload
        let mut crc_check_buf = Vec::with_capacity(total_length);
        crc_check_buf.extend_from_slice(&envelope);
        crc_check_buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // Zero out checksum field
        crc_check_buf.extend_from_slice(&payload);

        let calculated_crc = crc32c(&crc_check_buf);
        if calculated_crc != stored_crc {
            return ReadResult::Corrupt(format!(
                "CRC32C mismatch at record {}: stored 0x{:08X}, calculated 0x{:08X}",
                record_index, stored_crc, calculated_crc
            ));
        }

        // Decode payload
        match kind {
            RECORD_KIND_RAW_FRAME => {
                if payload.len() < 60 {
                    return ReadResult::Corrupt("RawFrame payload too short".to_string());
                }
                let broker_id = u32::from_le_bytes(payload[0..4].try_into().unwrap());
                if self.broker_id != 0 && broker_id != self.broker_id {
                    return ReadResult::Corrupt(format!(
                        "RawFrame broker {} does not match file broker {}",
                        broker_id, self.broker_id
                    ));
                }
                let connection_generation = u64::from_le_bytes(payload[4..12].try_into().unwrap());
                let frame_index = u64::from_le_bytes(payload[12..20].try_into().unwrap());
                let rx_mono_ns = MonoNs(u64::from_le_bytes(payload[20..28].try_into().unwrap()));
                let rx_unix = i64::from_le_bytes(payload[28..36].try_into().unwrap());
                let rx_unix_ns = if (flags & 1) != 0 { Some(rx_unix) } else { None };
                let config_epoch = u64::from_le_bytes(payload[36..44].try_into().unwrap());
                let analysis_segment = u64::from_le_bytes(payload[44..52].try_into().unwrap());
                let wire_length = u32::from_le_bytes(payload[52..56].try_into().unwrap()) as usize;
                let disp_count = u32::from_le_bytes(payload[56..60].try_into().unwrap()) as usize;

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
                let config_epoch = u64::from_le_bytes(payload[0..8].try_into().unwrap());
                let observed_mono_ns = MonoNs(u64::from_le_bytes(payload[8..16].try_into().unwrap()));
                let text_len = u32::from_le_bytes(payload[16..20].try_into().unwrap()) as usize;
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
                let mono_ns = MonoNs(u64::from_le_bytes(payload[0..8].try_into().unwrap()));
                let broker_id = u32::from_le_bytes(payload[8..12].try_into().unwrap());
                if self.broker_id != 0 && broker_id != self.broker_id {
                    return ReadResult::Corrupt(format!(
                        "Diagnostic broker {} does not match file broker {}",
                        broker_id, self.broker_id
                    ));
                }
                let severity_num = u16::from_le_bytes(payload[12..14].try_into().unwrap());
                let d_flags = u16::from_le_bytes(payload[14..16].try_into().unwrap());
                let session_raw = u64::from_le_bytes(payload[16..24].try_into().unwrap());
                let session_id = if (d_flags & (1 << 0)) != 0 { Some(session_raw) } else { None };

                let first = u64::from_le_bytes(payload[24..32].try_into().unwrap());
                let last = u64::from_le_bytes(payload[32..40].try_into().unwrap());
                let sequence_range = if (d_flags & (1 << 1)) != 0 { Some((first, last)) } else { None };

                let count_raw = u64::from_le_bytes(payload[40..48].try_into().unwrap());
                let known_count = if (d_flags & (1 << 2)) != 0 { Some(count_raw) } else { None };
                let detail_value = i64::from_le_bytes(payload[48..56].try_into().unwrap());
                let code_len = u32::from_le_bytes(payload[56..60].try_into().unwrap()) as usize;
                let msg_len = u32::from_le_bytes(payload[60..64].try_into().unwrap()) as usize;

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
            _ => ReadResult::Corrupt(format!("Unknown record kind {}", kind)),
        }
    }
}
