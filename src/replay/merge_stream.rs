//! Multi-broker k-way chronological tick merge stream.
//! Interleaves ticks across up to 5 brokers with nanosecond-level chronological ordering.

use super::parquet_source::{BrokerParquetSource, ReplayTick};

pub struct MergeStream {
    pub sources: Vec<BrokerParquetSource>,
}

impl MergeStream {
    pub fn new(sources: Vec<BrokerParquetSource>) -> Self {
        Self { sources }
    }

    /// Load partition for all brokers for the specified UTC millisecond timestamp.
    pub fn load_for_utc_ms(&mut self, utc_ms: i64) -> Result<(), String> {
        for s in &mut self.sources {
            let _ = s.load_for_utc_ms(utc_ms)?;
        }
        Ok(())
    }

    /// Seek all broker cursors to the given UTC millisecond timestamp.
    pub fn seek_to_utc(&mut self, target_utc_ms: i64) {
        for s in &mut self.sources {
            s.seek_to_utc(target_utc_ms);
        }
    }

    /// Peek the earliest UTC millisecond timestamp across all broker streams.
    pub fn peek_utc_ms(&mut self) -> Option<i64> {
        let mut min_utc = None;
        for s in &mut self.sources {
            if let Some(t) = s.peek() {
                match min_utc {
                    None => min_utc = Some(t.utc_ms),
                    Some(m) if t.utc_ms < m => min_utc = Some(t.utc_ms),
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
            if let Some(t) = s.peek() {
                let key = (t.utc_ms, t.broker_id);
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
                if let Some(t) = s.peek() {
                    let key = (t.utc_ms, t.broker_id);
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
    /// Result is strictly sorted chronologically by `(utc_ms, broker_id)`.
    pub fn get_warmup_ticks(&mut self, from_utc_ms: i64, to_utc_ms: i64) -> Vec<ReplayTick> {
        let mut merged = Vec::new();
        for s in &mut self.sources {
            let ticks = s.get_warmup_range(from_utc_ms, to_utc_ms);
            merged.extend(ticks);
        }
        merged.sort_by_key(|t| (t.utc_ms, t.broker_id));
        merged
    }

    /// Return total broker sources in this merge stream.
    pub fn broker_count(&self) -> usize {
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
