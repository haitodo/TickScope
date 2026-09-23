//! Broker Behaviour Hypothesis Layer.
//! Reference: RFC Beta 0.3 (docs/improvement.md Sections 48-51, 73, 77-80, 91 Invariants I6, I12, I13).
//!
//! RFC Core Invariants:
//! - NEVER assign confidence percentages or probabilities (e.g., "87% Sticky").
//! - NEVER hypothesize LP identity (no "LP #1", "Barclays LP", etc.).
//! - NEVER produce trading signals or recommendations (BUY, SELL, ENTRY, EXIT).
//! - Always present hypotheses with explicit, verifiable evidence chains (RFC Section 50).

use crate::contracts::types::*;
use crate::metrics::fingerprint::{
    BrokerFingerprint, QuotePersistenceTracker, RepricingPersistenceTracker, SampleContext,
};
use serde::{Deserialize, Serialize};

/// Hypothesis categories for broker feed behavior (RFC Beta 0.3 Sections 48, 49, 78).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HypothesisType {
    PossibleFiltering,
    PossibleAggregation,
    PossibleDelayedRepricing,
    PossibleStickyPricing,
    PossibleSpreadFirstResponse,
}

impl HypothesisType {
    /// Human-readable default title prefixed with "Possible " (RFC Section 48, 78).
    pub const fn default_title(&self) -> &'static str {
        match self {
            Self::PossibleFiltering => "Possible Quote Filtering",
            Self::PossibleAggregation => "Possible Feed Aggregation",
            Self::PossibleDelayedRepricing => "Possible Delayed Repricing",
            Self::PossibleStickyPricing => "Possible Sticky Pricing",
            Self::PossibleSpreadFirstResponse => "Possible Spread-First Response",
        }
    }
}

/// Structured evidence supporting a hypothesis (RFC Beta 0.3 Sections 50, 51).
/// All fields are objective mathematical measurements without confidence scores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceChain {
    pub sample_count: u64,
    pub median_lag_ms: Option<f64>,
    pub concordance_ratio: Option<f64>,
    pub fresh_rate: f64,
    pub dispersion_points: Option<f64>,
    pub items: Vec<String>,
}

impl EvidenceChain {
    pub fn new(sample_count: u64, fresh_rate: f64) -> Self {
        Self {
            sample_count,
            median_lag_ms: None,
            concordance_ratio: None,
            fresh_rate,
            dispersion_points: None,
            items: Vec::new(),
        }
    }

    pub fn with_lag(mut self, lag_ms: f64) -> Self {
        self.median_lag_ms = Some(lag_ms);
        self
    }

    pub fn with_concordance(mut self, ratio: f64) -> Self {
        self.concordance_ratio = Some(ratio);
        self
    }

    pub fn with_dispersion(mut self, dispersion: f64) -> Self {
        self.dispersion_points = Some(dispersion);
        self
    }

    pub fn with_item(mut self, item: impl Into<String>) -> Self {
        self.items.push(item.into());
        self
    }

    pub fn add_item(&mut self, item: impl Into<String>) {
        self.items.push(item.into());
    }
}

/// A behavioral hypothesis regarding a broker's quote feed.
/// Always attaches statistical sample context and an evidence chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub broker_id: BrokerId,
    pub hypothesis_type: HypothesisType,
    pub title: String,
    pub evidence: EvidenceChain,
    pub sample_context: SampleContext,
}

impl Hypothesis {
    pub fn new(
        broker_id: BrokerId,
        hypothesis_type: HypothesisType,
        evidence: EvidenceChain,
        sample_context: SampleContext,
    ) -> Self {
        let title = hypothesis_type.default_title().to_string();
        Self {
            broker_id,
            hypothesis_type,
            title,
            evidence,
            sample_context,
        }
    }

