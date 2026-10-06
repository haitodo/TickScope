//! Parquet tick data reader for FX broker Hive-partitioned datasets.
//! Schema: `broker={broker}/symbol={symbol}/year={YYYY}/month={MM}/data.parquet`

use arrow::array::{Float64Array, Int64Array};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::core::types::BrokerId;
use crate::replay::driver::{mt5_to_utc_ms, utc_to_mt5_ms};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayTick {
    pub broker_id: BrokerId,
    pub utc_ms: i64,
    pub mt5_ms: i64,
    pub bid: f64,
    pub ask: f64,
    pub receive_delay_ms: i64,
}

impl ReplayTick {
    #[inline]
    #[must_use]
    pub const fn effective_utc_ms(&self) -> i64 {
        self.utc_ms + self.receive_delay_ms
    }
}

#[derive(Debug, Clone)]
pub struct PartitionMeta {
    pub path: PathBuf,
    pub year: i32,
    pub month: u32,
}

pub struct BrokerParquetSource {
    pub broker_id: BrokerId,
    pub broker_name: String,
    pub symbol: String,
    pub root_dir: PathBuf,
    pub partitions: Vec<PartitionMeta>,
    pub current_partition_idx: Option<usize>,
    pub current_ticks: Arc<Vec<ReplayTick>>,
    pub cursor: usize,
    /// Physical network reception delay profile in milliseconds (Domestic: 15-30ms, Overseas: 120-220ms)
    pub receive_delay_ms: i64,
    /// In-memory partition cache by partition index
    partition_cache: HashMap<usize, Arc<Vec<ReplayTick>>>,
}

