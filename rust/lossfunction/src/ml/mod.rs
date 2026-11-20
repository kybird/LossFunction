//! ONNX MLP direction models — Rust inference without Python.
//!
//! Loads the per-symbol models exported by `scripts/ml/train_mlp.py`
//! (StandardScaler is folded INTO the graph, so raw log-return features go
//! in). Inference is advisory-only: callers surface probabilities as
//! signals; nothing here touches the order path (wiki:
//! trading-core-determinism).
//!
//! Determinism: models are tiny (3→16→2), single-threaded GEMM — outputs
//! match Python onnxruntime within the fixture tolerance (regression test
//! against `models/regression_fixture.json`).

use std::collections::HashMap;
use std::path::Path;

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

use crate::types::Symbol;

/// Symbols the training pipeline exports (models/<symbol>.onnx).
pub const MODEL_SYMBOLS: [&str; 3] = ["005930", "035420", "069500"];

/// ONNX input name (see models/meta.json — single f32[N, 3] "input").
const INPUT_NAME: &str = "input";
/// ONNX classifier outputs: int64 label, then f32 probabilities [N, 2].
const PROBABILITIES_OUTPUT: &str = "probabilities";

#[derive(thiserror::Error, Debug)]
pub enum MlpError {
    #[error("ort: {0}")]
    Ort(#[from] ort::Error),
    #[error("ONNX Runtime environment init failed: {0}")]
    EnvironmentInit(String),
    #[error("missing model for {0}")]
    MissingModel(Symbol),
}

/// Causal raw features from completed-bar closes (oldest first):
/// log_return over lookbacks 1/5/20 at the latest bar — mirrors the
/// Python FeatureBuilder (uses bars at or before t only).
pub fn raw_log_returns(closes: &[f64]) -> Option<[f32; 3]> {
    const LOOKBACKS: [usize; 3] = [1, 5, 20];
    let last = closes.len().checked_sub(1)?;
    let log_close = |i: usize| closes[i].ln();
    let mut features = [0f32; 3];
    for (slot, lookback) in LOOKBACKS.iter().enumerate() {
        let base = last.checked_sub(*lookback)?;
        features[slot] = (log_close(last) - log_close(base)) as f32;
    }
    Some(features)
}

/// Per-symbol sessions over the exported ONNX models.
pub struct OnnxMlpDirection {
    #[allow(dead_code)] // keeps the ONNX Runtime environment alive with the sessions
    environment: Environment,
    // ort's `run` needs &mut Session; advisory inference is serialized —
    // determinism-friendly and plenty for signal cadence.
    sessions: HashMap<Symbol, std::sync::Mutex<Session>>,
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

impl OnnxMlpDirection {
    /// Load `<models_dir>/<symbol>.onnx` for every [`MODEL_SYMBOLS`].
    pub fn load(models_dir: &Path) -> Result<Self, MlpError> {
        let environment = shared_environment()?;
        let mut sessions = HashMap::new();
        for raw in MODEL_SYMBOLS {
            let symbol = Symbol::parse(raw).expect("static symbol parses");
            let path = models_dir.join(format!("{raw}.onnx"));
            sessions.insert(
                symbol,
                std::sync::Mutex::new(Session::builder(&environment)?.commit_from_file(path)?),
            );
        }
        Ok(Self {
            environment,
            sessions,
        })
    }