    pub fn with_custom_title(
        broker_id: BrokerId,
        hypothesis_type: HypothesisType,
        title: impl Into<String>,
        evidence: EvidenceChain,
        sample_context: SampleContext,
    ) -> Self {
        Self {
            broker_id,
            hypothesis_type,
            title: title.into(),
            evidence,
            sample_context,
        }
    }
}

/// Configuration thresholds for the `HypothesisEngine`.
#[derive(Debug, Clone)]
pub struct HypothesisEngineConfig {
    /// Low-N guard: minimum sample count required before hypothesis consideration (RFC Section 70).
    pub min_sample_count: u64,
    /// Minimum fresh rate required to consider observations reliable.
    pub min_fresh_rate: f64,
    /// Follow frequency threshold for Delayed Repricing.
    pub delayed_repricing_min_follow_freq: f64,
    /// Median delay threshold in milliseconds for Delayed Repricing.
    pub delayed_repricing_min_delay_ms: f64,
    /// Minimum quote persistence in milliseconds for Sticky Pricing.
    pub sticky_pricing_min_persistence_ms: f64,
    /// Follow rate threshold after lead for Sticky Pricing.
    pub sticky_pricing_min_follow_rate: f64,
    /// Maximum reversion rate threshold for Sticky Pricing.
    pub sticky_pricing_max_reversion_rate: f64,
    /// Spread expansion frequency threshold for Spread-First Response.
    pub spread_first_min_expansion_freq: f64,
    /// Stale frequency threshold for Quote Filtering.
    pub filtering_min_stale_freq: f64,
    /// Quote persistence threshold in milliseconds for Feed Aggregation.
    pub aggregation_persistence_threshold_ms: f64,
}

impl Default for HypothesisEngineConfig {
    fn default() -> Self {
        Self {
            min_sample_count: 10,
            min_fresh_rate: 0.80,
            delayed_repricing_min_follow_freq: 0.35,
            delayed_repricing_min_delay_ms: 5.0,
            sticky_pricing_min_persistence_ms: 30.0,
            sticky_pricing_min_follow_rate: 0.50,
            sticky_pricing_max_reversion_rate: 0.20,
            spread_first_min_expansion_freq: 0.25,
            filtering_min_stale_freq: 0.15,
            aggregation_persistence_threshold_ms: 100.0,
        }
    }
}

/// Evaluates broker fingerprints, quote persistence, and repricing persistence
/// to produce candidate hypotheses with attached evidence chains.
#[derive(Debug, Clone)]
pub struct HypothesisEngine {
    config: HypothesisEngineConfig,
}

impl Default for HypothesisEngine {
    fn default() -> Self {
        Self::new(HypothesisEngineConfig::default())
    }
}

