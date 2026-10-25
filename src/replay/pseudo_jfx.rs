//! JFX-specific Synthetic Tick Generation Engine (PseudoJfxEngine)
//! Recreates JFX market microstructure characteristics from raw OANDA tick streams:
//! - Zero group delay (0 ms)
//! - Directional impulse quantization (M_OPT) preventing spike dampening and upward bias
//! - Strict change-driven emission (filtering flat quotes)
//! - Empirical JFX spread model (0.2 pip base, pre-event widening, peak decay, rollover expansion)

use arrow::array::{Int64Array, StringArray};
use chrono::{Datelike, Timelike};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

/// Indicator profile definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IndicatorProfile {
    pub currency: String,
    pub event_name: String,
    pub importance: String,
    pub tier: u8,
    pub dmm_advance_seconds: i64,
    pub dmm_base_pre_spread: f64,
    pub dmm_base_peak_spread: f64,
    pub dmm_recovery_seconds: i64,
}

/// Economic calendar event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicEvent {
    pub event_id: i64,
    pub utc_ms: i64,
    pub mt5_ms: i64,
    pub currency: String,
    pub event_name: String,
    pub importance: String,
    pub tier: u8,
    pub advance_ms: i64,
    pub base_pre_spread: f64,
    pub base_peak_spread: f64,
    pub recovery_ms: i64,
}

/// JFX quote characteristics derived from 21.36 million real JFX ticks
pub mod jfx_characteristics {
    /// Normal base spread: 0.2 pips (2 points)
    pub const BASE_SPREAD_PIPS: f64 = 0.2;

    /// Upper spread limit: 15.0 pips (150 points)
    pub const MAX_SPREAD_PIPS: f64 = 15.0;
}

/// US Eastern Daylight Saving Time check (DST starts 2nd Sunday in March, ends 1st Sunday in November)
pub fn is_us_dst(year: i32, month: u32, day: u32, hour: u32) -> bool {
    if !(3..=11).contains(&month) {
        return false;
    }
    if month > 3 && month < 11 {
        return true;
    }
    let chrono_date = match chrono::NaiveDate::from_ymd_opt(year, month, 1) {
        Some(d) => d,
        None => return false,
    };
    if month == 3 {
        let first_weekday = chrono_date.weekday().num_days_from_sunday();
        let second_sunday = if first_weekday == 0 {
            8
        } else {
            1 + (7 - first_weekday) + 7
        };
        if day > second_sunday {
            true
        } else if day == second_sunday {
            hour >= 2
        } else {
            false
        }
    } else {
        // month == 11
        let first_weekday = chrono_date.weekday().num_days_from_sunday();
        let first_sunday = if first_weekday == 0 {
            1
        } else {
            1 + (7 - first_weekday)
        };
        if day < first_sunday {
            true
        } else if day == first_sunday {
            hour < 2
        } else {
            false
        }
    }
}

/// JFX synthetic rate generation engine
pub struct PseudoJfxEngine {
    pub symbol: String,
    pub year: i32,
    pub month: u32,
    pub events: Vec<EconomicEvent>,
    /// Point size (JPY pairs: 0.001, Non-JPY pairs: 0.00001)
    point_size: f64,
    /// Price to integer point scale (JPY pairs: 1000.0, Non-JPY pairs: 100_000.0)
    point_scale: f64,
    last_raw_b_pt: i64,
    last_raw_a_pt: i64,
    last_raw_sum_pt: i64,
    last_q_pt: i64,
    last_emitted_bid_pt: i64,
    last_emitted_ask_pt: i64,
    recent_ticks: Vec<(i64, i64)>,
}

impl PseudoJfxEngine {
    #[must_use]
    pub fn new(symbol: &str, year_month: &str) -> Self {
        Self::new_with_custom_dir(symbol, year_month, None)
    }

    #[must_use]
    pub fn new_with_custom_dir(symbol: &str, year_month: &str, custom_dir: Option<&str>) -> Self {
        let parts: Vec<&str> = year_month.split('-').collect();
        let year: i32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(2026);
        let month: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(8);

        let sym_upper = symbol.to_uppercase();
        let is_jpy = sym_upper.contains("JPY");
        let (point_size, point_scale) = if is_jpy {
            (0.001, 1_000.0)
        } else {
            (0.00001, 100_000.0)
        };

        let profiles = Self::load_profile_matrix();
        let mut events =
            Self::load_events_for_month(&sym_upper, year, month, &profiles, custom_dir);
        events.retain(|e| e.tier <= 3);

        Self {
            symbol: sym_upper,
            year,
            month,
            events,
            point_size,
            point_scale,
            last_raw_b_pt: 0,
            last_raw_a_pt: 0,
            last_raw_sum_pt: 0,
            last_q_pt: 0,
            last_emitted_bid_pt: 0,
            last_emitted_ask_pt: 0,
            recent_ticks: Vec::with_capacity(256),
        }
    }