impl BrokerParquetSource {
    /// Discover the broker Hive partitions under `root_dir`, skipping entries that do not parse.
    ///
    /// # Errors
    ///
    /// Never fails today (unreadable partition directories are skipped); the `Result` keeps a future
    /// discovery failure from changing every call site.
    pub fn new(
        broker_id: BrokerId,
        broker_name: &str,
        symbol: &str,
        root_dir: &Path,
    ) -> Result<Self, String> {
        let sym_clean = symbol.to_lowercase();
        let sym_base = sym_clean.split(['.', '_', '/']).next().unwrap_or(&sym_clean).to_string();

        let mut dir_candidates = Vec::new();
        let name_lower = broker_name.to_lowercase();

        // Standard broker candidate names
        match name_lower.as_str() {
            "oanda" | "oanda_mt5" => {
                dir_candidates.push("broker=oanda_mt5".to_string());
                dir_candidates.push("broker=oanda_zip".to_string());
            }
            "tradeview" | "tradeview_mt5" => {
                dir_candidates.push("broker=tradeview_mt5".to_string());
            }
            "dukascopy" | "dukascopy_mt5" | "dukascopy_jforex" => {
                dir_candidates.push("broker=dukascopy_mt5".to_string());
                dir_candidates.push("broker=dukascopy_jforex".to_string());
            }
            "axiory" | "axiory_mt5" => {
                dir_candidates.push("broker=axiory_mt5".to_string());
            }
            "jfx" | "jfx_mt5" => {
                dir_candidates.push("broker=jfx_mt5".to_string());
            }
            "metaquotes" | "metaquotes_mt5" => {
                dir_candidates.push("broker=metaquotes_mt5".to_string());
            }
            _ => {}
        }

        // Direct candidate names
        let exact_candidate = format!("broker={name_lower}");
        if !dir_candidates.contains(&exact_candidate) {
            dir_candidates.push(exact_candidate);
        }
        let mt5_candidate = format!("broker={name_lower}_mt5");
        if !dir_candidates.contains(&mt5_candidate) {
            dir_candidates.push(mt5_candidate);
        }

        // Scan root_dir for any directory containing the broker name
        if let Ok(entries) = std::fs::read_dir(root_dir) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type() {
                    if ft.is_dir() {
                        let dir_name = entry.file_name().to_string_lossy().to_lowercase();
                        if dir_name.starts_with("broker=") && dir_name.contains(&name_lower)
                            && !dir_candidates.contains(&dir_name) {
                                dir_candidates.push(dir_name);
                            }
                    }
                }
            }
        }

        let mut partitions = Vec::new();

        for candidate in dir_candidates {
            let broker_dir = root_dir.join(&candidate);
            if !broker_dir.exists() {
                continue;
            }

            // Find matching symbol subdirectory
            let mut matched_sym_dirs = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&broker_dir) {
                for entry in entries.flatten() {
                    let dname = entry.file_name().to_string_lossy().to_string();
                    if let Some(s) = dname.strip_prefix("symbol=") {
                        let s_clean = s.to_lowercase();
                        let s_base = s_clean.split(['.', '_', '/']).next().unwrap_or(&s_clean);
                        if s_base == sym_base {
                            matched_sym_dirs.push(entry.path());
                        }
                    }
                }
            }

            for sym_dir in matched_sym_dirs {
                if let Ok(year_entries) = std::fs::read_dir(&sym_dir) {
                    for y_res in year_entries.flatten() {
                        let y_name = y_res.file_name().to_string_lossy().to_string();
                        if let Some(year_str) = y_name.strip_prefix("year=") {
                            let Ok(year) = year_str.parse::<i32>() else { continue };

                            if let Ok(month_entries) = std::fs::read_dir(y_res.path()) {
                                for m_res in month_entries.flatten() {
                                    let m_name = m_res.file_name().to_string_lossy().to_string();
                                    if let Some(month_str) = m_name.strip_prefix("month=") {
                                        let Ok(month) = month_str.parse::<u32>() else { continue };

                                        let parquet_file = m_res.path().join("data.parquet");
                                        if parquet_file.exists() {
                                            partitions.push(PartitionMeta {
                                                path: parquet_file,
                                                year,
                                                month,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !partitions.is_empty() {
                break;
            }
        }

        partitions.sort_by_key(|p| (p.year, p.month));

        log::info!(
            "[BrokerParquetSource] Broker {} ({}) discovered {} partition(s)",
            broker_id,
            broker_name,
            partitions.len()
        );

        Ok(Self {
            broker_id,
            broker_name: broker_name.to_string(),
            symbol: sym_base,
            root_dir: root_dir.to_path_buf(),
            partitions,
            current_partition_idx: None,
            current_ticks: Arc::new(Vec::new()),
            cursor: 0,
            receive_delay_ms: 0,
            partition_cache: HashMap::new(),
        })
    }

    #[must_use]
    pub const fn with_receive_delay_ms(mut self, delay_ms: i64) -> Self {
        self.receive_delay_ms = delay_ms;
        self
    }

    /// Apply measured physical network reception delay profile:
    /// Domestic brokers (JFX, OANDA): 15-30ms (default 20ms)
    /// Overseas brokers (Tradeview, Dukascopy, Axiory): 120-220ms (default 180ms)
    #[must_use]
    pub fn with_receive_delay_profile(mut self) -> Self {
        let name_lower = self.broker_name.to_lowercase();
        self.receive_delay_ms = if name_lower.contains("jfx") || name_lower.contains("oanda") {
            20
        } else {
            180
        };
        self
    }

    #[inline]
    #[must_use]
    pub const fn effective_utc_ms(&self, tick: &ReplayTick) -> i64 {
        tick.utc_ms + self.receive_delay_ms
    }

    /// Load the partition corresponding to the given year and month.
    /// # Errors
    ///
    /// Propagates the error from [`Self::load_partition_by_idx`].
    pub fn load_partition(&mut self, year: i32, month: u32) -> Result<bool, String> {
        let idx = self.partitions.iter().position(|p| p.year == year && p.month == month);
        let Some(idx) = idx else {
            return Ok(false);
        };
        self.load_partition_by_idx(idx)
    }

    /// Load partition by internal index (0-based) using memory cache.
    /// # Errors
    ///
    /// Returns a message naming the Parquet file when it cannot be opened or read.
    pub fn load_partition_by_idx(&mut self, idx: usize) -> Result<bool, String> {
        if idx >= self.partitions.len() {
            return Ok(false);
        }

        if self.current_partition_idx == Some(idx) {
            return Ok(true);
        }

        if let Some(cached) = self.partition_cache.get(&idx) {
            self.current_partition_idx = Some(idx);
            self.current_ticks = cached.clone();
            self.cursor = 0;
            return Ok(true);
        }

        let p_meta = &self.partitions[idx];
        let ticks = read_parquet_ticks(self.broker_id, &p_meta.path)?;
        let arc_ticks = Arc::new(ticks);

        self.partition_cache.insert(idx, arc_ticks.clone());
        self.current_partition_idx = Some(idx);
        self.current_ticks = arc_ticks;
        self.cursor = 0;

        log::info!(
            "[BrokerParquetSource] Broker {} ({}) loaded {:04}-{:02}: {} ticks",
            self.broker_id,
            self.broker_name,
            p_meta.year,
            p_meta.month,
            self.current_ticks.len()
        );

        Ok(true)
    }

    /// Automatically advance to the next chronological partition when the current partition is exhausted.
    pub fn try_advance_partition(&mut self) -> bool {
        let next_idx = match self.current_partition_idx {
            Some(i) => i + 1,
            None => 0,
        };
        if next_idx < self.partitions.len() {
            match self.load_partition_by_idx(next_idx) {
                Ok(true) => {
                    self.cursor = 0;
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Ensure partition is loaded into memory cache.
    fn ensure_partition_cached(&mut self, idx: usize) -> Result<bool, String> {
        if idx >= self.partitions.len() {
            return Ok(false);
        }
        if self.partition_cache.contains_key(&idx) {
            return Ok(true);
        }
        let p_meta = &self.partitions[idx];
        let ticks = read_parquet_ticks(self.broker_id, &p_meta.path)?;
        self.partition_cache.insert(idx, Arc::new(ticks));
        Ok(true)
    }

    /// Load the partition containing or nearest to the specified UTC millisecond timestamp.
    /// # Errors
    ///
    /// Propagates the error from [`Self::load_partition`] when the covering partition cannot be read.
    pub fn load_for_utc_ms(&mut self, utc_ms: i64) -> Result<bool, String> {
        let dt = chrono::DateTime::from_timestamp(utc_ms / 1000, 0)
            .map(|d| d.naive_utc());
        let (year, month) = match dt {
            Some(d) => {
                use chrono::Datelike;
                (d.year(), d.month())
            }
            None => return Ok(false),
        };

        self.load_partition(year, month)
    }

    /// Seek cursor to the specified UTC millisecond timestamp within the loaded partition.
    pub fn seek_to_utc(&mut self, target_utc_ms: i64) {
        if self.current_ticks.is_empty() {
            self.cursor = 0;
            return;
        }

        let idx = match self.current_ticks.binary_search_by_key(&target_utc_ms, |t| t.utc_ms) {
            Ok(exact) => exact,
            Err(insert_idx) => insert_idx,
        };

        self.cursor = idx.min(self.current_ticks.len());
    }

    /// Return the current tick under the cursor without consuming it.
    /// If the cursor has reached the end of the partition, automatically transitions to the next partition.
    pub fn peek(&mut self) -> Option<&ReplayTick> {
        if self.cursor >= self.current_ticks.len() {
            let _ = self.try_advance_partition();
        }
        if self.cursor < self.current_ticks.len() {
            Some(&self.current_ticks[self.cursor])
        } else {
            None
        }
    }

    /// Return the current tick and advance the cursor by 1.
    /// If the cursor has reached the end of the partition, automatically transitions to the next partition.
    pub fn advance(&mut self) -> Option<ReplayTick> {
        if self.cursor >= self.current_ticks.len() {
            let _ = self.try_advance_partition();
        }
        if self.cursor < self.current_ticks.len() {
            let mut t = self.current_ticks[self.cursor];
            t.receive_delay_ms = self.receive_delay_ms;
            self.cursor += 1;
            Some(t)
        } else {
            None
        }
    }

    /// Collect warm-up ticks in `[from_utc_ms, to_utc_ms]` from the loaded partition,
    /// seamlessly spanning the previous partition if `from_utc_ms` crosses a month boundary.
    pub fn get_warmup_range(&mut self, from_utc_ms: i64, to_utc_ms: i64) -> Vec<ReplayTick> {
        if from_utc_ms > to_utc_ms {
            return Vec::new();
        }

        let mut result = Vec::new();

        // Check if from_utc_ms falls in the previous partition
        if let Some(curr_idx) = self.current_partition_idx {
            if curr_idx > 0 {
                let prev_idx = curr_idx - 1;
                if let Ok(true) = self.ensure_partition_cached(prev_idx) {
                    if let Some(prev_ticks) = self.partition_cache.get(&prev_idx) {
                        let start = match prev_ticks.binary_search_by_key(&from_utc_ms, |t| t.utc_ms) {
                            Ok(i) | Err(i) => i,
                        };
                        let end = match prev_ticks.binary_search_by_key(&to_utc_ms, |t| t.utc_ms) {
                            Ok(i) => i.saturating_add(1),
                            Err(i) => i,
                        };
                        let start = start.min(prev_ticks.len());
                        let end = end.min(prev_ticks.len());
                        if start < end {
                            result.extend_from_slice(&prev_ticks[start..end]);
                        }
                    }
                }
            }
        }

        // Current partition ticks
        if !self.current_ticks.is_empty() {
            let start = match self.current_ticks.binary_search_by_key(&from_utc_ms, |t| t.utc_ms) {
                Ok(i) | Err(i) => i,
            };
            let end = match self.current_ticks.binary_search_by_key(&to_utc_ms, |t| t.utc_ms) {
                Ok(i) => i.saturating_add(1),
                Err(i) => i,
            };
            let start = start.min(self.current_ticks.len());
            let end = end.min(self.current_ticks.len());
            if start < end {
                result.extend_from_slice(&self.current_ticks[start..end]);
            }
        }

        for t in &mut result {
            t.receive_delay_ms = self.receive_delay_ms;
        }

        result
    }
}

/// Read ticks from a single parquet file with projection, robust schema decoding, and strict sorting.
fn read_parquet_ticks(broker_id: BrokerId, path: &Path) -> Result<Vec<ReplayTick>, String> {
    let file = File::open(path)
        .map_err(|e| format!("Failed to open parquet file '{}': {}", path.display(), e))?;

    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| format!("ParquetRecordBatchReaderBuilder failed '{}': {}", path.display(), e))?;

    let file_schema = builder.schema();
    let wanted = ["utc_ms", "mt5_ms", "bid", "ask"];
    let mut root_indices = Vec::new();
    for (idx, field) in file_schema.fields().iter().enumerate() {
        if wanted.iter().any(|&w| w.eq_ignore_ascii_case(field.name())) {
            root_indices.push(idx);
        }
    }

    let builder = if root_indices.is_empty() {
        builder
    } else {
        let mask = ProjectionMask::roots(builder.parquet_schema(), root_indices);
        builder.with_projection(mask)
    };

    let reader = builder
        .build()
        .map_err(|e| format!("Failed to build parquet reader: {e}"))?;

    let mut ticks = Vec::new();

    for batch_res in reader {
        let batch = batch_res.map_err(|e| format!("Error reading record batch: {e}"))?;
        let schema = batch.schema();

        let utc_idx = schema.index_of("utc_ms").ok();
        let mt5_idx = schema.index_of("mt5_ms").ok();
        let bid_idx = schema.index_of("bid").ok();
        let ask_idx = schema.index_of("ask").ok();

        let (Some(b_idx), Some(a_idx)) = (bid_idx, ask_idx) else {
            continue;
        };

        let bid_col = batch.column(b_idx).as_any().downcast_ref::<Float64Array>();
        let ask_col = batch.column(a_idx).as_any().downcast_ref::<Float64Array>();
        let utc_col = utc_idx.and_then(|i| batch.column(i).as_any().downcast_ref::<Int64Array>());
        let mt5_col = mt5_idx.and_then(|i| batch.column(i).as_any().downcast_ref::<Int64Array>());

        let (Some(b_arr), Some(a_arr)) = (bid_col, ask_col) else {
            continue;
        };

        let num_rows = batch.num_rows();
        ticks.reserve(num_rows);

        for row in 0..num_rows {
            let bid = b_arr.value(row);
            let ask = a_arr.value(row);
            let mut utc_ms = utc_col.map_or(0, |c| c.value(row));
            let mut mt5_ms = mt5_col.map_or(0, |c| c.value(row));

            if utc_ms == 0 && mt5_ms > 0 {
                utc_ms = mt5_to_utc_ms(mt5_ms);
            } else if mt5_ms == 0 && utc_ms > 0 {
                mt5_ms = utc_to_mt5_ms(utc_ms);
            }

            if bid > 0.0 && ask >= bid && bid.is_finite() && ask.is_finite() && utc_ms > 0 {
                ticks.push(ReplayTick {
                    broker_id,
                    utc_ms,
                    mt5_ms,
                    bid,
                    ask,
                    receive_delay_ms: 0,
                });
            }
        }
    }

    // Ensure sorted strictly by utc_ms
    ticks.sort_by_key(|t| t.utc_ms);

    Ok(ticks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_tick_structure() {
        let t = ReplayTick {
            broker_id: 1,
            utc_ms: 1_700_000_000_000,
            mt5_ms: 1_700_007_200_000,
            bid: 150.123,
            ask: 150.125,
            receive_delay_ms: 0,
        };
        assert_eq!(t.broker_id, 1);
        assert_eq!(t.bid, 150.123);
        assert_eq!(t.ask, 150.125);
    }
}
