"""MLP pipeline tests — leakage guards, versioning, inference schema."""

import math
from datetime import UTC, datetime, timedelta
from decimal import Decimal
from pathlib import Path

import numpy as np
import pytest

from lossfunction.backtest import Bar
from lossfunction.ml import FeatureBuilder, MLPPipeline, ModelSignal, next_bar_labels
from lossfunction.strategy import MarketSnapshot

T0 = datetime(2026, 9, 14, 0, 0, tzinfo=UTC)


def _bars(n: int = 120, seed: int = 3) -> list[Bar]:
    """Deterministic wave-plus-drift prices (learnable next-bar pattern)."""
    rng = np.random.default_rng(seed)
    bars: list[Bar] = []
    price = 80000.0
    for i in range(n):
        price += 120 * math.sin(i / 6.0) + 5 + rng.normal(0, 3)
        bars.append(
            Bar(
                symbol="005930",
                timestamp=T0 + timedelta(minutes=i),
                open=Decimal(str(round(price, 2))),
                high=Decimal(str(round(price, 2))),
                low=Decimal(str(round(price, 2))),
                close=Decimal(str(round(price, 2))),
            )
        )
    return bars


# ── AC1: no leakage in features and split ──────────────────────────


def test_features_are_causal() -> None:
    """Perturbing the last bar must not change any earlier feature row."""
    bars = _bars()
    builder = FeatureBuilder()
    _, X_before = builder.build(bars)

    perturbed = list(bars)
    perturbed[-1] = perturbed[-1].model_copy(update={"close": Decimal("123456.78")})
    _, X_after = builder.build(perturbed)

    assert np.array_equal(X_before[:-1], X_after[:-1])
    assert not np.array_equal(X_before[-1], X_after[-1])


def test_feature_row_uses_only_past_closes() -> None:
    bars = _bars(30)
    builder = FeatureBuilder(lookbacks=(1, 3))
    timestamps, X = builder.build(bars)
    start = 3  # max lookback
    assert timestamps[0] == bars[start].timestamp
    expected = math.log(float(bars[start].close)) - math.log(float(bars[start - 1].close))
    assert X[0, 0] == pytest.approx(expected)


def test_split_is_chronological_with_embargo() -> None:
    bars = _bars()
    _, report = MLPPipeline().train(bars)
    # Training features end strictly before test starts — and with at least
    # one full bar of embargo (the dropped label row).
    assert report.train_end < report.test_start
    bar_times = [bar.timestamp for bar in bars]
    train_end_idx = bar_times.index(report.train_end)
    test_start_idx = bar_times.index(report.test_start)
    assert test_start_idx - train_end_idx >= 2  # 1 embargo row between


def test_scaler_fitted_on_train_split_only() -> None:
    bars = _bars()
    builder = FeatureBuilder()
    X_all = builder.build(bars)[1]
    # Mirror the trainer's row accounting: the last feature row has no
    # next-bar label and is dropped before splitting.
    n = len(bars) - 1 - max(builder.lookbacks)
    X = X_all[:n]
    split = int(n * 0.75)
    train_only_mean = X[: split - 1].mean(axis=0)

    _, report = MLPPipeline(test_fraction=0.25).train(bars)
    assert np.allclose(report.scaler_mean, train_only_mean, rtol=1e-9)
    # Fitted on the train split only — NOT on the full dataset.
    assert not np.allclose(report.scaler_mean, X.mean(axis=0), rtol=1e-6)


def test_labels_are_next_bar_direction() -> None:
    bars = _bars(10)
    times, y = next_bar_labels(bars)
    assert len(y) == 9
    assert times[0] == bars[0].timestamp
    assert y[0] == (1 if float(bars[1].close) > float(bars[0].close) else 0)


# ── AC2: versioned save/load ───────────────────────────────────────


def test_bundle_roundtrip_with_metadata(tmp_path: Path) -> None:
    bars = _bars()
    bundle, report = MLPPipeline().train(bars)
    bundle.save(tmp_path)

    assert (tmp_path / "model.pkl").exists()
    assert (tmp_path / "metadata.json").exists()

    loaded_marker = (tmp_path / "model.pkl").exists() and (tmp_path / "metadata.json").exists()
    assert loaded_marker

    restored = type(bundle).load(tmp_path)
    assert restored.metadata["model_name"] == report.model_name
    assert restored.metadata["model_version"] == report.model_version
    assert restored.metadata["feature_names"] == list(report.feature_names)
    assert len(restored.metadata["dataset_sha256"]) == 64
    assert restored.metadata["sklearn_version"]

    # Same inputs through the restored model give the same signal.
    pipeline = MLPPipeline()
    fresh = pipeline.predict(bundle, bars, symbol="005930")
    from_disk = pipeline.predict(restored, bars, symbol="005930")
    assert fresh == from_disk


def test_training_is_deterministic() -> None:
    bars = _bars()
    _, first = MLPPipeline(random_state=7).train(bars)
    _, second = MLPPipeline(random_state=7).train(bars)
    assert first == second


# ── AC3: inference output feeds the decision layer schema ──────────


def test_inference_signal_fits_decision_layer_schema() -> None:
    bars = _bars()
    pipeline = MLPPipeline()
    bundle, _ = pipeline.train(bars)
    signal = pipeline.predict(bundle, bars, symbol="005930")

    assert isinstance(signal, ModelSignal)
    assert 0.0 <= signal.probability_up <= 1.0
    assert signal.symbol == "005930"
    assert set(signal.features) == {"log_return_1", "log_return_5", "log_return_20"}

    # The signal rides a MarketSnapshot without breaking its schema, and a
    # strategy reading snapshot.signals sees exactly this object.
    snapshot = MarketSnapshot(
        quotes={}, positions={}, as_of=signal.as_of, signals={"005930": signal}
    )
    assert snapshot.signals["005930"] == signal