    pub fn load_profile_matrix() -> HashMap<String, IndicatorProfile> {
        let possible_paths = [
            PathBuf::from(r"D:\dev\TickReplay\analysis\scripts\indicator_profile_matrix.json"),
            PathBuf::from(r"D:\dev\TickReplay\data\analysis\indicator_profile_matrix.json"),
            PathBuf::from(r"D:\dev\Drenhis\analysis\scripts\indicator_profile_matrix.json"),
            PathBuf::from(r"D:\dev\Drenhis\analysis\indicator_profile_matrix.json"),
            PathBuf::from("data/analysis/indicator_profile_matrix.json"),
        ];

        for p in &possible_paths {
            if p.exists() {
                if let Ok(content) = std::fs::read_to_string(p) {
                    if let Ok(map) =
                        serde_json::from_str::<HashMap<String, IndicatorProfile>>(&content)
                    {
                        return map;
                    }
                }
            }
        }

        let mut map = HashMap::new();
        map.insert(
            "USD:非農業部門雇用者数".to_string(),
            IndicatorProfile {
                currency: "USD".to_string(),
                event_name: "非農業部門雇用者数".to_string(),
                importance: "high".to_string(),
                tier: 1,
                dmm_advance_seconds: 35,
                dmm_base_pre_spread: 0.039,
                dmm_base_peak_spread: 0.156,
                dmm_recovery_seconds: 120,
            },
        );
        map.insert(
            "USD:CPI".to_string(),
            IndicatorProfile {
                currency: "USD".to_string(),
                event_name: "CPI".to_string(),
                importance: "high".to_string(),
                tier: 1,
                dmm_advance_seconds: 30,
                dmm_base_pre_spread: 0.035,
                dmm_base_peak_spread: 0.140,
                dmm_recovery_seconds: 90,
            },
        );
        map
    }

