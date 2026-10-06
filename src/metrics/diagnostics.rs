//! Opt-in, low-overhead runtime timing samples.
//!
//! Producers submit fixed-size samples to a bounded, non-blocking channel. A
//! dedicated worker computes one-second summaries and writes them to a CSV
//! file. The normal runtime does not create this channel or worker.

use crate::core::types::RunId;
use crate::metrics::latency::LatencyRingBuffer;
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SAMPLE_CHANNEL_CAPACITY: usize = 8192;
const SUMMARY_INTERVAL: Duration = Duration::from_secs(1);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum DiagnosticStage {
    TcpReceiveToIngress = 0,
    RawPersistence = 1,
    TcpReceiveToAck = 2,
    BatchAckWrite = 3,
    IngressToEngine = 4,
    EngineFrameProcessing = 5,
    ProjectionBuild = 6,
    SnapshotBuild = 7,
    SnapshotToUi = 8,
    UiRenderWork = 9,
}

impl DiagnosticStage {
    const ALL: [Self; 10] = [
        Self::TcpReceiveToIngress,
        Self::RawPersistence,
        Self::TcpReceiveToAck,
        Self::BatchAckWrite,
        Self::IngressToEngine,
        Self::EngineFrameProcessing,
        Self::ProjectionBuild,
        Self::SnapshotBuild,
        Self::SnapshotToUi,
        Self::UiRenderWork,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::TcpReceiveToIngress => "tcp_receive_to_ingress",
            Self::RawPersistence => "raw_persistence",
            Self::TcpReceiveToAck => "tcp_receive_to_ack",
            Self::BatchAckWrite => "batch_ack_write",
            Self::IngressToEngine => "ingress_to_engine",
            Self::EngineFrameProcessing => "engine_frame_processing",
            Self::ProjectionBuild => "projection_build",
            Self::SnapshotBuild => "snapshot_build",
            Self::SnapshotToUi => "snapshot_to_ui",
            Self::UiRenderWork => "ui_render_work",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DiagnosticSample {
    stage: DiagnosticStage,
    duration_ns: u64,
}

/// Cloneable producer endpoint. Recording never blocks and never formats text.
#[derive(Clone)]
pub struct DiagnosticsHandle {
    sender: Sender<DiagnosticSample>,
    dropped_samples: Arc<AtomicU64>,
}

impl DiagnosticsHandle {
    #[inline]
    pub fn record_ns(&self, stage: DiagnosticStage, duration_ns: u64) {
        if self
            .sender
            .try_send(DiagnosticSample { stage, duration_ns })
            .is_err()
        {
            self.dropped_samples.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_duration(&self, stage: DiagnosticStage, duration: Duration) {
        self.record_ns(stage, duration.as_nanos().min(u128::from(u64::MAX)) as u64);
    }
}

/// Owns the opt-in diagnostics file and summary worker for one runtime run.
pub struct DiagnosticsRuntime {
    handle: DiagnosticsHandle,
    stop_sender: Sender<()>,
    worker: Option<JoinHandle<std::io::Result<()>>>,
    output_path: PathBuf,
}

impl DiagnosticsRuntime {
    pub fn start(
        directory: impl AsRef<Path>,
        run_id: RunId,
        raw_capture_enabled: bool,
        ack_mode: &str,
    ) -> Result<Self, String> {
        fs::create_dir_all(directory.as_ref()).map_err(|error| {
            format!(
                "Failed to create diagnostics directory '{}': {error}",
                directory.as_ref().display()
            )
        })?;

        let run_id_text = format_run_id(run_id);
        let output_path = directory.as_ref().join(format!("run_{run_id_text}.csv"));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output_path)
            .map_err(|error| {
                format!(
                    "Failed to create diagnostics file '{}': {error}",
                    output_path.display()
                )
            })?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "# TickScope diagnostics v{}", env!("CARGO_PKG_VERSION"))
            .and_then(|()| writeln!(writer, "# run_id={run_id_text}"))
            .and_then(|()| writeln!(writer, "# raw_capture={raw_capture_enabled}"))
            .and_then(|()| writeln!(writer, "# ack_mode={ack_mode}"))
            .and_then(|()| {
                writeln!(
                    writer,
                    "# ack_boundary={}",
                    match (ack_mode, raw_capture_enabled) {
                        ("off", _) => "disabled",
                        (_, true) => "durable_raw_frame_and_volatile_ingress_acceptance",
                        (_, false) => "volatile_ingress_acceptance",
                    }
                )
            })
            .and_then(|()| writeln!(writer, "# summary_interval_ms=1000"))
            .and_then(|()| {
                writeln!(
                    writer,
                    "# percentile_window_samples_per_stage=2048"
                )
            })
            .and_then(|()| {
                writeln!(
                    writer,
                    "interval_end_utc_ms,interval_elapsed_ms,stage,accepted_samples,percentile_window_samples,dropped_metric_samples,p50_us,p95_us,p99_us,max_us"
                )
            })
            .and_then(|()| writer.flush())
            .map_err(|error| {
                format!(
                    "Failed to initialize diagnostics file '{}': {error}",
                    output_path.display()
                )
            })?;

        let (sample_sender, sample_receiver) = bounded(SAMPLE_CHANNEL_CAPACITY);
        let (stop_sender, stop_receiver) = bounded(1);
        let dropped_samples = Arc::new(AtomicU64::new(0));
        let worker_dropped_samples = dropped_samples.clone();
        let worker = thread::Builder::new()
            .name("tickscope-diagnostics".to_string())
            .spawn(move || {
                diagnostics_worker(
                    writer,
                    sample_receiver,
                    stop_receiver,
                    worker_dropped_samples,
                )
            })
            .map_err(|error| format!("Failed to start diagnostics worker: {error}"))?;

        Ok(Self {
            handle: DiagnosticsHandle {
                sender: sample_sender,
                dropped_samples,
            },
            stop_sender,
            worker: Some(worker),
            output_path,
        })
    }

    pub fn handle(&self) -> DiagnosticsHandle {
        self.handle.clone()
    }

    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    pub fn finish(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.stop_sender.try_send(());
            match worker.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::warn!(
                    "Diagnostics writer failed for '{}': {}",
                    self.output_path.display(),
                    error
                ),
                Err(_) => log::warn!("Diagnostics writer thread panicked"),
            }
        }
    }
}

