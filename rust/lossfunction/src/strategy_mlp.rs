//! MLP advisory strategy — ort-path inference as signals, never orders.
//!
//! Implements the same `Strategy` trait as the tradable strategies but
//! emits ZERO intents by construction: per-symbol P(next bar up) rides in
//! `features` for audit/replay, the order path (risk gates, kill switch)
//! never sees it. GLM advisories are expected to join the same channel
//! (wiki: trading-core-determinism — model output is advisory input only).

use std::sync::Arc;

use rust_decimal::prelude::ToPrimitive;

use crate::ml::OnnxMlpDirection;
use crate::strategy::{MarketSnapshot, Strategy, StrategyDecision};
use crate::types::Symbol;

/// Advisory MLP direction strategy over the exported ONNX models.
pub struct MlpAdvisoryStrategy {
    model: Arc<OnnxMlpDirection>,
}

impl MlpAdvisoryStrategy {
    pub fn new(model: Arc<OnnxMlpDirection>) -> Self {
        Self { model }
    }
}

impl Strategy for MlpAdvisoryStrategy {
    fn name(&self) -> &str {
        "mlp-advisory"
    }

    fn version(&self) -> &str {
        "1"
    }

    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let mut features = std::collections::BTreeMap::new();
        let mut covered = 0usize;
        let mut symbols: Vec<&Symbol> = snapshot.history.keys().collect();
        symbols.sort_unstable(); // deterministic iteration over HashMap keys

        for symbol in symbols {
            let closes: Vec<f64> = snapshot.history[symbol]
                .iter()
                .map(|price| price.to_f64().unwrap_or(f64::NAN))
                .collect();
            if closes.iter().any(|value| value.is_nan()) {
                features.insert(format!("mlp_skip_{symbol}"), "non-numeric close".into());
                continue;
            }
            match self.model.prob_up_from_closes(symbol, &closes) {
                Ok(Some(probability)) => {
                    features.insert(format!("mlp_prob_up_{symbol}"), format!("{probability:.6}"));
                    covered += 1;
                }
                Ok(None) => {
                    features.insert(format!("mlp_skip_{symbol}"), "insufficient history".into());
                }
                Err(error) => {
                    // Fail visible, never loud: the advisory channel carries
                    // the error as data; the trading core keeps running.
                    features.insert(format!("mlp_error_{symbol}"), error.to_string());
                }
            }
        }

        StrategyDecision {
            strategy_name: self.name().into(),
            strategy_version: self.version().into(),
            intents: Vec::new(), // advisory-only: this strategy never orders
            features,
            rationale: format!(
                "MLP advisory (ort): {covered} symbol(s) scored, 0 intents by design"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ml::OnnxMlpDirection;
    use crate::strategy::MarketSnapshot;
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn models_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models")
    }

    fn snapshot_with_history(closes: &[f64]) -> MarketSnapshot {
        let mut history = HashMap::new();
        history.insert(
            Symbol::parse("005930").expect("symbol"),
            closes
                .iter()
                .map(|c| Decimal::from_f64_retain(*c).expect("decimal"))
                .collect(),
        );
        MarketSnapshot {
            history,
            ..MarketSnapshot::default()
        }
    }

    /// Python onnxruntime reference for this history (2026-11-20):
    /// 005930 prob_up = 0.4524281621 (raw features [1,5,20]).
    #[test]
    fn advisory_signal_comes_from_the_ort_path_and_never_orders() {
        let closes: Vec<f64> = (0..30)
            .map(|i| 100.0 * (1.0 + 0.01 * (((i * 7) % 13) - 6) as f64 / 6.0))
            .collect();
        let model = Arc::new(OnnxMlpDirection::load(&models_dir()).expect("models load"));
        let strategy = MlpAdvisoryStrategy::new(Arc::clone(&model));

        let snapshot = snapshot_with_history(&closes);
        let decision = strategy.decide(&snapshot);

        assert!(decision.intents.is_empty(), "advisory must not order");
        let feature = decision
            .features
            .get("mlp_prob_up_005930")
            .expect("probability feature present");
        let probability: f64 = feature.parse().expect("parses as f64");
        assert!(
            (probability - 0.4524281621).abs() < 1e-5,
            "ort-path probability drifted from the Python reference: {probability}"
        );

        // Determinism: same snapshot in, same decision out.
        let again = strategy.decide(&snapshot);
        assert_eq!(decision, again);
    }

    #[test]
    fn short_history_is_skipped_not_failed() {
        let model = Arc::new(OnnxMlpDirection::load(&models_dir()).expect("models load"));
        let strategy = MlpAdvisoryStrategy::new(model);
        let snapshot = snapshot_with_history(&[100.0; 15]);
        let decision = strategy.decide(&snapshot);
        assert!(decision.intents.is_empty());
        assert_eq!(
            decision.features.get("mlp_skip_005930").map(String::as_str),
            Some("insufficient history")
        );
    }
}