    pub fn load_events_for_month(
        symbol: &str,
        year: i32,
        month: u32,
        profiles: &HashMap<String, IndicatorProfile>,
        custom_dir: Option<&str>,
    ) -> Vec<EconomicEvent> {
        let sym_lower = symbol.to_lowercase();
        let month_str = format!("{:02}", month);

        let candidates = [
            custom_dir.map(PathBuf::from),
            Some(PathBuf::from(r"D:\Drehis\economic")),
            Some(PathBuf::from(r"D:\Drenhis\economic")),
        ];

        let sym_folder = format!("symbol={sym_lower}");

        for base_opt in candidates.into_iter().flatten() {
            let possible_parquet_paths = [
                base_opt
                    .join("economic")
                    .join(&sym_folder)
                    .join(format!("year={year}"))
                    .join(format!("month={month_str}"))
                    .join("events.parquet"),
                base_opt
                    .join(&sym_folder)
                    .join(format!("year={year}"))
                    .join(format!("month={month_str}"))
                    .join("events.parquet"),
                base_opt
                    .join("economic")
                    .join(&sym_folder)
                    .join(format!("year={year}"))
                    .join(format!("month={month_str}"))
                    .join("data.parquet"),
                base_opt
                    .join(&sym_folder)
                    .join(format!("year={year}"))
                    .join(format!("month={month_str}"))
                    .join("data.parquet"),
            ];

            for path in &possible_parquet_paths {
                if !path.exists() {
                    continue;
                }
                let Ok(file) = File::open(path) else { continue };
                let Ok(builder) = ParquetRecordBatchReaderBuilder::try_new(file) else {
                    continue;
                };
                let file_schema = builder.schema();
                let wanted_cols = [
                    "event_id",
                    "utc_ms",
                    "mt5_ms",
                    "currency",
                    "event_name",
                    "importance",
                ];
                let mut root_indices = Vec::new();
                for (idx, field) in file_schema.fields().iter().enumerate() {
                    if wanted_cols
                        .iter()
                        .any(|&c| c.eq_ignore_ascii_case(field.name()))
                    {
                        root_indices.push(idx);
                    }
                }

                let builder = if !root_indices.is_empty() {
                    let mask = ProjectionMask::roots(builder.parquet_schema(), root_indices);
                    builder.with_projection(mask)
                } else {
                    builder
                };

                let Ok(mut reader) = builder.build() else {
                    continue;
                };
                let mut events = Vec::new();
                while let Some(Ok(batch)) = reader.next() {
                    let schema = batch.schema();
                    let event_id_idx = schema.index_of("event_id").ok();
                    let utc_ms_idx = schema.index_of("utc_ms").ok();
                    let mt5_ms_idx = schema.index_of("mt5_ms").ok();
                    let curr_idx = schema.index_of("currency").ok();
                    let name_idx = schema.index_of("event_name").ok();
                    let imp_idx = schema.index_of("importance").ok();

                    let (Some(e_idx), Some(u_idx), Some(m_idx)) =
                        (event_id_idx, utc_ms_idx, mt5_ms_idx)
                    else {
                        continue;
                    };
                    let (Some(c_idx), Some(n_idx), Some(i_idx)) = (curr_idx, name_idx, imp_idx)
                    else {
                        continue;
                    };

                    let event_id_col = batch.column(e_idx).as_any().downcast_ref::<Int64Array>();
                    let utc_ms_col = batch.column(u_idx).as_any().downcast_ref::<Int64Array>();
                    let mt5_ms_col = batch.column(m_idx).as_any().downcast_ref::<Int64Array>();
                    let curr_col = batch.column(c_idx).as_any().downcast_ref::<StringArray>();
                    let name_col = batch.column(n_idx).as_any().downcast_ref::<StringArray>();
                    let imp_col = batch.column(i_idx).as_any().downcast_ref::<StringArray>();

                    let (
                        Some(e_arr),
                        Some(u_arr),
                        Some(m_arr),
                        Some(c_arr),
                        Some(n_arr),
                        Some(i_arr),
                    ) = (
                        event_id_col,
                        utc_ms_col,
                        mt5_ms_col,
                        curr_col,
                        name_col,
                        imp_col,
                    )
                    else {
                        continue;
                    };

                    for row in 0..batch.num_rows() {
                        let currency = c_arr.value(row).to_string();
                        let event_name = n_arr.value(row).to_string();
                        let importance = i_arr.value(row).to_lowercase();
                        let key = format!("{currency}:{event_name}");

                        let (tier, adv_ms, base_pre, base_peak, rec_ms) =
                            if let Some(prof) = profiles.get(&key) {
                                (
                                    prof.tier,
                                    prof.dmm_advance_seconds * 1000,
                                    prof.dmm_base_pre_spread,
                                    prof.dmm_base_peak_spread,
                                    prof.dmm_recovery_seconds * 1000,
                                )
                            } else {
                                match importance.as_str() {
                                    "high" => (1, 30_000, 0.035, 0.140, 90_000),
                                    "medium" => (2, 20_000, 0.015, 0.060, 45_000),
                                    _ => (4, 10_000, 0.005, 0.020, 20_000),
                                }
                            };

                        events.push(EconomicEvent {
                            event_id: e_arr.value(row),
                            utc_ms: u_arr.value(row),
                            mt5_ms: m_arr.value(row),
                            currency,
                            event_name,
                            importance,
                            tier,
                            advance_ms: adv_ms,
                            base_pre_spread: base_pre,
                            base_peak_spread: base_peak,
                            recovery_ms: rec_ms,
                        });
                    }
                }

                if !events.is_empty() {
                    events.sort_by_key(|e| e.utc_ms);
                    return events;
                }
            }
        }

        Vec::new()
    }

