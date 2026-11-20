"""Train per-symbol MLP direction classifiers and export them to ONNX.

Split/feature procedure is copied from reference/src/lossfunction/ml/pipeline.py
(that file is frozen — copy, don't import or modify):

- Causal log-return features over the configured lookbacks: row at t uses
  bars up to and including t only.
- Binary next-bar direction labels.
- Chronological split with embargo: the last training row is dropped because
  its next-bar label peeks one bar into the test period.
- StandardScaler fitted on the training split only.

Feature sets (argv[2]):
- "price" (default): log_return over LOOKBACKS — the reference recipe.
- "ext": price returns + volume/volatility/candle features, all from the
  same daily OHLCV bars (causality preserved: bar t is completed data).

The scaler is folded into the exported ONNX graph via a sklearn Pipeline, so
the Rust inference adapter feeds RAW features. Determinism: random_state=7
everywhere, single fixed dataset.

Usage:
    python train_mlp.py [lookbacks-csv] [price|ext]    # e.g. 1,5,20,60,120 ext
    MLP_MODELS_DIR=/tmp/x python ...                   # redirect artifacts

Outputs (all git-tracked, outside data/):
- models/<symbol>.onnx            x3
- models/meta.json                feature order, split spans, seed, versions
- models/regression_fixture.json  fixed inputs -> onnxruntime expected outputs
"""

import hashlib
import itertools
import json
import os
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

# Universe: owner holdings (2026-11-20 decision — codes name-lookup + price-
# cross-checked; see raw 2026-11-20 Case 13). Override with MLP_SYMBOLS=csv.
# 017670=SK텔레콤(017660은 데이터 없음 — 웹검색 오류 정정), 미래에셋벤처투자는
# 코드-가격 불일치(-37%)로 오너 확인 대기 중 제외(raw 2026-11-20 Case 13).
HOLDING_SYMBOLS = (
    "005930", "032830", "000660", "161510", "017670", "034950", "100790",
    "010140", "006260", "034020", "489030", "0152E0", "051900",
    "441640", "298040",
)
# 제외: 459580(KODEX CD금리 — 라벨 841/841 상승, 방향 불확실성 없는 현금성 상품).
# 100790(미래에셋벤처투자)은 오너 확인으로 코드 확정 — 원 포트폴리오 표의 가격 21,000이
# 오기였고 실제 13,230~13,350(raw 2026-11-20 Case 13 정정).
SYMBOLS = tuple(s for s in os.environ.get("MLP_SYMBOLS", ",".join(HOLDING_SYMBOLS)).split(",") if s)
# Lookbacks may be overridden for experiments: `train_mlp.py 1,5,20,60,120`.
# Default is the promoted 5-lookback set (raw 2026-11-20 Case 10/13).
LOOKBACKS = tuple(int(v) for v in (sys.argv[1].split(",") if len(sys.argv) > 1 else (1, 5, 20, 60, 120)))
# Feature set: "price" (reference recipe) or "ext" (adds volume/vol/candle).
FEATURE_SET = sys.argv[2] if len(sys.argv) > 2 else "price"
# Experiments redirect artifacts without touching production models/.
MODELS_DIR = Path(os.environ.get("MLP_MODELS_DIR", ROOT / "models"))
TEST_FRACTION = 0.25
RANDOM_STATE = 7
# sklearn computes in float64, the ONNX graph in float32 — parity tolerance.
PARITY_TOLERANCE = 1e-5


