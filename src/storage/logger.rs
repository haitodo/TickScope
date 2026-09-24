//! Storage serialization and binary logger worker.
//! Reference: docs/blueprint/storage-format.md

use crate::contracts::crc32c::crc32c;
use crate::contracts::ports::{AppendResult, LogSinkPort};
use crate::contracts::types::*;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use std::fs::{create_dir_all, OpenOptions};
use std::io::{BufWriter, Write};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
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
    sender: Sender<LogCommand>,
    running: Arc<AtomicBool>,
    queued_bytes: Arc<std::sync::atomic::AtomicUsize>,
    max_queue_bytes: usize,
    fault: Arc<Mutex<Option<String>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct QueuedLogRecord {
    record: Arc<LogRecord>,
    queued_bytes: usize,
    durable_reply: Option<Sender<Result<(), String>>>,
}

enum LogCommand {
    Record(QueuedLogRecord),
    Flush(Sender<Result<(), String>>),
}

struct ActiveLog {
    utc_date: String,
    writer: BufWriter<std::fs::File>,
    next_record_index: u64,
}

fn log_broker_id(record: &LogRecord) -> BrokerId {
    match record {
        LogRecord::RawFrame(raw) => raw.broker_id,
        LogRecord::Diagnostic(diagnostic) => diagnostic.broker_id,
        // Configuration metadata describes the complete run, rather than one feed.
        LogRecord::Metadata(_) => 0,
    }
}