    /// Process a single tick and produce a JFX characteristic quote.
    /// Change-Driven: Returns `None` if the quantized Bid and Ask match the previous emitted values.
    #[inline]
    pub fn process_tick(
        &mut self,
        mt5_time_msc: i64,
        raw_bid: f64,
        raw_ask: f64,
    ) -> Option<(f64, f64, f64)> {
        // 1. Integer point space (1pt = 0.1pip)
        let b_pt = (raw_bid * self.point_scale).round() as i64;
        let a_pt = (raw_ask * self.point_scale).round() as i64;
        let sum_pt = b_pt + a_pt; // 2 * mid_pt

        let q_pt = if self.last_raw_sum_pt == 0 {
            sum_pt / 2
        } else if sum_pt == self.last_raw_sum_pt {
            self.last_q_pt
        } else {
            let da_pt = a_pt - self.last_raw_a_pt;
            let db_pt = b_pt - self.last_raw_b_pt;
            let d_sum = sum_pt - self.last_raw_sum_pt;

            // 2. Directional Impulse Quantization (M_OPT)
            let up_impulse = (da_pt >= 2 && db_pt >= 0) || (d_sum >= 3);
            if up_impulse {
                (sum_pt + 1) / 2
            } else {
                sum_pt / 2
            }
        };

        self.last_raw_b_pt = b_pt;
        self.last_raw_a_pt = a_pt;
        self.last_raw_sum_pt = sum_pt;
        self.last_q_pt = q_pt;

        // 3. JST breakdown for rollover & session schedule
        let naive_mt5_sec = mt5_time_msc / 1000;
        let naive_dt = chrono::DateTime::from_timestamp(naive_mt5_sec, 0)
            .map(|dt| dt.naive_utc())
            .unwrap_or_default();
        let y = naive_dt.year();
        let m = naive_dt.month();
        let d = naive_dt.day();
        let h = naive_dt.hour();
        let is_dst = is_us_dst(y, m, d, h);

        let offset_ms = if is_dst {
            3 * 3600 * 1000
        } else {
            2 * 3600 * 1000
        };
        let utc_ms = mt5_time_msc - offset_ms;
        let jst_ms = utc_ms + 9 * 3600 * 1000;

        let jst_sec = jst_ms / 1000;
        let jst_dt = chrono::DateTime::from_timestamp(jst_sec, 0)
            .map(|dt| dt.naive_utc())
            .unwrap_or_default();
        let jst_hour = jst_dt.hour();
        let jst_min = jst_dt.minute();

        // 4. Spread modeling
        let mut target_spread_pips = jfx_characteristics::BASE_SPREAD_PIPS;

        // Early morning rollover
        let rollover_hour = if is_dst { 6 } else { 7 };
        let pre_hour = rollover_hour - 1;

        if jst_hour == pre_hour && jst_min == 59 {
            target_spread_pips = target_spread_pips.max(3.2);
        } else if jst_hour == rollover_hour {
            let early_pips = if jst_min <= 20 {
                9.8
            } else if jst_min <= 30 {
                8.5
            } else {
                5.9
            };
            target_spread_pips = target_spread_pips.max(early_pips);
        } else if jst_hour == rollover_hour + 1 {
            if jst_min < 5 {
                target_spread_pips = target_spread_pips.max(3.6);
            } else if jst_min < 11 {
                let progress = (jst_min - 5) as f64 / 6.0;
                let early_pips = 1.5 - (1.5 - 1.1) * progress;
                target_spread_pips = target_spread_pips.max(early_pips);
            } else if jst_min < 15 {
                let progress = (jst_min - 11) as f64 / 4.0;
                let early_pips = 1.1 - (1.1 - 0.2) * progress;
                target_spread_pips = target_spread_pips.max(early_pips);
            }
        }

        let raw_oanda_spread_pips = (a_pt - b_pt).max(0) as f64 * 0.1;

        // Economic event dynamic widening
        if !self.events.is_empty() {
            let is_near_event = self.events.iter().any(|ev| {
                let adv_ms = match ev.tier {
                    1 => 15_000,
                    2 => 10_000,
                    _ => 5_000,
                };
                mt5_time_msc >= (ev.mt5_ms - adv_ms - 10_000)
                    && mt5_time_msc <= (ev.mt5_ms + ev.recovery_ms)
            });

            if is_near_event {
                let cutoff_10s = mt5_time_msc - 10_000;
                self.recent_ticks.retain(|&(t, _)| t >= cutoff_10s);
                self.recent_ticks.push((mt5_time_msc, q_pt));

                let mut min_q = q_pt;
                let mut max_q = q_pt;
                for &(_, p) in &self.recent_ticks {
                    if p < min_q {
                        min_q = p;
                    }
                    if p > max_q {
                        max_q = p;
                    }
                }
                let price_volatility_10s_pips = (max_q - min_q) as f64 * 0.1;

                for ev in &self.events {
                    let adv_ms = match ev.tier {
                        1 => 15_000,
                        2 => 10_000,
                        3 => 5_000,
                        _ => continue,
                    };
                    let t_start = ev.mt5_ms - adv_ms;
                    let t_end = ev.mt5_ms + ev.recovery_ms;

                    if mt5_time_msc >= t_start && mt5_time_msc <= t_end {
                        let ind_pips = if mt5_time_msc < ev.mt5_ms {
                            let rel_sec = (mt5_time_msc - ev.mt5_ms) as f64 / 1000.0;
                            match ev.tier {
                                1 => {
                                    if rel_sec < -5.0 {
                                        0.2 + (2.5 - 0.2) * ((rel_sec + 15.0) / 10.0).max(0.0)
                                    } else if rel_sec < -1.0 {
                                        2.5 + (3.2 - 2.5) * ((rel_sec + 5.0) / 4.0)
                                    } else {
                                        3.2 + (7.9 - 3.2) * ((rel_sec + 1.0) / 1.0)
                                    }
                                }
                                2 => {
                                    if rel_sec < -1.0 {
                                        0.2 + (2.0 - 0.2) * ((rel_sec + 10.0) / 9.0).max(0.0)
                                    } else {
                                        2.0 + (4.5 - 2.0) * ((rel_sec + 1.0) / 1.0)
                                    }
                                }
                                _ => 0.2 + (1.8 - 0.2) * ((rel_sec + 5.0) / 5.0).max(0.0),
                            }
                        } else if mt5_time_msc <= ev.mt5_ms + 10_000 {
                            let (base_peak_pips, max_cap_pips) = match ev.tier {
                                1 => (7.9, 14.9),
                                2 => (4.5, 8.0),
                                _ => (1.8, 3.5),
                            };
                            let oanda_excess_pips = (raw_oanda_spread_pips - 0.6).max(0.0);
                            let dyn_pips = base_peak_pips
                                + 0.30 * oanda_excess_pips
                                + 0.40 * price_volatility_10s_pips;
                            dyn_pips.min(max_cap_pips)
                        } else {
                            let decay_start_pips = match ev.tier {
                                1 => 4.5,
                                2 => 3.0,
                                _ => 1.2,
                            };
                            let rem_progress = ((mt5_time_msc - (ev.mt5_ms + 10_000)) as f64
                                / ((ev.recovery_ms - 10_000) as f64).max(1.0))
                            .clamp(0.0, 1.0);
                            let decay = (1.0 - rem_progress).powi(2);
                            0.2 + (decay_start_pips - 0.2) * decay
                        };

                        if ind_pips > target_spread_pips {
                            target_spread_pips = ind_pips;
                        }
                    }
                }
            } else if !self.recent_ticks.is_empty() {
                self.recent_ticks.clear();
            }
        }

        // Volatility stress trigger
        if raw_oanda_spread_pips > 0.8 {
            let stress_pips = 0.2 + 0.35 * (raw_oanda_spread_pips - 0.8);
            if stress_pips > target_spread_pips {
                target_spread_pips = stress_pips;
            }
        }

        let clamped_pips = target_spread_pips.clamp(
            jfx_characteristics::BASE_SPREAD_PIPS,
            jfx_characteristics::MAX_SPREAD_PIPS,
        );

        // 5. Point integer space calculation (ask - bid == spread guaranteed)
        let spr_pt = (clamped_pips * 10.0).round().max(2.0) as i64;
        let jfx_bid_pt = q_pt - (spr_pt / 2);
        let jfx_ask_pt = jfx_bid_pt + spr_pt;

        // 6. Strict Change-Driven emission
        if self.last_emitted_bid_pt > 0
            && jfx_bid_pt == self.last_emitted_bid_pt
            && jfx_ask_pt == self.last_emitted_ask_pt
        {
            return None;
        }

        self.last_emitted_bid_pt = jfx_bid_pt;
        self.last_emitted_ask_pt = jfx_ask_pt;

        let jfx_bid = jfx_bid_pt as f64 * self.point_size;
        let jfx_ask = jfx_ask_pt as f64 * self.point_size;
        let final_spread = spr_pt as f64 * self.point_size;

        let norm_bid = (jfx_bid * self.point_scale).round() / self.point_scale;
        let norm_ask = (jfx_ask * self.point_scale).round() / self.point_scale;
        let norm_spr = (final_spread * self.point_scale).round() / self.point_scale;

        Some((norm_bid, norm_ask, norm_spr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pseudo_jfx_zero_delay_and_impulse() {
        let mut engine = PseudoJfxEngine::new("USDJPY", "2026-09");

        // 1. Initial tick
        let t1 = engine.process_tick(1788500000000, 150.000, 150.004);
        assert!(t1.is_some());
        let (b1, a1, spr1) = t1.unwrap();
        assert_eq!(b1, 150.001);
        assert_eq!(a1, 150.003);
        assert_eq!(spr1, 0.002);

        // 2. Flat tick: None (Change-Driven)
        let t1_same = engine.process_tick(1788500000050, 150.000, 150.004);
        assert!(t1_same.is_none());

        // 3. Upward impulse break
        let t3 = engine.process_tick(1788500000150, 150.001, 150.008);
        assert!(t3.is_some());
        let (b3, a3, _) = t3.unwrap();
        assert_eq!(b3, 150.004);
        assert_eq!(a3, 150.006);
    }
}