impl Drop for DiagnosticsRuntime {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

fn diagnostics_worker(
    mut writer: BufWriter<std::fs::File>,
    samples: Receiver<DiagnosticSample>,
    stop: Receiver<()>,
    dropped_samples: Arc<AtomicU64>,
) -> std::io::Result<()> {
    let mut windows: [LatencyRingBuffer; DiagnosticStage::ALL.len()] =
        std::array::from_fn(|_| LatencyRingBuffer::new());
    let mut interval_start = Instant::now();

    loop {
        if stop.try_recv().is_ok() {
            drain_samples(&samples, &mut windows);
            write_interval(&mut writer, &mut windows, &dropped_samples, interval_start)?;
            break;
        }

        let remaining = SUMMARY_INTERVAL.saturating_sub(interval_start.elapsed());
        let timeout = remaining.min(STOP_POLL_INTERVAL);
        match samples.recv_timeout(timeout) {
            Ok(sample) => windows[sample.stage as usize].record(sample.duration_ns),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                write_interval(&mut writer, &mut windows, &dropped_samples, interval_start)?;
                break;
            }
        }

        if interval_start.elapsed() >= SUMMARY_INTERVAL {
            write_interval(&mut writer, &mut windows, &dropped_samples, interval_start)?;
            interval_start = Instant::now();
        }
    }

    writer.flush()
}

fn drain_samples(
    samples: &Receiver<DiagnosticSample>,
    windows: &mut [LatencyRingBuffer; DiagnosticStage::ALL.len()],
) {
    while let Ok(sample) = samples.try_recv() {
        windows[sample.stage as usize].record(sample.duration_ns);
    }
}

fn write_interval(
    writer: &mut BufWriter<std::fs::File>,
    windows: &mut [LatencyRingBuffer; DiagnosticStage::ALL.len()],
    dropped_samples: &AtomicU64,
    interval_start: Instant,
) -> std::io::Result<()> {
    let dropped = dropped_samples.swap(0, Ordering::Relaxed);
    let mut wrote_row = false;
    let interval_end_utc_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let interval_elapsed_ms = interval_start.elapsed().as_millis();

    for stage in DiagnosticStage::ALL {
        let ring = &mut windows[stage as usize];
        if ring.total_samples() == 0 {
            continue;
        }
        let percentiles = ring.compute_percentiles();
        writeln!(
            writer,
            "{interval_end_utc_ms},{interval_elapsed_ms},{},{},{},{},{:.3},{:.3},{:.3},{:.3}",
            stage.name(),
            percentiles.sample_count,
            ring.valid_count(),
            0,
            percentiles.p50_us,
            percentiles.p95_us,
            percentiles.p99_us,
            percentiles.max_us,
        )?;
        ring.clear();
        wrote_row = true;
    }

    if dropped > 0 {
        writeln!(
            writer,
            "{interval_end_utc_ms},{interval_elapsed_ms},metric_samples_dropped,0,0,{dropped},0,0,0,0"
        )?;
        wrote_row = true;
    }

    if wrote_row {
        writer.flush()?;
    }
    Ok(())
}

fn format_run_id(run_id: RunId) -> String {
    let mut text = String::with_capacity(32);
    for byte in run_id.0 {
        let _ = write!(&mut text, "{byte:02x}");
    }
    text
}
