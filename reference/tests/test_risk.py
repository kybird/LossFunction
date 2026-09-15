"""Risk layer tests — limits, stale data, kill switch."""

from datetime import UTC, datetime, timedelta
from decimal import Decimal

import pytest

from lossfunction.broker.base import Quote
from lossfunction.domain.order import Order, OrderSide, OrderType
from lossfunction.domain.portfolio import Fill, Portfolio
from lossfunction.risk import OrderRejected, RiskLimits, RiskManager, RiskRejectionReason

NOW = datetime(2026, 9, 14, 0, 30, tzinfo=UTC)

LIMITS = RiskLimits(
    max_order_notional=Decimal("10000000"),  # 1천만원
    max_position_quantity=100,
    max_gross_exposure=Decimal("30000000"),  # 3천만원
    daily_loss_limit=Decimal("500000"),  # 50만원
)


def _clock(now: datetime = NOW):
    return lambda: now


def _order(**overrides: object) -> Order:
    defaults: dict[str, object] = {
        "client_order_id": "c-1",
        "symbol": "005930",
        "side": OrderSide.BUY,
        "order_type": OrderType.MARKET,
        "quantity": 10,
    }
    defaults.update(overrides)
    return Order(**defaults)  # type: ignore[arg-type]


def _quotes(now: datetime = NOW, price: str = "80000") -> dict[str, Quote]:
    return {"005930": Quote(symbol="005930", last_price=Decimal(price), timestamp=now)}


def _manager(**limits_overrides: object) -> RiskManager:
    limits = LIMITS.model_copy(update=limits_overrides)  # type: ignore[arg-type]
    return RiskManager(limits, clock=_clock())


def test_within_limits_order_passes_and_returns_notional() -> None:
    notional = _manager().check_order(_order(), Portfolio(), _quotes())
    assert notional == Decimal("800000")  # 10 * 80000


# ── AC1: limit breaches block orders ───────────────────────────────


def test_order_notional_limit_blocks() -> None:
    manager = _manager(max_order_notional=Decimal("500000"))
    with pytest.raises(OrderRejected) as excinfo:
        manager.check_order(_order(), Portfolio(), _quotes())
    assert excinfo.value.reason is RiskRejectionReason.ORDER_NOTIONAL_EXCEEDED


def test_position_quantity_limit_blocks_buys() -> None:
    portfolio = Portfolio().apply_fill(
        Fill(
            client_order_id="x",
            symbol="005930",
            side=OrderSide.BUY,
            quantity=95,
            price=Decimal("80000"),
        )
    )
    manager = _manager()
    with pytest.raises(OrderRejected) as excinfo:
        manager.check_order(_order(quantity=10), portfolio, _quotes())
    assert excinfo.value.reason is RiskRejectionReason.POSITION_QUANTITY_EXCEEDED
    # Selling never trips the position cap.
    manager.check_order(_order(side=OrderSide.SELL, quantity=10), portfolio, _quotes())


def test_gross_exposure_limit_blocks() -> None:
    # Two symbols maxed under the per-symbol cap, priced so gross already
    # exceeds the portfolio-wide limit; any further buy must be refused.
    quotes = {
        symbol: Quote(symbol=symbol, last_price=Decimal("200000"), timestamp=NOW)
        for symbol in ("005930", "035420", "069500")
    }
    portfolio = (
        Portfolio()
        .apply_fill(
            Fill(
                client_order_id="x",
                symbol="005930",
                side=OrderSide.BUY,
                quantity=100,
                price=Decimal("200000"),
            )
        )
        .apply_fill(
            Fill(
                client_order_id="y",
                symbol="035420",
                side=OrderSide.BUY,
                quantity=100,
                price=Decimal("200000"),
            )
        )
    )
    manager = _manager()
    with pytest.raises(OrderRejected) as excinfo:
        manager.check_order(_order(symbol="069500", quantity=10), portfolio, quotes)
    assert excinfo.value.reason is RiskRejectionReason.GROSS_EXPOSURE_EXCEEDED


def test_daily_loss_limit_blocks_all_orders() -> None:
    portfolio = (
        Portfolio()
        .apply_fill(
            Fill(
                client_order_id="x",
                symbol="005930",
                side=OrderSide.BUY,
                quantity=100,
                price=Decimal("80000"),
            )
        )
        .apply_fill(
            Fill(
                client_order_id="y",
                symbol="005930",
                side=OrderSide.SELL,
                quantity=100,
                price=Decimal("74000"),
            )
        )  # -600000 realized
    )
    manager = _manager()
    for side in (OrderSide.BUY, OrderSide.SELL):
        with pytest.raises(OrderRejected) as excinfo:
            manager.check_order(_order(side=side, quantity=1), portfolio, _quotes())
        assert excinfo.value.reason is RiskRejectionReason.DAILY_LOSS_LIMIT_EXCEEDED


# ── AC2: stale market data blocks orders ───────────────────────────


def test_stale_quote_blocks_orders() -> None:
    old_quote_time = NOW - timedelta(seconds=30)
    manager = _manager()
    with pytest.raises(OrderRejected) as excinfo:
        manager.check_order(_order(), Portfolio(), _quotes(now=old_quote_time))
    assert excinfo.value.reason is RiskRejectionReason.STALE_MARKET_DATA


def test_fresh_quote_passes() -> None:
    fresh = NOW - timedelta(seconds=5)
    _manager().check_order(_order(), Portfolio(), _quotes(now=fresh))


def test_missing_quote_blocks_orders() -> None:
    with pytest.raises(OrderRejected) as excinfo:
        _manager().check_order(_order(), Portfolio(), {})
    assert excinfo.value.reason is RiskRejectionReason.NO_QUOTE


def test_limit_order_still_requires_fresh_quote() -> None:
    old = NOW - timedelta(seconds=60)
    with pytest.raises(OrderRejected, match="stale"):
        _manager().check_order(
            _order(order_type=OrderType.LIMIT, limit_price=Decimal("79000")),
            Portfolio(),
            _quotes(now=old),
        )


# ── AC3: kill switch blocks every path ─────────────────────────────


def test_kill_switch_blocks_every_order_regardless_of_state() -> None:
    manager = _manager()
    manager.activate_kill_switch("manual halt")
    assert manager.kill_switch_active
    assert manager.kill_reason == "manual halt"

    variants = [
        _order(),  # plain buy
        _order(side=OrderSide.SELL, quantity=3),
        _order(order_type=OrderType.LIMIT, limit_price=Decimal("79000")),
    ]
    portfolio = Portfolio().apply_fill(
        Fill(
            client_order_id="x",
            symbol="005930",
            side=OrderSide.BUY,
            quantity=50,
            price=Decimal("80000"),
        )
    )
    for order in variants:
        with pytest.raises(OrderRejected) as excinfo:
            manager.check_order(order, portfolio, _quotes())
        assert excinfo.value.reason is RiskRejectionReason.KILL_SWITCH

    # Kill switch precedes every other check — even without quotes.
    with pytest.raises(OrderRejected) as excinfo:
        manager.check_order(_order(), portfolio, {})
    assert excinfo.value.reason is RiskRejectionReason.KILL_SWITCH


def test_kill_switch_release_resumes_orders() -> None:
    manager = _manager()
    manager.activate_kill_switch("halt")
    manager.deactivate_kill_switch()
    assert not manager.kill_switch_active
    manager.check_order(_order(), Portfolio(), _quotes())
