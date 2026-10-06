//! Multi-broker k-way chronological tick merge stream.
//! Interleaves ticks across up to 5 brokers with nanosecond-level chronological ordering.

use super::parquet_source::{BrokerParquetSource, ReplayTick};

pub struct MergeStream {
    pub sources: Vec<BrokerParquetSource>,
}

impl MergeStream {
    #[must_use]
    pub const fn new(sources: Vec<BrokerParquetSource>) -> Self {
        Self { sources }
    }

    /// Load partition for all brokers for the specified UTC millisecond timestamp, accounting for physical receive delays.
    /// Make sure every source has the partition covering `utc_ms` loaded.
    ///
    /// # Errors
    ///
    /// Returns the first per-source error, which names the Parquet file that could not be read.
    pub fn load_for_utc_ms(&mut self, utc_ms: i64) -> Result<(), String> {
        for s in &mut self.sources {
            let _ = s.load_for_utc_ms(utc_ms - s.receive_delay_ms)?;
        }
        Ok(())
    }

    /// Seek all broker cursors to the given UTC millisecond timestamp, accounting for physical receive delays.
    pub fn seek_to_utc(&mut self, target_utc_ms: i64) {
        for s in &mut self.sources {
            s.seek_to_utc(target_utc_ms - s.receive_delay_ms);
        }
    }

    /// Peek the earliest UTC millisecond timestamp across all broker streams.
    pub fn peek_utc_ms(&mut self) -> Option<i64> {
        let mut min_utc = None;
        for s in &mut self.sources {
            if let Some(t) = s.peek().copied() {
                let eff_utc = s.effective_utc_ms(&t);
                match min_utc {
                    None => min_utc = Some(eff_utc),
                    Some(m) if eff_utc < m => min_utc = Some(eff_utc),
                    _ => {}
                }
            }
        }
        min_utc
    }

    /// Pop the next chronological tick across all 5 broker sources.
    /// Breaks ties deterministically by `broker_id`.
    pub fn pop_next(&mut self) -> Option<ReplayTick> {
        let mut best_idx = None;
        let mut best_key = (i64::MAX, u32::MAX);

        for (idx, s) in self.sources.iter_mut().enumerate() {
            if let Some(t) = s.peek().copied() {
                let key = (s.effective_utc_ms(&t), t.broker_id);
                if key < best_key {
                    best_key = key;
                    best_idx = Some(idx);
                }
            }
        }

        if let Some(idx) = best_idx {
            self.sources[idx].advance()
        } else {
            None
        }
    }

    /// Pop all ticks up to `max_utc_ms` (inclusive), limited to `max_count`.
    /// Performs a single pass per tick, automatically transitioning partition boundaries.
    pub fn pop_up_to(&mut self, max_utc_ms: i64, max_count: usize) -> Vec<ReplayTick> {
        let mut result = Vec::with_capacity(max_count.min(256));
        while result.len() < max_count {
            let mut best_idx = None;
            let mut best_key = (i64::MAX, u32::MAX);

            for (idx, s) in self.sources.iter_mut().enumerate() {
                if let Some(t) = s.peek().copied() {
                    let key = (s.effective_utc_ms(&t), t.broker_id);
                    if key < best_key {
                        best_key = key;
                        best_idx = Some(idx);
                    }
                }
            }

            match best_idx {
                Some(idx) if best_key.0 <= max_utc_ms => {
                    if let Some(tick) = self.sources[idx].advance() {
                        result.push(tick);
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        result
    }

    /// Collect and merge warm-up ticks in `[from_utc_ms, to_utc_ms]` from all brokers.
    /// Result is strictly sorted chronologically by `(effective_utc_ms, broker_id)`.
    pub fn get_warmup_ticks(&mut self, from_utc_ms: i64, to_utc_ms: i64) -> Vec<ReplayTick> {
        let mut merged = Vec::new();
        for s in &mut self.sources {
            let eff_from = from_utc_ms - s.receive_delay_ms;
            let eff_to = to_utc_ms - s.receive_delay_ms;
            let ticks = s.get_warmup_range(eff_from, eff_to);
            for t in ticks {
                merged.push((s.effective_utc_ms(&t), t));
            }
        }
        merged.sort_by_key(|(eff, t)| (*eff, t.broker_id));
        merged.into_iter().map(|(_, t)| t).collect()
    }

    /// Return total broker sources in this merge stream.
    #[must_use]
    pub const fn broker_count(&self) -> usize {
        self.sources.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_stream_empty() {
        let mut stream = MergeStream::new(vec![]);
        assert_eq!(stream.peek_utc_ms(), None);
        assert_eq!(stream.pop_next(), None);
    }
}
