//! Sequence tracking, duplicate detection, and gap detection for broker tick streams.

use crate::core::types::{Sequence, SequenceDisposition};
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone)]
pub struct SequenceLedger {
    expected_sequence: Sequence,
    seen_sequences: HashSet<Sequence>,
    recent_sequences: VecDeque<Sequence>,
    capacity: usize,
}

impl SequenceLedger {
    pub fn new(capacity: usize) -> Self {
        Self {
            expected_sequence: 0,
            seen_sequences: HashSet::with_capacity(capacity.min(4096)),
            recent_sequences: VecDeque::with_capacity(capacity.min(4096)),
            capacity,
        }
    }

    #[inline]
    pub fn expected_sequence(&self) -> Sequence {
        self.expected_sequence
    }

    #[inline]
    pub fn reset(&mut self) {
        self.expected_sequence = 0;
        self.seen_sequences.clear();
        self.recent_sequences.clear();
    }

    /// Observe a sequence number and return its disposition and an optional gap range `(first, last)`.
    #[inline]
    pub fn observe(&mut self, sequence: Sequence) -> (SequenceDisposition, Option<(Sequence, Sequence)>) {
        if self.seen_sequences.contains(&sequence) {
            (SequenceDisposition::DuplicateExact, None)
        } else if sequence < self.expected_sequence {
            (SequenceDisposition::OutOfOrderUnverified, None)
        } else if sequence > self.expected_sequence {
            let gap = (self.expected_sequence, sequence.saturating_sub(1));
            self.record_new(sequence);
            (SequenceDisposition::New, Some(gap))
        } else {
            self.record_new(sequence);
            (SequenceDisposition::New, None)
        }
    }

    #[inline]
    fn record_new(&mut self, sequence: Sequence) {
        self.seen_sequences.insert(sequence);
        self.recent_sequences.push_back(sequence);
        self.expected_sequence = sequence.saturating_add(1);

        while self.recent_sequences.len() > self.capacity {
            if let Some(expired) = self.recent_sequences.pop_front() {
                self.seen_sequences.remove(&expired);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequential_observations() {
        let mut ledger = SequenceLedger::new(100);
        assert_eq!(ledger.expected_sequence(), 0);

        let (disp, gap) = ledger.observe(0);
        assert_eq!(disp, SequenceDisposition::New);
        assert_eq!(gap, None);
        assert_eq!(ledger.expected_sequence(), 1);

        let (disp, gap) = ledger.observe(1);
        assert_eq!(disp, SequenceDisposition::New);
        assert_eq!(gap, None);
        assert_eq!(ledger.expected_sequence(), 2);
    }

    #[test]
    fn test_duplicate_detection() {
        let mut ledger = SequenceLedger::new(100);
        ledger.observe(0);
        ledger.observe(1);

        let (disp, gap) = ledger.observe(1);
        assert_eq!(disp, SequenceDisposition::DuplicateExact);
        assert_eq!(gap, None);
        assert_eq!(ledger.expected_sequence(), 2);
    }

    #[test]
    fn test_gap_detection() {
        let mut ledger = SequenceLedger::new(100);
        ledger.observe(0);

        // Jump from 0 to 5 -> gap is 1..=4
        let (disp, gap) = ledger.observe(5);
        assert_eq!(disp, SequenceDisposition::New);
        assert_eq!(gap, Some((1, 4)));
        assert_eq!(ledger.expected_sequence(), 6);

        // Sequence 3 arrives later -> out of order unverified
        let (disp, gap) = ledger.observe(3);
        assert_eq!(disp, SequenceDisposition::OutOfOrderUnverified);
        assert_eq!(gap, None);
    }

    #[test]
    fn test_capacity_and_expiry() {
        let mut ledger = SequenceLedger::new(3);
        ledger.observe(1);
        ledger.observe(2);
        ledger.observe(3);
        ledger.observe(4); // 1 expires

        assert_eq!(ledger.seen_sequences.len(), 3);
        assert!(!ledger.seen_sequences.contains(&1));
        assert!(ledger.seen_sequences.contains(&2));
        assert!(ledger.seen_sequences.contains(&3));
        assert!(ledger.seen_sequences.contains(&4));
    }

    #[test]
    fn test_reset() {
        let mut ledger = SequenceLedger::new(100);
        ledger.observe(5);
        ledger.reset();
        assert_eq!(ledger.expected_sequence(), 0);
        assert!(ledger.seen_sequences.is_empty());
        assert!(ledger.recent_sequences.is_empty());
    }
}