impl HypothesisEngine {
    pub fn new(config: HypothesisEngineConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &HypothesisEngineConfig {
        &self.config
    }

    /// Evaluates a broker's fingerprint along with optional persistence and repricing trackers.
    /// Returns all candidate hypotheses meeting observational evidence criteria.
    pub fn evaluate(
        &self,
        fingerprint: &BrokerFingerprint,
        repricing: Option<&RepricingPersistenceTracker>,
        persistence: Option<&QuotePersistenceTracker>,
    ) -> Vec<Hypothesis> {
        let mut hypotheses = Vec::new();

        // Low-N guard: reject evaluation if sample count is insufficient (RFC Section 70)
        if fingerprint.sample_context.sample_count < self.config.min_sample_count {
            return hypotheses;
        }

        // 1. Possible Delayed Repricing
        if let Some(h) = self.eval_delayed_repricing(fingerprint) {
            hypotheses.push(h);
        }

        // 2. Possible Sticky Pricing
        if let Some(h) = self.eval_sticky_pricing(fingerprint, repricing, persistence) {
            hypotheses.push(h);
        }

        // 3. Possible Spread-First Response
        if let Some(h) = self.eval_spread_first_response(fingerprint) {
            hypotheses.push(h);
        }

        // 4. Possible Quote Filtering
        if let Some(h) = self.eval_quote_filtering(fingerprint, persistence) {
            hypotheses.push(h);
        }

        // 5. Possible Feed Aggregation
        if let Some(h) = self.eval_feed_aggregation(fingerprint, persistence) {
            hypotheses.push(h);
        }

        hypotheses
    }

    /// Evaluates only from fingerprint when trackers are not available.
    pub fn evaluate_fingerprint(&self, fingerprint: &BrokerFingerprint) -> Vec<Hypothesis> {
        self.evaluate(fingerprint, None, None)
    }

    fn eval_delayed_repricing(&self, fp: &BrokerFingerprint) -> Option<Hypothesis> {
        if fp.observed_follow_freq >= self.config.delayed_repricing_min_follow_freq
            && fp.median_delay_ms >= self.config.delayed_repricing_min_delay_ms
            && fp.sample_context.fresh_rate >= self.config.min_fresh_rate
        {
            let mut evidence =
                EvidenceChain::new(fp.sample_context.sample_count, fp.sample_context.fresh_rate)
                    .with_lag(fp.median_delay_ms);

            evidence.add_item(format!(
                "{} comparable events evaluated",
                fp.sample_context.sample_count
            ));
            evidence.add_item(format!("median observed lag {:.1}ms", fp.median_delay_ms));
            evidence.add_item(format!(
                "observed follow frequency {:.1}%",
                fp.observed_follow_freq * 100.0
            ));
            evidence.add_item(format!(
                "fresh rate {:.1}%",
                fp.sample_context.fresh_rate * 100.0
            ));
            if fp.consensus_deviation_pips < 1.0 {
                evidence.add_item("low consensus deviation during repricing".to_string());
            }

            Some(Hypothesis::new(
                fp.broker_id,
                HypothesisType::PossibleDelayedRepricing,
                evidence,
                fp.sample_context,
            ))
        } else {
            None
        }
    }

    fn eval_sticky_pricing(
        &self,
        fp: &BrokerFingerprint,
        repricing: Option<&RepricingPersistenceTracker>,
        persistence: Option<&QuotePersistenceTracker>,
    ) -> Option<Hypothesis> {
        let mut evidence_items = Vec::new();
        let mut concordance = None;

        let has_repricing_evidence = if let Some(rep) = repricing {
            if rep.follow_count() >= 3
                && rep.follow_rate() >= self.config.sticky_pricing_min_follow_rate
                && rep.reversion_rate() <= self.config.sticky_pricing_max_reversion_rate
            {
                concordance = Some(rep.follow_rate());
                evidence_items.push(format!(
                    "follow frequency {:.1}%, reversion frequency {:.1}%",
                    rep.follow_rate() * 100.0,
                    rep.reversion_rate() * 100.0
                ));
                true
            } else {
                false
            }
        } else {
            false
        };

        let has_persistence_evidence = if let Some(pers) = persistence {
            if let Some(med_p) = pers.median_persistence_ms() {
                if med_p >= self.config.sticky_pricing_min_persistence_ms {
                    evidence_items.push(format!("median quote persistence {:.1}ms", med_p));
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        let fallback_evidence = !has_repricing_evidence
            && !has_persistence_evidence
            && fp.observed_lead_freq >= 0.40
            && fp.stale_freq <= 0.05
            && fp.median_delay_ms <= 5.0;

        if has_repricing_evidence || has_persistence_evidence || fallback_evidence {
            let mut evidence =
                EvidenceChain::new(fp.sample_context.sample_count, fp.sample_context.fresh_rate);

            if let Some(r) = concordance {
                evidence = evidence.with_concordance(r);
            }

            evidence.add_item(format!(
                "{} comparable moves evaluated",
                fp.sample_context.sample_count
            ));
            for item in evidence_items {
                evidence.add_item(item);
            }
            if fallback_evidence {
                evidence.add_item(format!(
                    "observed lead frequency {:.1}% with stable prices",
                    fp.observed_lead_freq * 100.0
                ));
            }
            evidence.add_item(format!(
                "fresh rate {:.1}%",
                fp.sample_context.fresh_rate * 100.0
            ));

            Some(Hypothesis::new(
                fp.broker_id,
                HypothesisType::PossibleStickyPricing,
                evidence,
                fp.sample_context,
            ))
        } else {
            None
        }
    }

    fn eval_spread_first_response(&self, fp: &BrokerFingerprint) -> Option<Hypothesis> {
        if fp.spread_expansion_freq >= self.config.spread_first_min_expansion_freq {
            let mut evidence =
                EvidenceChain::new(fp.sample_context.sample_count, fp.sample_context.fresh_rate);

            evidence.add_item(format!(
                "{} observation samples evaluated",
                fp.sample_context.sample_count
            ));
            evidence.add_item(format!(
                "spread expansion frequency {:.1}% during quote adjustments",
                fp.spread_expansion_freq * 100.0
            ));
            evidence.add_item(format!(
                "fresh rate {:.1}%",
                fp.sample_context.fresh_rate * 100.0
            ));

            Some(Hypothesis::new(
                fp.broker_id,
                HypothesisType::PossibleSpreadFirstResponse,
                evidence,
                fp.sample_context,
            ))
        } else {
            None
        }
    }

    fn eval_quote_filtering(
        &self,
        fp: &BrokerFingerprint,
        persistence: Option<&QuotePersistenceTracker>,
    ) -> Option<Hypothesis> {
        let is_stale_high = fp.stale_freq >= self.config.filtering_min_stale_freq;
        let is_persistence_high = persistence
            .and_then(|p| p.median_persistence_ms())
            .map(|m| m >= 250.0)
            .unwrap_or(false);

        if is_stale_high || is_persistence_high {
            let mut evidence =
                EvidenceChain::new(fp.sample_context.sample_count, fp.sample_context.fresh_rate);

            evidence.add_item(format!(
                "{} quotes observed across {}ms window",
                fp.sample_context.sample_count, fp.sample_context.window_ms
            ));
            if is_stale_high {
                evidence.add_item(format!(
                    "stale quote frequency {:.1}% exceeds baseline",
                    fp.stale_freq * 100.0
                ));
            }
            if is_persistence_high {
                if let Some(med_p) = persistence.and_then(|p| p.median_persistence_ms()) {
                    evidence.add_item(format!(
                        "elevated median quote persistence {:.1}ms indicates selective update filtering",
                        med_p
                    ));
                }
            }
            evidence.add_item(format!(
                "fresh rate {:.1}%",
                fp.sample_context.fresh_rate * 100.0
            ));

            Some(Hypothesis::new(
                fp.broker_id,
                HypothesisType::PossibleFiltering,
                evidence,
                fp.sample_context,
            ))
        } else {
            None
        }
    }

    fn eval_feed_aggregation(
        &self,
        fp: &BrokerFingerprint,
        persistence: Option<&QuotePersistenceTracker>,
    ) -> Option<Hypothesis> {
        let is_persistence_quantized = persistence
            .and_then(|p| p.median_persistence_ms())
            .map(|m| m >= self.config.aggregation_persistence_threshold_ms)
            .unwrap_or(false);

        let is_pattern_clustered =
            fp.median_delay_ms >= 15.0 && fp.outlier_freq <= 0.02 && fp.observed_follow_freq >= 0.40;

        if is_persistence_quantized || is_pattern_clustered {
            let mut evidence =
                EvidenceChain::new(fp.sample_context.sample_count, fp.sample_context.fresh_rate);

            evidence.add_item(format!(
                "{} quote samples analyzed across {}ms window",
                fp.sample_context.sample_count, fp.sample_context.window_ms
            ));
            evidence.add_item(
                "quote revisions exhibit discrete batching or time-window aggregation".to_string(),
            );
            if is_persistence_quantized {
                if let Some(med_p) = persistence.and_then(|p| p.median_persistence_ms()) {
                    evidence.add_item(format!(
                        "measured median persistence of {:.1}ms matches batch aggregation threshold",
                        med_p
                    ));
                }
            }
            evidence.add_item(format!(
                "fresh rate {:.1}%",
                fp.sample_context.fresh_rate * 100.0
            ));

            Some(Hypothesis::new(
                fp.broker_id,
                HypothesisType::PossibleAggregation,
                evidence,
                fp.sample_context,
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delayed_repricing_hypothesis_generation() {
        let engine = HypothesisEngine::default();

        let ctx = SampleContext::new(37, 10_000, 0.992);
        let fp = BrokerFingerprint::new(
            1,
            0.15,
            0.65, // observed_follow_freq
            8.2,  // median_delay_ms
            0.10,
            0.01,
            0.0,
            0.4,
            ctx,
        );

        let hypotheses = engine.evaluate(&fp, None, None);
        assert_eq!(hypotheses.len(), 1);

        let h = &hypotheses[0];
        assert_eq!(h.broker_id, 1);
        assert_eq!(h.hypothesis_type, HypothesisType::PossibleDelayedRepricing);
        assert_eq!(h.title, "Possible Delayed Repricing");
        assert_eq!(h.evidence.sample_count, 37);
        assert_eq!(h.evidence.median_lag_ms, Some(8.2));
        assert_eq!(h.evidence.fresh_rate, 0.992);

        // Verify evidence lines match RFC Section 50 example
        assert!(h.evidence.items.iter().any(|i| i.contains("37 comparable events")));
        assert!(h.evidence.items.iter().any(|i| i.contains("median observed lag 8.2ms")));
        assert!(h.evidence.items.iter().any(|i| i.contains("fresh rate 99.2%")));
    }

    #[test]
    fn test_sticky_pricing_hypothesis_generation() {
        let engine = HypothesisEngine::default();

        let ctx = SampleContext::new(2481, 60_000, 0.995);
        let fp = BrokerFingerprint::new(2, 0.55, 0.20, 2.1, 0.08, 0.01, 0.0, 0.2, ctx);

        let mut repricing = RepricingPersistenceTracker::new(2);
        for _ in 0..72 {
            repricing.record_outcome(true, false);
        }
        for _ in 0..4 {
            repricing.record_outcome(false, true);
        }
        for _ in 0..24 {
            repricing.record_outcome(false, false);
        }

        let mut persistence = QuotePersistenceTracker::new(2);
        persistence.record_duration_ms(38.0);
        persistence.record_duration_ms(40.0);
        persistence.record_duration_ms(36.0);

        let hypotheses = engine.evaluate(&fp, Some(&repricing), Some(&persistence));
        let sticky = hypotheses
            .iter()
            .find(|h| h.hypothesis_type == HypothesisType::PossibleStickyPricing)
            .expect("Should generate PossibleStickyPricing");

        assert_eq!(sticky.title, "Possible Sticky Pricing");
        assert_eq!(sticky.evidence.sample_count, 2481);
        assert!(sticky
            .evidence
            .items
            .iter()
            .any(|i| i.contains("median quote persistence")));
        assert!(sticky
            .evidence
            .items
            .iter()
            .any(|i| i.contains("follow frequency")));
    }

    #[test]
    fn test_spread_first_response_hypothesis_generation() {
        let engine = HypothesisEngine::default();

        let ctx = SampleContext::new(50, 5_000, 0.98);
        let fp = BrokerFingerprint::new(
            3,
            0.30,
            0.30,
            4.0,
            0.45, // high spread expansion frequency
            0.02,
            0.0,
            0.5,
            ctx,
        );

        let hypotheses = engine.evaluate(&fp, None, None);
        let spread_first = hypotheses
            .iter()
            .find(|h| h.hypothesis_type == HypothesisType::PossibleSpreadFirstResponse)
            .expect("Should generate PossibleSpreadFirstResponse");

        assert_eq!(spread_first.title, "Possible Spread-First Response");
        assert!(spread_first
            .evidence
            .items
            .iter()
            .any(|i| i.contains("spread expansion frequency 45.0%")));
    }

    #[test]
    fn test_quote_filtering_and_aggregation_hypothesis() {
        let engine = HypothesisEngine::default();

        let ctx = SampleContext::new(100, 20_000, 0.95);
        let fp = BrokerFingerprint::new(
            4,
            0.05,
            0.15,
            18.0,
            0.05,
            0.22, // elevated stale frequency
            0.0,
            0.8,
            ctx,
        );

        let mut persistence = QuotePersistenceTracker::new(4);
        persistence.record_duration_ms(150.0);
        persistence.record_duration_ms(160.0);

        let hypotheses = engine.evaluate(&fp, None, Some(&persistence));
        assert!(hypotheses
            .iter()
            .any(|h| h.hypothesis_type == HypothesisType::PossibleFiltering));
        assert!(hypotheses
            .iter()
            .any(|h| h.hypothesis_type == HypothesisType::PossibleAggregation));
    }

    #[test]
    fn test_low_n_guard_suppresses_hypotheses() {
        let engine = HypothesisEngine::default();

        // Sample count is 5, but min_sample_count is 10
        let ctx = SampleContext::new(5, 1_000, 1.0);
        let fp = BrokerFingerprint::new(1, 0.1, 0.8, 12.0, 0.5, 0.3, 0.0, 0.5, ctx);

        let hypotheses = engine.evaluate(&fp, None, None);
        assert!(
            hypotheses.is_empty(),
            "Low sample count (n=5) must suppress hypotheses under RFC Section 70"
        );
    }

    #[test]
    fn test_rfc_invariants_no_confidence_no_lp_no_signals() {
        let engine = HypothesisEngine::default();
        let ctx = SampleContext::new(100, 10_000, 0.99);
        let fp = BrokerFingerprint::new(1, 0.5, 0.5, 8.0, 0.35, 0.18, 0.0, 0.3, ctx);

        let mut repricing = RepricingPersistenceTracker::new(1);
        for _ in 0..10 {
            repricing.record_outcome(true, false);
        }

        let mut persistence = QuotePersistenceTracker::new(1);
        persistence.record_duration_ms(120.0);

        let hypotheses = engine.evaluate(&fp, Some(&repricing), Some(&persistence));
        assert!(!hypotheses.is_empty());

        for h in hypotheses {
            // Title must always start with "Possible "
            assert!(
                h.title.starts_with("Possible "),
                "Title must start with 'Possible '"
            );

            // Invariant I6: Never hypothesize LP identity
            assert!(
                !h.title.to_lowercase().contains("lp"),
                "Hypothesis title must not hypothesize LP identity"
            );

            for item in &h.evidence.items {
                let lower = item.to_lowercase();
                // RFC Section 51, 80: No confidence score or probabilities
                assert!(
                    !lower.contains("confidence"),
                    "Evidence item must not contain 'confidence': {item}"
                );
                assert!(
                    !lower.contains("probability"),
                    "Evidence item must not contain 'probability': {item}"
                );

                // Invariant I13: No trading signals
                assert!(
                    !lower.contains("buy")
                        && !lower.contains("sell")
                        && !lower.contains("entry")
                        && !lower.contains("exit")
                        && !lower.contains("signal"),
                    "Evidence item must not contain trading signals: {item}"
                );

                // Invariant I6: Never hypothesize LP identity
                assert!(
                    !lower.contains("lp #") && !lower.contains("lp identity"),
                    "Evidence item must not hypothesize LP identity: {item}"
                );
            }
        }
    }
}
