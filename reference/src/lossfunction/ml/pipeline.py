"""MLP feature/training/inference pipeline with time-series discipline.

Leakage controls, each enforced structurally:
- Features are strictly causal: the row at time t is computed from bars at
  or before t only.
- The train/test split is chronological and **embargoed**: the last training
  row is dropped because its next-bar label looks one bar into the test
  period.
- Normalization statistics are fitted on the training split only and travel
  with the saved bundle.
"""

import hashlib
import json
import pickle
from datetime import datetime
from pathlib import Path
from typing import Any

import numpy as np
from pydantic import BaseModel, ConfigDict, Field
from sklearn.neural_network import MLPClassifier
from sklearn.preprocessing import StandardScaler

from lossfunction.backtest.engine import Bar
from lossfunction.ml.schema import ModelSignal


class FeatureBuilder:
    """Causal log-return features over fixed lookbacks."""

    def __init__(self, lookbacks: tuple[int, ...] = (1, 5, 20)) -> None:
        if not lookbacks or any(k < 1 for k in lookbacks):
            msg = f"lookbacks must be positive ints, got {lookbacks}"
            raise ValueError(msg)
        self.lookbacks = tuple(sorted(lookbacks))

    @property
    def feature_names(self) -> list[str]:
        return [f"log_return_{k}" for k in self.lookbacks]

    def build(self, bars: list[Bar]) -> tuple[list[datetime], np.ndarray]:
        """Return (timestamps, X) where X[i] uses bars up to timestamps[i]."""
        if len(bars) <= max(self.lookbacks):
            msg = f"need more than {max(self.lookbacks)} bars, got {len(bars)}"
            raise ValueError(msg)
        closes = np.array([float(bar.close) for bar in bars])
        log_close = np.log(closes)
        start = max(self.lookbacks)
        # feature_k(t) = log_close[t] - log_close[t-k], aligned at time t.
        rows = np.column_stack(
            [log_close[start:] - log_close[start - k : len(bars) - k] for k in self.lookbacks]
        )
        timestamps = [bar.timestamp for bar in bars[start:]]
        return timestamps, rows


def next_bar_labels(bars: list[Bar]) -> tuple[list[datetime], np.ndarray]:
    """Binary next-bar direction labels; label time = the bar it predicts."""
    closes = [float(bar.close) for bar in bars]
    labels = np.array([1 if closes[i + 1] > closes[i] else 0 for i in range(len(closes) - 1)])
    timestamps = [bars[i].timestamp for i in range(len(closes) - 1)]
    return timestamps, labels


class TrainReport(BaseModel):
    model_config = ConfigDict(frozen=True)

    model_name: str
    model_version: str
    feature_names: tuple[str, ...]
    n_train: int
    n_test: int
    train_end: datetime  # latest bar time among training FEATURES
    test_start: datetime
    test_accuracy: float
    scaler_mean: tuple[float, ...]


class ModelBundle(BaseModel):
    model_config = ConfigDict(frozen=True)

    metadata: dict[str, Any]
    model: Any = Field(exclude=True, repr=False)
    scaler: Any = Field(exclude=True, repr=False)

    def save(self, directory: Path) -> None:
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "model.pkl").write_bytes(
            pickle.dumps({"model": self.model, "scaler": self.scaler})
        )
        (directory / "metadata.json").write_text(
            json.dumps(self.metadata, indent=2, default=str), encoding="utf-8"
        )

    @classmethod
    def load(cls, directory: Path) -> "ModelBundle":
        payload = pickle.loads((directory / "model.pkl").read_bytes())  # noqa: S301
        metadata = json.loads((directory / "metadata.json").read_text(encoding="utf-8"))
        return cls(metadata=metadata, model=payload["model"], scaler=payload["scaler"])


class MLPPipeline:
    """Train, persist, and run a causal-feature MLP direction classifier."""

    def __init__(
        self,
        *,
        model_name: str = "mlp-direction",
        model_version: str = "1",
        test_fraction: float = 0.25,
        random_state: int = 7,
    ) -> None:
        if not 0.0 < test_fraction < 1.0:
            msg = f"test_fraction must be in (0, 1), got {test_fraction}"
            raise ValueError(msg)
        self.model_name = model_name
        self.model_version = model_version
        self.test_fraction = test_fraction
        self.random_state = random_state

    def train(
        self, bars: list[Bar], feature_builder: FeatureBuilder | None = None
    ) -> tuple[ModelBundle, TrainReport]:
        builder = feature_builder or FeatureBuilder()
        timestamps, X_all = builder.build(bars)
        label_times, y_all = next_bar_labels(bars)

        start = max(builder.lookbacks)
        # The last feature row has no next bar to predict — drop it, then
        # row i of X pairs with label y_all[start + i] (same timestamp).
        n = len(bars) - 1 - start
        feat_times = timestamps[:n]
        X = X_all[:n]
        y = y_all[start : start + n]
        if list(label_times[start : start + n]) != list(feat_times):
            msg = "feature/label misalignment"
            raise ValueError(msg)

        split = int(n * (1 - self.test_fraction))
        # Embargo: drop the last training row — its label peeks into test.
        train_idx = np.arange(0, split - 1)
        test_idx = np.arange(split, n)

        scaler = StandardScaler().fit(X[train_idx])
        X_train = scaler.transform(X[train_idx])
        X_test = scaler.transform(X[test_idx])

        model = MLPClassifier(
            hidden_layer_sizes=(16,),
            max_iter=300,
            random_state=self.random_state,
        )
        model.fit(X_train, y[train_idx])
        accuracy = float(model.score(X_test, y[test_idx]))

        digest = hashlib.sha256(json.dumps([str(bar.close) for bar in bars]).encode()).hexdigest()

        report = TrainReport(
            model_name=self.model_name,
            model_version=self.model_version,
            feature_names=tuple(builder.feature_names),
            n_train=len(train_idx),
            n_test=len(test_idx),
            train_end=feat_times[int(train_idx[-1])],
            test_start=feat_times[int(test_idx[0])],
            test_accuracy=accuracy,
            scaler_mean=tuple(float(v) for v in scaler.mean_),
        )
        metadata = {
            **json.loads(report.model_dump_json()),
            "dataset_sha256": digest,
            "sklearn_version": __import__("sklearn").__version__,
            "trained_at": datetime.now().isoformat(),
        }
        return ModelBundle(metadata=metadata, model=model, scaler=scaler), report

    def predict(
        self,
        bundle: ModelBundle,
        bars: list[Bar],
        *,
        symbol: str,
        feature_builder: FeatureBuilder | None = None,
    ) -> ModelSignal:
        builder = feature_builder or FeatureBuilder()
        timestamps, X = builder.build(bars)
        scaled = bundle.scaler.transform(X[-1:])
        probability = float(bundle.model.predict_proba(scaled)[0, 1])
        return ModelSignal(
            model_name=bundle.metadata["model_name"],
            model_version=bundle.metadata["model_version"],
            symbol=symbol,
            as_of=timestamps[-1].isoformat(),
            probability_up=probability,
            features={
                name: str(value) for name, value in zip(builder.feature_names, X[-1], strict=True)
            },
        )
