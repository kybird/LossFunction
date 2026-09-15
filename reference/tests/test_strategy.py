"""Strategy layer tests — determinism, recording, example strategy."""

from datetime import UTC, datetime
from decimal import Decimal

import pytest

from lossfunction.broker.base import Quote
from lossfunction.domain.order import OrderSide
from lossfunction.domain.portfolio import PositionState
from lossfunction.strategy import (
    DecisionLayer,
    EntryPriceStrategy,
    MarketSnapshot,
    Strategy,
    StrategyDecision,
)

NOW = datetime(2026, 9, 14, 0, 30, tzinfo=UTC)


def _snapshot(**overrides: object) -> MarketSnapshot:
    defaults: dict[str, object] = {
        "quotes": {
            "005930": Quote(symbol="005930", last_price=Decimal("79000"), timestamp=NOW),
            "035420": Quote(symbol="035420", last_price=Decimal("42000"), timestamp=NOW),
        },
        "positions": {},
        "as_of": "session-1",
    }
    defaults.update(overrides)
    return MarketSnapshot(**defaults)  # type: ignore[arg-type]


def test_example_strategy_buys_at_entry() -> None:
    strategy = EntryPriceStrategy({"005930": "80000", "035420": "41000"}, quantity=5)
    decision = strategy.decide(_snapshot())

    assert decision.strategy_name == "entry-price"
    assert decision.strategy_version == "1"
    assert [(i.symbol, i.side, i.quantity, i.limit_price) for i in decision.intents] == [
        ("005930", OrderSide.BUY, 5, "79000"),  # 79000 <= 80000 entry
    ]
    # 035420 at 42000 > 41000 entry → no signal, but its features are recorded.
    assert decision.features["035420:price"] == "42000"
    assert decision.features["035420:entry"] == "41000"


def test_example_strategy_skips_held_symbols() -> None:
    strategy = EntryPriceStrategy({"005930": "80000"})
    snapshot = _snapshot(
        positions={
            "005930": PositionState(symbol="005930", quantity=3, average_price=Decimal("78000"))
        },
    )
    decision = strategy.decide(snapshot)
    assert decision.intents == ()
    assert decision.features["005930:held"] == "3"


def test_example_strategy_handles_missing_quote() -> None:
    strategy = EntryPriceStrategy({"999999": "1000"})
    decision = strategy.decide(_snapshot())
    assert decision.intents == ()
    assert decision.features["999999"] == "no-quote"


# ── AC1: same input → same signal ──────────────────────────────────


def test_same_snapshot_yields_identical_decisions() -> None:
    strategy = EntryPriceStrategy({"005930": "80000"})
    first = strategy.decide(_snapshot())
    second = strategy.decide(_snapshot())
    assert first == second  # full structural equality, not just intents


def test_decision_layer_repeats_identically() -> None:
    layer = DecisionLayer(EntryPriceStrategy({"005930": "80000"}))
    assert layer.decide(_snapshot()) == layer.decide(_snapshot())


# ── AC2: every decision is recorded with features ──────────────────


def test_decision_layer_records_input_features_and_signals() -> None:
    recorded: list[tuple[StrategyDecision, MarketSnapshot]] = []
    layer = DecisionLayer(
        EntryPriceStrategy({"005930": "80000", "035420": "41000"}),
        on_decision=lambda d, s: recorded.append((d, s)),
    )
    decision = layer.decide(_snapshot())

    assert len(recorded) == 1
    recorded_decision, recorded_snapshot = recorded[0]
    assert recorded_decision == decision
    assert recorded_snapshot == _snapshot()
    # The decision itself carries the features the signals derived from.
    assert decision.features["005930:price"] == "79000"
    assert decision.features["005930:entry"] == "80000"
    assert layer.decisions() == [decision]


# ── interface conformance ──────────────────────────────────────────


def test_strategy_is_abstract() -> None:
    with pytest.raises(TypeError):
        Strategy()  # type: ignore[abstract]


def test_decision_is_frozen() -> None:
    decision = EntryPriceStrategy({"005930": "80000"}).decide(_snapshot())
    with pytest.raises(Exception, match="frozen"):
        decision.rationale = "tampered"  # type: ignore[misc]


def test_snapshot_is_frozen() -> None:
    snapshot = _snapshot()
    with pytest.raises(Exception, match="frozen"):
        snapshot.as_of = "tampered"  # type: ignore[misc]
