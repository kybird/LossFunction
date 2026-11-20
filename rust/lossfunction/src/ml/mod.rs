//! ONNX MLP direction models — Rust inference without Python.
//!
//! Loads every `<symbol>.onnx` in the models directory (exported by
//! `scripts/ml/train_mlp.py`; StandardScaler is folded INTO the graph, so raw
//! log-return features go in). Inference is advisory-only: callers surface
//! probabilities as signals; nothing here touches the order path (wiki:
//! trading-core-determinism).
//!
//! Determinism: models are tiny (5→16→2), single-threaded GEMM — outputs
//! match Python onnxruntime within the fixture tolerance (regression test
//! against `models/regression_fixture.json`).

use std::collections::HashMap;
use std::path::Path;

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

use crate::types::Symbol;

/// Lookbacks of the promoted 5-feature models — MUST match
/// models/meta.json feature_names (the fixture regression test guards the
/// pairing: a width mismatch fails loudly at inference).
pub const LOOKBACKS: [usize; 5] = [1, 5, 20, 60, 120];

/// ONNX input name (see models/meta.json — single f32[N, 5] "input").
const INPUT_NAME: &str = "input";
/// ONNX classifier outputs: int64 label, then f32 probabilities [N, 2].
const PROBABILITIES_OUTPUT: &str = "probabilities";

