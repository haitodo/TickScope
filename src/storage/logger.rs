//! Storage serialization and binary logger worker.
//! Reference: docs/blueprint/storage-format.md

use crate::contracts::crc32c::crc32c;
use crate::contracts::ports::{AppendResult, LogSinkPort};
use crate::contracts::types::*;
use crossbeam_channel::{bounded, Receiver, Sender};
use std::fs::{create_dir_all, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const STORAGE_MAGIC: [u8; 4] = [0x54, 0x4C, 0x4F, 0x47]; // TLOG
pub const STORAGE_VERSION: u16 = 1;
pub const FILE_HEADER_LEN: usize = 56;
pub const RECORD_ENVELOPE_LEN: usize = 24;

pub const RECORD_KIND_RAW_FRAME: u16 = 1;
pub const RECORD_KIND_METADATA: u16 = 2;
pub const RECORD_KIND_DIAGNOSTIC: u16 = 3;

pub fn encode_record(
    record: &LogRecord,
    record_index: u64,
) -> Result<Vec<u8>, String> {
    let mut payload = Vec::new();
    let (kind, flags) = match record {
        LogRecord::RawFrame(raw) => {
            let flags: u16 = if raw.rx_unix_ns.is_some() { 1 } else { 0 };
            payload.extend_from_slice(&raw.broker_id.to_le_bytes());
            payload.extend_from_slice(&raw.connection_generation.to_le_bytes());
            payload.extend_from_slice(&raw.frame_index.to_le_bytes());
            payload.extend_from_slice(&raw.rx_mono_ns.0.to_le_bytes());
            payload.extend_from_slice(&raw.rx_unix_ns.unwrap_or(0).to_le_bytes());
            payload.extend_from_slice(&raw.config_epoch.to_le_bytes());
            payload.extend_from_slice(&raw.analysis_segment.to_le_bytes());
            payload.extend_from_slice(&(raw.raw_wire_bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(&(raw.dispositions.len() as u32).to_le_bytes());
            payload.extend_from_slice(&raw.raw_wire_bytes);
            for d in &raw.dispositions {
                payload.push(*d as u8);
            }
            (RECORD_KIND_RAW_FRAME, flags)
        }
        LogRecord::Metadata(meta) => {
            payload.extend_from_slice(&meta.config_epoch.to_le_bytes());
            payload.extend_from_slice(&meta.observed_mono_ns.0.to_le_bytes());
            let text_bytes = meta.toml_text.as_bytes();
            payload.extend_from_slice(&(text_bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(text_bytes);
            (RECORD_KIND_METADATA, 0)
        }
        LogRecord::Diagnostic(diag) => {
            let mut flags: u16 = 0;
            if diag.session_id.is_some() {
                flags |= 1 << 0;
            }
            if diag.sequence_range.is_some() {
                flags |= 1 << 1;
            }
            if diag.known_count.is_some() {
                flags |= 1 << 2;
            }

            payload.extend_from_slice(&diag.mono_ns.0.to_le_bytes());
            payload.extend_from_slice(&diag.broker_id.to_le_bytes());
            payload.extend_from_slice(&(diag.severity as u16).to_le_bytes());
            payload.extend_from_slice(&flags.to_le_bytes());
            payload.extend_from_slice(&diag.session_id.unwrap_or(0).to_le_bytes());

            let (first, last) = diag.sequence_range.unwrap_or((0, 0));
            payload.extend_from_slice(&first.to_le_bytes());
            payload.extend_from_slice(&last.to_le_bytes());
            payload.extend_from_slice(&diag.known_count.unwrap_or(0).to_le_bytes());
            payload.extend_from_slice(&diag.detail_value.to_le_bytes());

            let code_bytes = diag.code.as_bytes();
            let msg_bytes = diag.message.as_bytes();
            payload.extend_from_slice(&(code_bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(&(msg_bytes.len() as u32).to_le_bytes());
            payload.extend_from_slice(code_bytes);
            payload.extend_from_slice(msg_bytes);
            (RECORD_KIND_DIAGNOSTIC, 0)
        }
    };

    let total_len = (RECORD_ENVELOPE_LEN + payload.len()) as u32;
    let mut envelope = vec![0u8; total_len as usize];

    envelope[0..4].copy_from_slice(&total_len.to_le_bytes());
    envelope[4..6].copy_from_slice(&kind.to_le_bytes());
    envelope[6..8].copy_from_slice(&flags.to_le_bytes());
    envelope[8..16].copy_from_slice(&record_index.to_le_bytes());
    envelope[16..20].copy_from_slice(&0u32.to_le_bytes()); // Checksum = 0 during calculation
    envelope[20..24].copy_from_slice(&0u32.to_le_bytes()); // Reserved

    envelope[RECORD_ENVELOPE_LEN..].copy_from_slice(&payload);

    // Calculate CRC32C over envelope (with checksum=0) + payload
    let checksum = crc32c(&envelope);
    envelope[16..20].copy_from_slice(&checksum.to_le_bytes());

    Ok(envelope)
}

pub fn create_file_header(run_id: RunId, file_id: u64, broker_id: BrokerId) -> [u8; FILE_HEADER_LEN] {
    let mut hdr = [0u8; FILE_HEADER_LEN];
    hdr[0..4].copy_from_slice(&STORAGE_MAGIC);
    hdr[4..6].copy_from_slice(&STORAGE_VERSION.to_le_bytes());
    hdr[6..8].copy_from_slice(&(FILE_HEADER_LEN as u16).to_le_bytes());
    hdr[8..24].copy_from_slice(&run_id.0);
    hdr[24..32].copy_from_slice(&file_id.to_le_bytes());
    hdr[32..36].copy_from_slice(&broker_id.to_le_bytes());
    hdr[36..40].copy_from_slice(&0u32.to_le_bytes()); // Flags
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64;
    hdr[40..48].copy_from_slice(&now_ns.to_le_bytes());
    hdr[48..56].copy_from_slice(&0u64.to_le_bytes()); // Reserved
    hdr
}

pub struct AsyncLogger {
    sender: Sender<Arc<LogRecord>>,
    running: Arc<AtomicBool>,
}

impl AsyncLogger {
    pub fn new<P: AsRef<Path>>(
        log_dir: P,
        run_id: RunId,
        capacity: usize,
        flush_interval_ms: u64,
    ) -> Result<Self, String> {
        create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log dir: {}", e))?;

        let (sender, receiver) = bounded(capacity);
        let running = Arc::new(AtomicBool::new(true));

        let r_clone = running.clone();
        let log_dir_buf = log_dir.as_ref().to_path_buf();

        thread::spawn(move || {
            Self::worker_loop(receiver, r_clone, log_dir_buf, run_id, flush_interval_ms);
        });

        Ok(Self { sender, running })
    }

    fn worker_loop(
        receiver: Receiver<Arc<LogRecord>>,
        running: Arc<AtomicBool>,
        log_dir: PathBuf,
        run_id: RunId,
        flush_interval_ms: u64,
    ) {
        let file_path = log_dir.join(format!("ticks_{}.tlog", hex::encode(&run_id.0[0..4])));
        let file = match OpenOptions::new().create(true).append(true).open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open log file {:?}: {}", file_path, e);
                return;
            }
        };

        let mut writer = BufWriter::with_capacity(65536, file);
        let header = create_file_header(run_id, 1, 0);
        let _ = writer.write_all(&header);
        let _ = writer.flush();

        let mut record_index: u64 = 0;
        let mut last_flush = std::time::Instant::now();
        let flush_interval = Duration::from_millis(flush_interval_ms);

        while running.load(Ordering::SeqCst) || !receiver.is_empty() {
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(rec) => {
                    if let Ok(bytes) = encode_record(&rec, record_index) {
                        if writer.write_all(&bytes).is_ok() {
                            record_index += 1;
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }

            if last_flush.elapsed() >= flush_interval {
                let _ = writer.flush();
                last_flush = std::time::Instant::now();
            }
        }

        let _ = writer.flush();
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl LogSinkPort for AsyncLogger {
    fn try_append(&self, record: Arc<LogRecord>) -> AppendResult<Arc<LogRecord>> {
        match self.sender.try_send(record) {
            Ok(_) => AppendResult::Accepted,
            Err(crossbeam_channel::TrySendError::Full(r)) => AppendResult::Full(r),
            Err(crossbeam_channel::TrySendError::Disconnected(r)) => {
                AppendResult::Fault(r, "Logger channel closed".to_string())
            }
        }
    }

    fn flush(&self) -> Result<(), String> {
        Ok(())
    }
}

mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
