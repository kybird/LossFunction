"""Backtesting engine tests — costs, look-ahead guard, reproducibility."""

from datetime import UTC, datetime, timedelta
from decimal import Decimal

import pytest

from lossfunction.backtest import (
    BacktestConfig,
    BacktestEngine,
    BacktestFeed,
    Bar,
    LookaheadError,
)
from lossfunction.strategy import MarketSnapshot, Strategy

T0 = datetime(2026, 9, 14, 0, 0, tzinfo=UTC)


def _bars(closes: list[str], symbol: str = "005930") -> list[Bar]:
    return [
        Bar(
            symbol=symbol,
            timestamp=T0 + timedelta(minutes=i),
            open=Decimal(c),
            high=Decimal(c),
            low=Decimal(c),
            close=Decimal(c),
        )
        for i, c in enumerate(closes)
    ]


class BuyOnceStrategy(Strategy):
    """Buys 10 shares on the first snapshot, sells 10 on the next."""

    name = "buy-once"
    version = "1"

    def __init__(self) -> None:
        self.calls = 0

    def decide(self, snapshot: MarketSnapshot):
        from lossfunction.domain.order import OrderSide, OrderType
        from lossfunction.strategy import OrderIntent, StrategyDecision

        if self.calls == 0:
            intent = OrderIntent(
                symbol="005930", side=OrderSide.BUY, order_type=OrderType.MARKET, quantity=10
            )
        elif self.calls == 1:
            intent = OrderIntent(
                symbol="005930", side=OrderSide.SELL, order_type=OrderType.MARKET, quantity=10
            )
        else:
            intent = None
        self.calls += 1
        return StrategyDecision(
            strategy_name=self.name,
            strategy_version=self.version,
            intents=(intent,) if intent else (),
            features={"calls": str(self.calls)},
        )


class SnapshotSpyStrategy(Strategy):
    """Records every snapshot it sees (for look-ahead assertions)."""

    name = "spy"
    version = "1"

    def __init__(self) -> None:
        self.seen: list[MarketSnapshot] = []
        inner = BuyOnceStrategy()
        self._inner = inner

    def decide(self, snapshot: MarketSnapshot):
        from lossfunction.strategy import StrategyDecision

        self.seen.append(snapshot)
        decision = self._inner.decide(snapshot)
        return StrategyDecision(
            strategy_name=self.name,
            strategy_version=self.version,
            intents=decision.intents,
            features=decision.features,
        )


# ── AC1: costs enter the P&L ───────────────────────────────────────


async def test_costs_reflected_in_equity() -> None:
    config = BacktestConfig(
        initial_cash=Decimal("1000000"),
        commission_rate=Decimal("0.001"),  # 0.1% for exact math
        tax_rate=Decimal("0.01"),  # 1% sell tax for exact math
    )
    result = await BacktestEngine(BuyOnceStrategy(), config).run(_bars(["80000", "90000"]))

    buy_notional = Decimal(10) * Decimal("80000")  # fills at bar close
    sell_notional = Decimal(10) * Decimal("90000")
    expected_commission = buy_notional * Decimal("0.001") + sell_notional * Decimal("0.001")
    expected_tax = sell_notional * Decimal("0.01")

    assert result.total_commission == expected_commission
    assert result.total_tax == expected_tax
    assert result.cash == (
        Decimal("1000000")
        - buy_notional
        - buy_notional * Decimal("0.001")
        + sell_notional
        - sell_notional * Decimal("0.001")
        - expected_tax
    )
    assert result.equity == result.cash  # flat at the end
    assert result.realized_pnl == (Decimal("90000") - Decimal("80000")) * 10


async def test_unfilled_limit_orders_cost_nothing() -> None:
    from lossfunction.domain.order import OrderSide, OrderType
    from lossfunction.strategy import OrderIntent, StrategyDecision

    class RestingLimitStrategy(Strategy):
        name = "resting"
        version = "1"

        def decide(self, snapshot: MarketSnapshot) -> StrategyDecision:
            return StrategyDecision(
                strategy_name=self.name,
                strategy_version=self.version,
                intents=(
                    OrderIntent(
                        symbol="005930",
                        side=OrderSide.BUY,
                        order_type=OrderType.LIMIT,
                        quantity=10,
                        limit_price="1000",
                    ),  # never crossed
                ),
                features={},
            )

    result = await BacktestEngine(RestingLimitStrategy()).run(_bars(["80000", "80000"]))
    assert result.fills == ()
    assert result.total_commission == Decimal("0")
    assert result.cash == result.config.initial_cash


# ── AC2: look-ahead is blocked ─────────────────────────────────────


def test_feed_rejects_future_bar_access() -> None:
    bars = _bars(["1", "2", "3"])
    feed = BacktestFeed(bars)
    feed.next()  # cursor = 1
    with pytest.raises(LookaheadError):
        feed.bar_at(1)
    with pytest.raises(LookaheadError):
        feed.bar_at(2)
    assert feed.bar_at(0).close == Decimal("1")


def test_feed_rejects_unsorted_bars() -> None:
    bars = _bars(["1", "2"])
    shuffled = [bars[1], bars[0]]
    with pytest.raises(ValueError, match="pre-sorted"):
        BacktestFeed(shuffled)


async def test_strategy_never_sees_future_bars() -> None:
    bars = _bars(["10", "20", "30"])
    spy = SnapshotSpyStrategy()
    await BacktestEngine(spy).run(bars)

    assert len(spy.seen) == 3
    for i, snapshot in enumerate(spy.seen):
        expected_time = bars[i].timestamp
        for quote in snapshot.quotes.values():
            assert quote.timestamp <= expected_time
    # Decision 3 must not contain the 4th... there is none; the final snapshot
    # contains exactly one quote at the current bar's time.
    last_quotes = spy.seen[-1].quotes
    assert set(last_quotes) == {"005930"}
    assert last_quotes["005930"].last_price == Decimal("30")


# ── AC3: reproducibility ───────────────────────────────────────────


async def test_same_inputs_reproduce_identical_results() -> None:
    bars = _bars(["10", "5", "20", "15"])
    first = await BacktestEngine(BuyOnceStrategy()).run(list(bars))
    second = await BacktestEngine(BuyOnceStrategy()).run(list(bars))
    assert first == second
    assert first.equity_curve == second.equity_curve
    assert first.fills == second.fills