def load_bars(db_path: Path, symbol: str) -> tuple[list[datetime], dict[str, np.ndarray]]:
    """Return (timestamps, columns) for the symbol's daily OHLCV bars."""
    con = sqlite3.connect(db_path)
    try:
        rows = con.execute(
            "SELECT ts, open, high, low, close, volume FROM candles"
            " WHERE symbol = ? AND timeframe = 'day' ORDER BY ts",
            (symbol,),
        ).fetchall()
    finally:
        con.close()
    if len(rows) <= max(LOOKBACKS) + 1:
        msg = f"{symbol}: need more than {max(LOOKBACKS) + 1} bars, got {len(rows)}"
        raise ValueError(msg)
    stamps = [datetime.fromisoformat(ts) for ts, *_ in rows]
    columns = {
        name: np.array([row[i] for row in rows], dtype=np.float64)
        for i, name in enumerate(("open", "high", "low", "close", "volume"), start=1)
    }
    return stamps, columns


def build_feature_matrix(columns: dict[str, np.ndarray]) -> tuple[list[str], np.ndarray]:
    """Causal feature rows — the price recipe is the reference FeatureBuilder.

    Every feature at row t uses bars at or before t only (completed bars).
    """
    closes = columns["close"]
    log_close = np.log(closes)
    start = max(LOOKBACKS)
    # feature_k(t) = log_close[t] - log_close[t-k], aligned at time t.
    matrices = [
        log_close[start:] - log_close[start - k : len(closes) - k] for k in LOOKBACKS
    ]
    names = [f"log_return_{k}" for k in LOOKBACKS]

    # Subset selection for isolation experiments: EXT_FEATURES=vol_surprise_20
    wanted = set(os.environ.get("EXT_FEATURES", "").split(",")) - {""}

    def selected(name: str) -> bool:
        return not wanted or name in wanted

    if FEATURE_SET == "ext":
        high, low, opens, vols = (
            columns["high"],
            columns["low"],
            columns["open"],
            columns["volume"],
        )
        one = np.concatenate([[0.0], np.diff(log_close)])  # per-bar log return

        def trailing_mean(i: int, series: np.ndarray, window: int) -> float:
            lo = max(0, i - window + 1)
            return float(series[lo : i + 1].mean())

        # Volume surprise: ln(vol_t / mean(vol, last 20 incl t)).
        vol_mean = np.array([trailing_mean(i, vols, 20) for i in range(len(vols))])
        if selected("vol_surprise_20"):
            matrices.append(np.log(vols[start:] / vol_mean[start:]))
            names.append("vol_surprise_20")
        # Realized volatility: rms of per-bar log returns, last 20 incl t.
        realized = np.array([trailing_mean(i, one**2, 20) ** 0.5 for i in range(len(one))])
        if selected("realized_vol_20"):
            matrices.append(realized[start:])
            names.append("realized_vol_20")
        # Candle geometry of bar t (completed): range, gap vs prior close,
        # close position within the bar's range.
        if selected("range_1"):
            matrices.append(np.log(high[start:] / low[start:]))
            names.append("range_1")
        gap = np.log(opens / np.roll(closes, 1))
        gap[0] = 0.0
        if selected("gap_1"):
            matrices.append(gap[start:])
            names.append("gap_1")
        position = (closes - low) / np.maximum(high - low, 1e-12)
        if selected("close_pos_1"):
            matrices.append(position[start:])
            names.append("close_pos_1")
    return names, np.column_stack(matrices)


def next_bar_labels(closes: np.ndarray) -> np.ndarray:
    """Binary next-bar direction — copied from reference next_bar_labels."""
    return np.array([1 if closes[i + 1] > closes[i] else 0 for i in range(len(closes) - 1)])


def dataset_digest(closes: np.ndarray) -> str:
    """Same digest recipe as reference MLPPipeline.train."""
    return hashlib.sha256(json.dumps([str(c) for c in closes]).encode()).hexdigest()


def align_features_labels(columns: dict[str, np.ndarray]) -> tuple[np.ndarray, np.ndarray]:
    """Pair each feature row with its same-timestamp next-bar label.

    Copied from reference train(): the last feature row has no next bar to
    predict — drop it, then row i of X pairs with y_all[start + i].
    """
    _, x_all = build_feature_matrix(columns)
    y_all = next_bar_labels(columns["close"])
    start = max(LOOKBACKS)
    n = len(columns["close"]) - 1 - start
    return x_all[:n], y_all[start : start + n]