fn open_log_writer(
    log_dir: &Path,
    run_id: RunId,
    file_id: u64,
    broker_id: BrokerId,
    utc_date: &str,
) -> Result<BufWriter<std::fs::File>, String> {
    let broker_dir = if broker_id == 0 {
        "global".to_string()
    } else {
        format!("broker-{broker_id:03}")
    };
    let directory = log_dir.join(utc_date).join(broker_dir);
    create_dir_all(&directory)
        .map_err(|error| format!("failed to create '{}': {error}", directory.display()))?;

    let file_path = directory.join(format!(
        "run_{}_{file_id:04}.tlog",
        hex::encode(&run_id.0),
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file_path)
        .map_err(|error| format!("failed to create '{}': {error}", file_path.display()))?;

    let mut writer = BufWriter::with_capacity(65_536, file);
    writer.write_all(&create_file_header(run_id, file_id, broker_id))
        .map_err(|error| format!("failed to write header for '{}': {error}", file_path.display()))?;
    writer.flush()
        .map_err(|error| format!("failed to flush header for '{}': {error}", file_path.display()))?;
    Ok(writer)
}

fn flush_writer(writer: &mut BufWriter<std::fs::File>, durable: bool) -> Result<(), String> {
    writer.flush().map_err(|error| format!("failed to flush log writer: {error}"))?;
    if durable {
        writer.get_ref().sync_data()
            .map_err(|error| format!("failed to sync log writer: {error}"))?;
    }
    Ok(())
}

/// A conservative upper bound used to enforce the configured in-memory byte
/// budget before a record is admitted to the logger worker.
fn queued_record_bytes(record: &LogRecord) -> usize {
    match record {
        LogRecord::RawFrame(raw) => raw.raw_wire_bytes.len()
            .saturating_add(raw.dispositions.len())
            .saturating_add(128),
        LogRecord::Metadata(metadata) => metadata.toml_text.len().saturating_add(64),
        LogRecord::Diagnostic(diagnostic) => diagnostic.code.len()
            .saturating_add(diagnostic.message.len())
            .saturating_add(128),
    }
}

fn utc_date_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    // Howard Hinnant's civil-date conversion, with 1970-01-01 as day zero.
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

impl AsyncLogger {
    pub fn new<P: AsRef<Path>>(
        log_dir: P,
        run_id: RunId,
        capacity: usize,
        max_queue_bytes: usize,
        flush_interval_ms: u64,
    ) -> Result<Self, String> {
        if capacity == 0 {
            return Err("logger record capacity must be positive".to_string());
        }
        if max_queue_bytes == 0 {
            return Err("logger byte capacity must be positive".to_string());
        }
        create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log dir: {}", e))?;

        let (sender, receiver) = bounded(capacity);
        let running = Arc::new(AtomicBool::new(true));
        let queued_bytes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let fault = Arc::new(Mutex::new(None));

        let r_clone = running.clone();
        let queued_bytes_clone = queued_bytes.clone();
        let fault_clone = fault.clone();
        let log_dir_buf = log_dir.as_ref().to_path_buf();

        let worker = thread::spawn(move || {
            Self::worker_loop(
                receiver,
                r_clone,
                queued_bytes_clone,
                fault_clone,
                log_dir_buf,
                run_id,
                flush_interval_ms,
            );
        });

        Ok(Self {
            sender,
            running,
            queued_bytes,
            max_queue_bytes,
            fault,
            worker: Mutex::new(Some(worker)),
        })
    }

    fn worker_loop(
        receiver: Receiver<LogCommand>,
        running: Arc<AtomicBool>,
        queued_bytes: Arc<std::sync::atomic::AtomicUsize>,
        fault: Arc<Mutex<Option<String>>>,
        log_dir: PathBuf,
        run_id: RunId,
        flush_interval_ms: u64,
    ) {
        let mut writers: HashMap<BrokerId, ActiveLog> = HashMap::new();
        let mut next_file_id = 1_u64;
        let mut last_flush = std::time::Instant::now();
        let flush_interval = Duration::from_millis(flush_interval_ms);

        while running.load(Ordering::SeqCst) || !receiver.is_empty() {
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(LogCommand::Record(command)) => {
                    queued_bytes.fetch_sub(command.queued_bytes, Ordering::SeqCst);
                    let broker_id = log_broker_id(&command.record);
                    let utc_date = utc_date_now();
                    let write_result = (|| -> Result<(), String> {
                        let needs_new_writer = writers.get(&broker_id)
                            .is_none_or(|active| active.utc_date != utc_date);
                        if needs_new_writer {
                            if let Some(mut previous) = writers.remove(&broker_id) {
                                flush_writer(&mut previous.writer, true)?;
                            }
                            let writer = open_log_writer(&log_dir, run_id, next_file_id, broker_id, &utc_date)?;
                            writers.insert(broker_id, ActiveLog {
                                utc_date,
                                writer,
                                next_record_index: 0,
                            });
                            next_file_id = next_file_id.saturating_add(1);
                        }

                        let active = writers.get_mut(&broker_id)
                            .expect("writer was just opened");
                        let bytes = encode_record(&command.record, active.next_record_index)?;
                        active.writer.write_all(&bytes)
                            .map_err(|error| format!("failed to write broker {broker_id} log record: {error}"))?;
                        active.next_record_index = active.next_record_index.saturating_add(1);
                        if command.durable_reply.is_some() {
                            // The reliable receiver only ACKs a source batch after its
                            // raw frame has reached stable storage.
                            flush_writer(&mut active.writer, true)?;
                        }
                        Ok(())
                    })();

                    match write_result {
                        Ok(()) => {
                            if let Some(reply) = command.durable_reply {
                                let _ = reply.send(Ok(()));
                            }
                        }
                        Err(error) => {
                            *fault.lock().expect("logger fault mutex poisoned") = Some(error.clone());
                            running.store(false, Ordering::SeqCst);
                            if let Some(reply) = command.durable_reply {
                                let _ = reply.send(Err(error));
                            }
                            break;
                        }
                    }
                }
                Ok(LogCommand::Flush(reply)) => {
                    let result = writers.values_mut()
                        .try_for_each(|active| flush_writer(&mut active.writer, true));
                    if let Err(error) = &result {
                        *fault.lock().expect("logger fault mutex poisoned") = Some(error.clone());
                        running.store(false, Ordering::SeqCst);
                    }
                    let _ = reply.send(result);
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }

            if last_flush.elapsed() >= flush_interval {
                let flush_result = writers.values_mut()
                    .try_for_each(|active| flush_writer(&mut active.writer, false));
                if let Err(error) = flush_result {
                    *fault.lock().expect("logger fault mutex poisoned") = Some(error);
                    running.store(false, Ordering::SeqCst);
                    break;
                }
                last_flush = std::time::Instant::now();
            }
        }

        if fault.lock().expect("logger fault mutex poisoned").is_none() {
            for active in writers.values_mut() {
                if let Err(error) = flush_writer(&mut active.writer, true) {
                    *fault.lock().expect("logger fault mutex poisoned") = Some(error);
                    break;
                }
            }
        }
    }

    fn current_fault(&self) -> Option<String> {
        self.fault.lock().expect("logger fault mutex poisoned").clone()
    }

    fn reserve_bytes(&self, bytes: usize) -> bool {
        loop {
            let current = self.queued_bytes.load(Ordering::SeqCst);
            let Some(next) = current.checked_add(bytes) else {
                return false;
            };
            if next > self.max_queue_bytes {
                return false;
            }
            if self.queued_bytes.compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return true;
            }
        }
    }

    fn try_enqueue(
        &self,
        record: Arc<LogRecord>,
        durable_reply: Option<Sender<Result<(), String>>>,
    ) -> AppendResult<Arc<LogRecord>> {
        if let Some(reason) = self.current_fault() {
            return AppendResult::Fault(record, reason);
        }
        if !self.running.load(Ordering::SeqCst) {
            return AppendResult::Fault(record, "Logger is stopped".to_string());
        }

        let bytes = queued_record_bytes(&record);
        if !self.reserve_bytes(bytes) {
            return AppendResult::Full(record);
        }

        let command = LogCommand::Record(QueuedLogRecord {
            record,
            queued_bytes: bytes,
            durable_reply,
        });
        match self.sender.try_send(command) {
            Ok(()) => AppendResult::Accepted,
            Err(TrySendError::Full(LogCommand::Record(command))) => {
                self.queued_bytes.fetch_sub(command.queued_bytes, Ordering::SeqCst);
                AppendResult::Full(command.record)
            }
            Err(TrySendError::Disconnected(LogCommand::Record(command))) => {
                self.queued_bytes.fetch_sub(command.queued_bytes, Ordering::SeqCst);
                AppendResult::Fault(command.record, "Logger channel closed".to_string())
            }
            Err(_) => unreachable!("only Record commands are submitted through try_enqueue"),
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn finish(&self) {
        self.stop();
        if let Some(worker) = self.worker.lock().expect("logger worker mutex poisoned").take() {
            let _ = worker.join();
        }
    }
}

impl LogSinkPort for AsyncLogger {
    fn try_append(&self, record: Arc<LogRecord>) -> AppendResult<Arc<LogRecord>> {
        self.try_enqueue(record, None)
    }

    fn append_durable(&self, record: Arc<LogRecord>) -> Result<(), String> {
        loop {
            let (reply_sender, reply_receiver) = bounded(1);
            match self.try_enqueue(record.clone(), Some(reply_sender)) {
                AppendResult::Accepted => {
                    return reply_receiver.recv()
                        .map_err(|_| "Logger worker stopped before durable append completed".to_string())?;
                }
                AppendResult::Full(_) => thread::sleep(Duration::from_millis(1)),
                AppendResult::Fault(_, reason) => return Err(reason),
            }
        }
    }

    fn flush(&self) -> Result<(), String> {
        if let Some(reason) = self.current_fault() {
            return Err(reason);
        }
        let (reply_sender, reply_receiver) = bounded(1);
        loop {
            match self.sender.try_send(LogCommand::Flush(reply_sender.clone())) {
                Ok(()) => break,
                Err(TrySendError::Full(_)) => thread::sleep(Duration::from_millis(1)),
                Err(TrySendError::Disconnected(_)) => {
                    return Err("Logger channel closed".to_string());
                }
            }
        }
        reply_receiver.recv()
            .map_err(|_| "Logger worker stopped before flush completed".to_string())?
    }
}

mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
