"""Train per-symbol MLP direction classifiers and export them to ONNX.

Split/feature procedure is copied from reference/src/lossfunction/ml/pipeline.py
(that file is frozen — copy, don't import or modify):

- Causal log-return features over lookbacks (1, 5, 20): row at t uses bars up
  to and including t only.
- Binary next-bar direction labels.
- Chronological split with embargo: the last training row is dropped because
  its next-bar label peeks one bar into the test period.
- StandardScaler fitted on the training split only.

The scaler is folded into the exported ONNX graph via a sklearn Pipeline, so
the Rust inference adapter (next card) feeds RAW features. Determinism:
random_state=7 everywhere, single fixed dataset.

Outputs (all git-tracked, outside data/):
- models/<symbol>.onnx            x3
- models/meta.json                feature order, split spans, seed, versions
- models/regression_fixture.json  fixed inputs -> onnxruntime expected outputs
"""

import hashlib
import json
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

import numpy as np
import onnxruntime as ort
import sklearn
import skl2onnx
from scipy.stats import binomtest
from sklearn.neural_network import MLPClassifier
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import StandardScaler
from skl2onnx import to_onnx

ROOT = Path(__file__).resolve().parents[2]
DB_PATH = ROOT / "data" / "lossfunction.db"
MODELS_DIR = ROOT / "models"

SYMBOLS = ("005930", "035420", "069500")
LOOKBACKS = (1, 5, 20)
TEST_FRACTION = 0.25
RANDOM_STATE = 7
# sklearn computes in float64, the ONNX graph in float32 — parity tolerance.
PARITY_TOLERANCE = 1e-5

# Fixed raw-feature grid for the regression fixture (shared by all symbols).
# Daily log-returns rarely exceed |0.08|; this grid spans the realistic range.
FIXTURE_INPUTS = [
    [round(a, 4), round(b, 4), round(c, 4)]
    for a in (-0.05, 0.0, 0.05)
    for b in (-0.05, 0.0, 0.05)
    for c in (-0.05, 0.0, 0.05)
]


def load_closes(db_path: Path, symbol: str) -> tuple[list[datetime], list[float]]:
    """Return (timestamps, closes) for the symbol's daily bars, chronological."""
    con = sqlite3.connect(db_path)
    try:
        rows = con.execute(
            "SELECT ts, close FROM candles WHERE symbol = ? AND timeframe = 'day'"
            " ORDER BY ts",
            (symbol,),
        ).fetchall()
    finally:
        con.close()
    if len(rows) <= max(LOOKBACKS) + 1:
        msg = f"{symbol}: need more than {max(LOOKBACKS) + 1} bars, got {len(rows)}"
        raise ValueError(msg)
    stamps = [datetime.fromisoformat(ts) for ts, _ in rows]
    closes = [float(close) for _, close in rows]
    return stamps, closes


def build_features(closes: list[float]) -> np.ndarray:
    """Causal log-return rows — copied from reference FeatureBuilder.build."""
    log_close = np.log(np.array(closes))
    start = max(LOOKBACKS)
    return np.column_stack(
        [log_close[start:] - log_close[start - k : len(closes) - k] for k in LOOKBACKS]
    )


def next_bar_labels(closes: list[float]) -> np.ndarray:
    """Binary next-bar direction — copied from reference next_bar_labels."""
    return np.array([1 if closes[i + 1] > closes[i] else 0 for i in range(len(closes) - 1)])


def dataset_digest(closes: list[float]) -> str:
    """Same digest recipe as reference MLPPipeline.train."""
    return hashlib.sha256(json.dumps([str(c) for c in closes]).encode()).hexdigest()


def align_features_labels(closes: list[float]) -> tuple[np.ndarray, np.ndarray]:
    """Pair each feature row with its same-timestamp next-bar label.

    Copied from reference train(): the last feature row has no next bar to
    predict — drop it, then row i of X pairs with y_all[start + i].
    """
    x_all = build_features(closes)
    y_all = next_bar_labels(closes)
    start = max(LOOKBACKS)
    n = len(closes) - 1 - start
    return x_all[:n], y_all[start : start + n]


def split_indices(n: int) -> tuple[np.ndarray, np.ndarray]:
    """Chronological split with embargo — copied from reference train().

    Embargo: train_idx stops at split-1, dropping the last training row whose
    label peeks one bar into the test period.
    """
    split = int(n * (1 - TEST_FRACTION))
    train_idx = np.arange(0, split - 1)
    test_idx = np.arange(split, n)
    return train_idx, test_idx