def fixture_grid(names: list[str]) -> list[list[float]]:
    """Fixed RAW-feature grid for the regression fixture (all symbols share).

    Small feature sets use a systematic product grid; larger ones use a
    deterministic RNG grid with per-feature scale hints (realistic daily
    magnitudes, seed 7 — reproducible).
    """
    if len(names) <= 5:
        return [
            [round(v, 4) for v in combo]
            for combo in itertools.product((-0.05, 0.0, 0.05), repeat=len(names))
        ]
    rng = np.random.RandomState(RANDOM_STATE)
    rows = rng.normal(0.0, 0.03, size=(32, len(names)))
    scales = {
        "vol_surprise_20": (0.0, 0.3),
        "realized_vol_20": (0.015, 0.008),
        "range_1": (0.02, 0.01),
        "gap_1": (0.0, 0.01),
    }
    for column, name in enumerate(names):
        if name in scales:
            mu, sigma = scales[name]
            draws = rng.normal(mu, sigma, 32)
            rows[:, column] = np.abs(draws) if mu > 0 else draws
    if "close_pos_1" in names:
        rows[:, names.index("close_pos_1")] = rng.uniform(0.0, 1.0, 32)
    return [[round(float(v), 6) for v in row] for row in rows]


def split_indices(n: int) -> tuple[np.ndarray, np.ndarray]:
    """Chronological split with embargo — copied from reference train().

    Embargo: train_idx stops at split-1, dropping the last training row whose
    label peeks one bar into the test period.
    """
    split = int(n * (1 - TEST_FRACTION))
    train_idx = np.arange(0, split - 1)
    test_idx = np.arange(split, n)
    return train_idx, test_idx


def train_symbol(symbol: str, feature_names: list[str], fixture_inputs: list[list[float]]) -> dict:
    """Train, export, and verify one symbol; return its meta/verification record."""
    stamps, columns = load_bars(DB_PATH, symbol)
    x, y = align_features_labels(columns)
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
        initial_types=[
            ("input", skl2onnx.common.data_types.FloatTensorType([None, len(feature_names)]))
        ],
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

    fixture_probs = session.run(None, {"input": np.array(fixture_inputs, dtype=np.float32)})
    record = {
        "model": f"models/{symbol}.onnx",
        "onnx_sha256": hashlib.sha256(model_path.read_bytes()).hexdigest(),
        "n_bars": len(stamps),
        "data_start": stamps[0].isoformat(),
        "data_end": stamps[-1].isoformat(),
        "dataset_sha256": dataset_digest(columns["close"]),
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
    _, probe = load_bars(DB_PATH, SYMBOLS[0])
    feature_names, _ = build_feature_matrix(probe)
    fixture_inputs = fixture_grid(feature_names)
    records = {
        symbol: train_symbol(symbol, feature_names, fixture_inputs) for symbol in SYMBOLS
    }

    meta = {
        "generated_at": datetime.now().isoformat(),
        "task": "MLP per-symbol direction classifiers, sklearn -> ONNX",
        "feature_set": FEATURE_SET,
        "feature_names": feature_names,
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
            "source": "data/lossfunction.db candles (KIS daily backfill —"
            " scripts/dev/backfill-daily.ps1, rate-limit throttled since"
            " 2026-11-20)",
            "symbols": {
                s: {"n_bars": r["n_bars"], "data_end": r["data_end"]}
                for s, r in records.items()
            },
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
        "feature_names": feature_names,
        "inputs": fixture_inputs,
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

    print(
        json.dumps(
            {
                "status": "ok",
                "feature_set": FEATURE_SET,
                "features": len(feature_names),
                "models_dir": MODELS_DIR.as_posix(),
            }
        )
    )


if __name__ == "__main__":
    main()