    /// P(up) for one symbol from RAW features — the scaler lives in the graph.
    pub fn prob_up(&self, symbol: &Symbol, features: [f32; 3]) -> Result<f32, MlpError> {
        let mut session = self
            .sessions
            .get(symbol)
            .ok_or_else(|| MlpError::MissingModel(symbol.clone()))?
            .lock()
            .expect("mlp session lock");
        let input = Tensor::from_array(([1usize, 3], features.to_vec()))?;
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
            Some(features) => self.prob_up(symbol, features).map(Some),
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

    /// Closes cross-checked against the training pipeline's FeatureBuilder
    /// (numpy log differences), 2026-11-20.
    const SYNTHETIC_CLOSES: [f64; 30] = [
        99.0,
        100.16666666666667,
        99.16666666666667,
        100.33333333333334,
        99.33333333333333,
        100.49999999999999,
        99.5,
        100.66666666666666,
        99.66666666666667,
        100.83333333333333,
        99.83333333333333,
        101.0,
        100.0,
        99.0,
        100.16666666666667,
        99.16666666666667,
        100.33333333333334,
        99.33333333333333,
        100.49999999999999,
        99.5,
        100.66666666666666,
        99.66666666666667,
        100.83333333333333,
        99.83333333333333,
        101.0,
        100.0,
        99.0,
        100.16666666666667,
        99.16666666666667,
        100.33333333333334,
    ];
    /// numpy reference: [log_return_1, log_return_5, log_return_20].
    const EXPECTED_FEATURES: [f32; 3] = [
        0.011696039763191557,
        -0.006622540760493401,
        -0.004971012722020163,
    ];

    #[test]
    fn raw_log_returns_match_python_reference() {
        let features = raw_log_returns(&SYNTHETIC_CLOSES).expect("30 closes is enough");
        for (got, want) in features.iter().zip(EXPECTED_FEATURES) {
            assert!(
                (got - want).abs() < 1e-7,
                "feature drift: got {got}, want {want}"
            );
        }
        // Causality: identical tail history must give identical features even
        // when older bars differ.
        let mut older = vec![5000.0; 40];
        older.extend_from_slice(&SYNTHETIC_CLOSES);
        assert_eq!(raw_log_returns(&older), raw_log_returns(&SYNTHETIC_CLOSES));
        // Insufficient history yields no signal, not an error.
        assert!(raw_log_returns(&SYNTHETIC_CLOSES[..20]).is_none());
    }

    /// AC1 — fixed inputs must reproduce the Python onnxruntime fixture
    /// (models/regression_fixture.json) within its stated tolerance.
    #[test]
    fn fixture_regression_matches_python_onnxruntime() {
        let models = models_dir();
        let fixture_path = models.join("regression_fixture.json");
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&fixture_path).expect("committed fixture"),
        )
        .expect("fixture parses");
        let tolerance = fixture["tolerance"].as_f64().expect("tolerance") as f32;

        let inputs: Vec<[f32; 3]> = fixture["inputs"]
            .as_array()
            .expect("inputs array")
            .iter()
            .map(|row| {
                let mut out = [0f32; 3];
                for (slot, value) in out.iter_mut().zip(row.as_array().expect("row")) {
                    *slot = value.as_f64().expect("f64") as f32;
                }
                out
            })
            .collect();

        let model = OnnxMlpDirection::load(&models).expect("models load");
        for raw in MODEL_SYMBOLS {
            let symbol = Symbol::parse(raw).expect("static symbol parses");
            let expected = fixture["expected"][raw].as_object().expect("symbol entry");
            let expected_labels: Vec<i64> = expected["label"]
                .as_array()
                .expect("labels")
                .iter()
                .map(|v| v.as_i64().expect("i64"))
                .collect();
            let expected_probs: Vec<f32> = expected["probabilities"]
                .as_array()
                .expect("probabilities")
                .iter()
                .map(|row| row[1].as_f64().expect("f64") as f32) // P(up)
                .collect();

            for (i, features) in inputs.iter().enumerate() {
                let probability = model
                    .prob_up(&symbol, *features)
                    .expect("inference succeeds");
                assert!(
                    (probability - expected_probs[i]).abs() < tolerance,
                    "{raw} row {i}: got {probability}, want {}",
                    expected_probs[i]
                );
                let predicted_up = (probability >= 0.5) as i64;
                assert_eq!(
                    predicted_up, expected_labels[i],
                    "{raw} row {i}: label mismatch"
                );
            }
        }
    }
}