#[derive(thiserror::Error, Debug)]
pub enum MlpError {
    #[error("ort: {0}")]
    Ort(#[from] ort::Error),
    #[error("ONNX Runtime environment init failed: {0}")]
    EnvironmentInit(String),
    #[error("no .onnx models found in {0}")]
    NoModels(String),
    #[error("missing model for {0}")]
    MissingModel(Symbol),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Causal raw features from completed-bar closes (oldest first):
/// log_return over [`LOOKBACKS`] at the latest bar — mirrors the training
/// FeatureBuilder (uses bars at or before t only).
pub fn raw_log_returns(closes: &[f64]) -> Option<[f32; LOOKBACKS.len()]> {
    let last = closes.len().checked_sub(1)?;
    let log_close = |i: usize| closes[i].ln();
    let mut features = [0f32; LOOKBACKS.len()];
    for (slot, lookback) in LOOKBACKS.iter().enumerate() {
        let base = last.checked_sub(*lookback)?;
        features[slot] = (log_close(last) - log_close(base)) as f32;
    }
    Some(features)
}

/// One ONNX Runtime environment per process (ort panics on a second one) —
/// shared by every `OnnxMlpDirection` load. The init result is memoized so
/// late failures surface identically on every call.
fn shared_environment() -> Result<Environment, MlpError> {
    static ENV: std::sync::OnceLock<Result<Environment, String>> = std::sync::OnceLock::new();
    ENV.get_or_init(|| ort::init().build().map_err(|error| error.to_string()))
        .clone()
        .map_err(MlpError::EnvironmentInit)
}

/// Per-symbol sessions over the exported ONNX models. Symbols are discovered
/// from the models directory, so adding/rotating instruments needs no code
/// change — retrain and drop the files in.
pub struct OnnxMlpDirection {
    #[allow(dead_code)] // keeps the ONNX Runtime environment alive with the sessions
    environment: Environment,
    sessions: HashMap<Symbol, std::sync::Mutex<Session>>,
}

impl OnnxMlpDirection {
    /// Load every `<symbol>.onnx` in `models_dir` (file stem = symbol code).
    pub fn load(models_dir: &Path) -> Result<Self, MlpError> {
        let environment = shared_environment()?;
        let mut sessions = HashMap::new();
        for entry in models_dir.read_dir()? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("onnx") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(symbol) = Symbol::parse(stem).ok() else {
                continue;
            };
            sessions.insert(
                symbol,
                std::sync::Mutex::new(Session::builder(&environment)?.commit_from_file(&path)?),
            );
        }
        if sessions.is_empty() {
            return Err(MlpError::NoModels(models_dir.display().to_string()));
        }
        Ok(Self {
            environment,
            sessions,
        })
    }

    /// P(up) for one symbol from RAW features — the scaler lives in the graph.
    pub fn prob_up(&self, symbol: &Symbol, features: &[f32]) -> Result<f32, MlpError> {
        let mut session = self
            .sessions
            .get(symbol)
            .ok_or_else(|| MlpError::MissingModel(symbol.clone()))?
            .lock()
            .expect("mlp session lock");
        let input = Tensor::from_array(([1usize, LOOKBACKS.len()], features.to_vec()))?;
        let outputs = session.run(ort::inputs![INPUT_NAME => &input])?;
        let (_, probabilities) = outputs[PROBABILITIES_OUTPUT].try_extract_tensor::<f32>()?;
        // Row-major [1, 2]: index 1 is P(class=1) = P(next bar up).
        Ok(probabilities[1])
    }

    /// P(up) from completed-bar closes for one symbol, when history suffices.
    pub fn prob_up_from_closes(
        &self,
        symbol: &Symbol,
        closes: &[f64],
    ) -> Result<Option<f32>, MlpError> {
        match raw_log_returns(closes) {
            Some(features) => self.prob_up(symbol, &features).map(Some),
            None => Ok(None), // not enough bars yet — no signal, no error
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn models_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models")
    }

    /// Synthetic closes with the pattern i -> 100*(1 + 0.01*(((i*7)%13)-6)/6),
    /// 150 bars — enough for the 120 lookback. Expected features cross-checked
    /// against the training pipeline's numpy math (2026-11-20).
    fn synthetic_closes() -> Vec<f64> {
        (0..150)
            .map(|i| 100.0 * (1.0 + 0.01 * ((((i * 7) % 13) - 6) as f64) / 6.0))
            .collect()
    }
    const EXPECTED_FEATURES: [f32; 5] = [
        -0.010000083334583465,
        -0.006677821142606177,
        -0.005012541823544936,
        -0.014962872676712813,
        -0.008340331916219412,
    ];

    #[test]
    fn raw_log_returns_match_python_reference() {
        let closes = synthetic_closes();
        let features = raw_log_returns(&closes).expect("150 closes cover the 120 lookback");
        for (got, want) in features.iter().zip(EXPECTED_FEATURES) {
            assert!(
                (got - want).abs() < 1e-7,
                "feature drift: got {got}, want {want}"
            );
        }
        // Causality: identical tail history must give identical features even
        // when older bars differ.
        let mut older = vec![5000.0; 40];
        older.extend_from_slice(&closes);
        assert_eq!(raw_log_returns(&older), raw_log_returns(&closes));
        // Insufficient history yields no signal, not an error.
        assert!(raw_log_returns(&closes[..120]).is_none());
    }

    /// AC1 — fixed inputs must reproduce the Python onnxruntime fixture
    /// (models/regression_fixture.json) within its stated tolerance, for
    /// every symbol the fixture carries.
    #[test]
    fn fixture_regression_matches_python_onnxruntime() {
        let models = models_dir();
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(models.join("regression_fixture.json"))
                .expect("committed fixture"),
        )
        .expect("fixture parses");
        let tolerance = fixture["tolerance"].as_f64().expect("tolerance") as f32;

        let inputs: Vec<Vec<f32>> = fixture["inputs"]
            .as_array()
            .expect("inputs array")
            .iter()
            .map(|row| {
                row.as_array()
                    .expect("row")
                    .iter()
                    .map(|value| value.as_f64().expect("f64") as f32)
                    .collect()
            })
            .collect();
        assert_eq!(
            inputs[0].len(),
            LOOKBACKS.len(),
            "fixture width must match the promoted lookback set"
        );

        let model = OnnxMlpDirection::load(&models).expect("models load");
        for (raw, expected) in fixture["expected"].as_object().expect("expected map") {
            let symbol = Symbol::parse(raw).expect("fixture symbol parses");
            let labels: Vec<i64> = expected["label"]
                .as_array()
                .expect("labels")
                .iter()
                .map(|v| v.as_i64().expect("i64"))
                .collect();
            let probs: Vec<f32> = expected["probabilities"]
                .as_array()
                .expect("probabilities")
                .iter()
                .map(|row| row[1].as_f64().expect("f64") as f32) // P(up)
                .collect();

            for (i, features) in inputs.iter().enumerate() {
                let probability = model
                    .prob_up(&symbol, features)
                    .expect("inference succeeds");
                assert!(
                    (probability - probs[i]).abs() < tolerance,
                    "{raw} row {i}: got {probability}, want {}",
                    probs[i]
                );
                let predicted_up = (probability >= 0.5) as i64;
                assert_eq!(predicted_up, labels[i], "{raw} row {i}: label mismatch");
            }
        }
    }
}