def train_symbol(symbol: str) -> dict:
    """Train, export, and verify one symbol; return its meta/verification record."""
    stamps, closes = load_closes(DB_PATH, symbol)
    x, y = align_features_labels(closes)
    feat_times = stamps[max(LOOKBACKS) : max(LOOKBACKS) + len(y)]
    train_idx, test_idx = split_indices(len(y))

    # Pipeline == reference math exactly: scaler fits on train rows only, then
    # the MLP fits on the scaled train rows. Folding scaler+MLP into one
    # Pipeline is what gets StandardScaler INTO the ONNX graph.
    pipe = Pipeline(
        [
            ("scaler", StandardScaler()),
            (
                "mlp",
                MLPClassifier(
                    hidden_layer_sizes=(16,),
                    max_iter=300,
                    random_state=RANDOM_STATE,
                ),
            ),
        ]
    )
    pipe.fit(x[train_idx], y[train_idx])
    accuracy = float(pipe.score(x[test_idx], y[test_idx]))

    # Coin-flip significance: exact binomial test on the held-out segment.
    n_test = int(len(test_idx))
    k_correct = int(round(accuracy * n_test))
    p_value = float(binomtest(k_correct, n_test, 0.5).pvalue)

    onnx_model = to_onnx(
        pipe,
        initial_types=[("input", skl2onnx.common.data_types.FloatTensorType([None, 3]))],
        options={id(pipe): {"zipmap": False}},
        target_opset=None,
    )
    model_path = MODELS_DIR / f"{symbol}.onnx"
    model_path.write_bytes(onnx_model.SerializeToString())

    # ── Verification: onnxruntime load + sklearn parity ────────────────────
    session = ort.InferenceSession(model_path.as_posix(), providers=["CPUExecutionProvider"])
    ort_outs = session.run(None, {"input": x[test_idx].astype(np.float32)})
    labels_out, probs_out = ort_outs[0], ort_outs[1]
    sklearn_probs = pipe.predict_proba(x[test_idx])
    parity_diff = float(np.max(np.abs(probs_out.astype(np.float64) - sklearn_probs)))
    if parity_diff >= PARITY_TOLERANCE:
        msg = f"{symbol}: onnxruntime/sklearn parity diff {parity_diff} >= {PARITY_TOLERANCE}"
        raise AssertionError(msg)
    sklearn_labels = pipe.predict(x[test_idx])
    if not np.array_equal(labels_out, sklearn_labels):
        msg = f"{symbol}: onnxruntime label output differs from sklearn predict"
        raise AssertionError(msg)

    fixture_probs = session.run(None, {"input": np.array(FIXTURE_INPUTS, dtype=np.float32)})
    record = {
        "model": f"models/{symbol}.onnx",
        "onnx_sha256": hashlib.sha256(model_path.read_bytes()).hexdigest(),
        "n_bars": len(closes),
        "data_start": stamps[0].isoformat(),
        "data_end": stamps[-1].isoformat(),
        "dataset_sha256": dataset_digest(closes),
        "n_train": int(len(train_idx)),
        "n_test": n_test,
        "train_end": feat_times[int(train_idx[-1])].isoformat(),
        "test_start": feat_times[int(test_idx[0])].isoformat(),
        "test_accuracy": accuracy,
        "binom_p_two_sided_vs_coin_flip": p_value,
        "parity_max_abs_diff_onnxruntime_vs_sklearn": parity_diff,
        "fixture_labels": [int(v) for v in fixture_probs[0]],
        "fixture_probabilities": [
            [float(p) for p in row] for row in fixture_probs[1].astype(np.float64)
        ],
    }
    return record


def main() -> None:
    MODELS_DIR.mkdir(parents=True, exist_ok=True)
    records = {symbol: train_symbol(symbol) for symbol in SYMBOLS}

    meta = {
        "generated_at": datetime.now().isoformat(),
        "task": "MLP per-symbol direction classifiers, sklearn -> ONNX",
        "feature_names": [f"log_return_{k}" for k in LOOKBACKS],
        "lookbacks": list(LOOKBACKS),
        "label": "next-bar direction (1 = up)",
        "model": "Pipeline(StandardScaler, MLPClassifier(hidden_layer_sizes=(16,),"
        " max_iter=300, random_state=7))",
        "random_state": RANDOM_STATE,
        "test_fraction": TEST_FRACTION,
        "split_procedure": (
            "reference/src/lossfunction/ml/pipeline.py — chronological split,"
            " embargo drops last train row (label peeks into test)"
        ),
        "scaler_note": "StandardScaler fitted on train split only and included"
        " in the ONNX graph — feed RAW features",
        "parity_tolerance": PARITY_TOLERANCE,
        "versions": {
            "python": sys.version.split()[0],
            "scikit-learn": sklearn.__version__,
            "skl2onnx": skl2onnx.__version__,
            "onnxruntime": ort.__version__,
            "numpy": np.__version__,
        },
        "data": {
            "source": "data/lossfunction.db (owner dev-machine copy — daily bars"
            " stop at 2026-09-21; ~2-month gap to 2026-11-20 not backfilled,"
            " vault locked so backfill-daily.ps1 was not runnable)",
            "symbols": {s: {"n_bars": r["n_bars"], "data_end": r["data_end"]} for s, r in records.items()},
        },
        "symbols": records,
    }
    (MODELS_DIR / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False), encoding="utf-8"
    )

    fixture = {
        "description": "Regression fixture for the Rust ort inference adapter:"
        " fixed RAW feature inputs, expected outputs from Python onnxruntime"
        " (float32 session, CPUExecutionProvider).",
        "input_name": "input",
        "output_names": ["label", "probabilities"],
        "feature_names": [f"log_return_{k}" for k in LOOKBACKS],
        "inputs": FIXTURE_INPUTS,
        "tolerance": PARITY_TOLERANCE,
        "expected": {
            s: {
                "label": r["fixture_labels"],
                "probabilities": r["fixture_probabilities"],
            }
            for s, r in records.items()
        },
    }
    (MODELS_DIR / "regression_fixture.json").write_text(
        json.dumps(fixture, indent=2, ensure_ascii=False), encoding="utf-8"
    )

    print(json.dumps({"status": "ok", "symbols": SYMBOLS, "models_dir": MODELS_DIR.as_posix()}))


if __name__ == "__main__":
    main()
