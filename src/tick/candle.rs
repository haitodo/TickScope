//! Candle aggregation and CandleBook for fixed time slots.
//! Reference: docs/blueprint/semantics-candle.md

use crate::contracts::models::*;
use crate::contracts::types::*;
use crate::contracts::config::SlotRetention;
use std::collections::{BTreeMap, HashMap};

pub fn calculate_slot_start(utc_ms: UtcMs, period_ms: i64) -> UtcMs {
    let start = utc_ms.0.div_euclid(period_ms) * period_ms;
    UtcMs(start)
}

#[derive(Debug, Clone)]
pub struct CandleBook {
    // period_ms -> broker_id -> slot_start -> CandleSlot
    books: HashMap<i64, HashMap<BrokerId, BTreeMap<UtcMs, CandleSlot>>>,
    supported_periods: Vec<i64>,
    retention_slots: HashMap<i64, usize>,
    latest_slot_by_broker: HashMap<(i64, BrokerId), UtcMs>,
}

impl CandleBook {
    pub fn new(supported_periods: Vec<i64>) -> Self {
        let retentions = supported_periods.iter().map(|&period_ms| SlotRetention {
            period_ms,
            // Preserve a useful bounded default even for direct test/tool use.
            slots: 1_024,
        }).collect::<Vec<_>>();
        Self::with_retentions(&retentions)
    }

    pub fn with_retentions(retentions: &[SlotRetention]) -> Self {
        let mut books = HashMap::new();
        let mut supported_periods = Vec::new();
        let mut retention_slots = HashMap::new();
        for retention in retentions {
            if retention.period_ms > 0 && retention.slots > 0 {
                books.entry(retention.period_ms).or_insert_with(HashMap::new);
                if !supported_periods.contains(&retention.period_ms) {
                    supported_periods.push(retention.period_ms);
                }
                retention_slots.insert(retention.period_ms, retention.slots);
            }
        }
        Self {
            books,
            supported_periods,
            retention_slots,
            latest_slot_by_broker: HashMap::new(),
        }
    }

    pub fn on_tick(
        &mut self,
        tick: &NormalizedTick,
        price_mode: PriceMode,
        current_utc_now: UtcMs,
    ) {
        let bid = tick.observed.record.bid;
        let ask = tick.observed.record.ask;

        // Validity check
        if bid <= 0.0 || ask <= 0.0 || ask < bid || bid.is_nan() || ask.is_nan() {
            return;
        }

        let price = match price_mode {
            PriceMode::Bid => bid,
            PriceMode::Ask => ask,
            PriceMode::Mid => (bid + ask) / 2.0,
        };

        let broker_id = tick.observed.tick_id.broker_id;
        let seq = tick.observed.tick_id.sequence;
        let key = (tick.utc_ms, seq);

        for &period in &self.supported_periods {
            let slot_start = calculate_slot_start(tick.utc_ms, period);
            let broker_map = self.books.get_mut(&period).unwrap().entry(broker_id).or_default();

            let slot = broker_map.entry(slot_start).or_insert_with(|| CandleSlot {
                broker_id,
                segment_id: tick.observed.segment_id,
                period_ms: period,
                start_utc_ms: slot_start,
                state: SlotState::Empty,
                ohlc: None,
                tick_count: 0,
                revision: 0,
                coverage: if tick.observed.is_warmup {
                    SlotCoverage::Partial
                } else {
                    SlotCoverage::Full
                },
            });

            match slot.ohlc.as_mut() {
                Some(ohlc) => {
                    ohlc.high = ohlc.high.max(price);
                    ohlc.low = ohlc.low.min(price);
                    if key < ohlc.open_key {
                        ohlc.open = price;
                        ohlc.open_key = key;
                    }
                    if key >= ohlc.close_key {
                        ohlc.close = price;
                        ohlc.close_key = key;
                    }
                    slot.tick_count += 1;
                    slot.revision += 1;
                }
                None => {
                    slot.ohlc = Some(Ohlc {
                        open: price,
                        high: price,
                        low: price,
                        close: price,
                        open_key: key,
                        close_key: key,
                    });
                    slot.tick_count = 1;
                    slot.revision += 1;
                }
            }

            if slot_start.0 + period <= current_utc_now.0 {
                slot.state = SlotState::Closed;
            } else {
                slot.state = SlotState::Active;
            }

            let latest_slot = self.latest_slot_by_broker
                .entry((period, broker_id))
                .or_insert(slot_start);
            if slot_start > *latest_slot {
                *latest_slot = slot_start;
            }
            let slots = self.retention_slots.get(&period).copied().unwrap_or(1).max(1);
            let cutoff = UtcMs(latest_slot.0.saturating_sub(
                period.saturating_mul((slots.saturating_sub(1)) as i64),
            ));
            broker_map.retain(|start, _| *start >= cutoff);
        }
    }

    pub fn advance_utc(&mut self, current_utc_now: UtcMs) {
        for (&period, broker_map) in self.books.iter_mut() {
            for slot_map in broker_map.values_mut() {
                for slot in slot_map.values_mut() {
                    if slot.state == SlotState::Active && slot.start_utc_ms.0 + period <= current_utc_now.0 {
                        slot.state = SlotState::Closed;
                    }
                }
            }
        }
    }

    pub fn get_candle_view(
        &self,
        period_ms: i64,
        broker_ids: &[BrokerId],
        num_slots: usize,
        current_utc_now: UtcMs,
    ) -> CandleView {
        let current_slot_start = calculate_slot_start(current_utc_now, period_ms);
        let mut slot_starts = Vec::with_capacity(num_slots);

        for i in (0..num_slots).rev() {
            let start = current_slot_start.0 - (i as i64 * period_ms);
            slot_starts.push(UtcMs(start));
        }

        let mut slots_by_broker = HashMap::new();
        let broker_map = self.books.get(&period_ms);

        for &broker_id in broker_ids {
            let mut broker_slots = Vec::with_capacity(num_slots);
            let b_slots = broker_map.and_then(|bm| bm.get(&broker_id));

            for &start in &slot_starts {
                if let Some(slot) = b_slots.and_then(|m| m.get(&start)) {
                    let mut s = slot.clone();
                    if s.state == SlotState::Active && s.start_utc_ms.0 + period_ms <= current_utc_now.0 {
                        s.state = SlotState::Closed;
                    }
                    broker_slots.push(s);
                } else {
                    let state = if start.0 + period_ms <= current_utc_now.0 {
                        SlotState::Empty
                    } else {
                        SlotState::Active
                    };
                    broker_slots.push(CandleSlot {
                        broker_id,
                        segment_id: 0,
                        period_ms,
                        start_utc_ms: start,
                        state,
                        ohlc: None,
                        tick_count: 0,
                        revision: 0,
                        coverage: SlotCoverage::Full,
                    });
                }
            }
            slots_by_broker.insert(broker_id, broker_slots);
        }

        CandleView {
            period_ms,
            slot_starts,
            slots_by_broker,
        }
    }
}
