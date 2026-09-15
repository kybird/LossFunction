"""Domain model tests — order rules, portfolio aggregation, purity."""

from decimal import Decimal
from pathlib import Path

import pytest
from pydantic import ValidationError

from lossfunction.domain import (
    Fill,
    InsufficientPositionError,
    InvalidOrderError,
    Order,
    OrderSide,
    OrderStatus,
    OrderType,
    Portfolio,
)

DOMAIN_PACKAGE = Path(__file__).parent.parent / "src" / "lossfunction" / "domain"


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


# ── order creation rules ────────────────────────────────────────────


def test_order_requires_positive_quantity() -> None:
    with pytest.raises(ValidationError, match="quantity must be positive"):
        _order(quantity=0)


def test_limit_order_requires_positive_price() -> None:
    with pytest.raises(ValidationError, match="positive limit_price"):
        _order(order_type=OrderType.LIMIT)
    with pytest.raises(ValidationError, match="positive limit_price"):
        _order(order_type=OrderType.LIMIT, limit_price=Decimal("0"))


def test_market_order_rejects_limit_price() -> None:
    with pytest.raises(ValidationError, match="must not carry"):
        _order(limit_price=Decimal("80000"))


def test_filled_quantity_bounded_by_order_quantity() -> None:
    with pytest.raises(ValidationError, match="outside"):
        _order(filled_quantity=11)
    _order(filled_quantity=10)  # fully filled is in range


def test_symbol_pattern_enforced() -> None:
    with pytest.raises(ValidationError):
        _order(symbol="AAPL")


# ── amend / cancel rules ────────────────────────────────────────────


def test_amend_pending_order_returns_new_instance() -> None:
    order = _order(order_type=OrderType.LIMIT, limit_price=Decimal("79000"))
    amended = order.amend(quantity=20, limit_price=Decimal("78500"))
    assert amended.quantity == 20
    assert amended.limit_price == Decimal("78500")
    assert order.quantity == 10  # original untouched
    assert order is not amended


def test_amend_rejected_when_not_amendable() -> None:
    filled = _order(status=OrderStatus.FILLED)
    with pytest.raises(InvalidOrderError, match="cannot amend"):
        filled.amend(quantity=20)


def test_amend_cannot_shrink_below_filled_quantity() -> None:
    order = _order(status=OrderStatus.PARTIALLY_FILLED, filled_quantity=8)
    with pytest.raises(InvalidOrderError, match="below already-filled"):
        order.amend(quantity=5)


def test_cancel_allowed_while_open_and_after_partial_fill() -> None:
    assert _order().cancel().status is OrderStatus.CANCELLED
    partial = _order(status=OrderStatus.PARTIALLY_FILLED, filled_quantity=5)
    assert partial.cancel().status is OrderStatus.CANCELLED


def test_cancel_rejected_for_terminal_statuses() -> None:
    for status in (OrderStatus.FILLED, OrderStatus.CANCELLED, OrderStatus.REJECTED):
        with pytest.raises(InvalidOrderError, match="cancel"):
            _order(status=status).cancel()


# ── portfolio aggregation ───────────────────────────────────────────


def _fill(symbol: str, side: OrderSide, qty: int, price: str, client_order_id: str = "c-1") -> Fill:
    return Fill(
        client_order_id=client_order_id,
        symbol=symbol,
        side=side,
        quantity=qty,
        price=Decimal(price),
    )


def test_buy_creates_position_at_fill_price() -> None:
    portfolio = Portfolio().apply_fill(_fill("005930", OrderSide.BUY, 10, "80000"))
    position = portfolio.position("005930")
    assert position is not None
    assert position.quantity == 10
    assert position.average_price == Decimal("80000")


def test_average_price_blends_across_buys() -> None:
    portfolio = (
        Portfolio()
        .apply_fill(_fill("005930", OrderSide.BUY, 10, "80000"))
        .apply_fill(_fill("005930", OrderSide.BUY, 10, "82000"))
    )
    position = portfolio.position("005930")
    assert position is not None
    assert position.quantity == 20
    assert position.average_price == Decimal("81000")


def test_sell_keeps_average_price_and_books_realized_pnl() -> None:
    portfolio = (
        Portfolio()
        .apply_fill(_fill("005930", OrderSide.BUY, 10, "80000"))
        .apply_fill(_fill("005930", OrderSide.SELL, 4, "85000"))
    )
    position = portfolio.position("005930")
    assert position is not None
    assert position.quantity == 6
    assert position.average_price == Decimal("80000")
    assert position.realized_pnl == Decimal("20000")  # (85000-80000) * 4
    assert portfolio.total_realized_pnl == Decimal("20000")


def test_full_sell_leaves_zero_quantity_entry() -> None:
    portfolio = (
        Portfolio()
        .apply_fill(_fill("005930", OrderSide.BUY, 10, "80000"))
        .apply_fill(_fill("005930", OrderSide.SELL, 10, "90000"))
    )
    position = portfolio.position("005930")
    assert position is not None
    assert position.quantity == 0
    assert position.realized_pnl == Decimal("100000")


def test_oversell_rejected() -> None:
    portfolio = Portfolio().apply_fill(_fill("005930", OrderSide.BUY, 5, "80000"))
    with pytest.raises(InsufficientPositionError, match="insufficient position"):
        portfolio.apply_fill(_fill("005930", OrderSide.SELL, 6, "85000"))


def test_sell_without_position_rejected() -> None:
    with pytest.raises(InsufficientPositionError):
        Portfolio().apply_fill(_fill("005930", OrderSide.SELL, 1, "80000"))


def test_positions_are_independent() -> None:
    portfolio = (
        Portfolio()
        .apply_fill(_fill("005930", OrderSide.BUY, 10, "80000"))
        .apply_fill(_fill("035420", OrderSide.BUY, 3, "41000"))
    )
    samsung = portfolio.position("005930")
    naver = portfolio.position("035420")
    assert samsung is not None and naver is not None
    assert samsung.average_price == Decimal("80000")
    assert naver.average_price == Decimal("41000")


# ── purity ──────────────────────────────────────────────────────────


def test_domain_package_has_no_infrastructure_imports() -> None:
    """AC: the domain module runs without HTTP/DB/broker dependencies."""
    forbidden = ("httpx", "asyncpg", "websockets", "pydantic_settings")
    for path in DOMAIN_PACKAGE.rglob("*.py"):
        source = path.read_text(encoding="utf-8")
        for module in forbidden:
            assert module not in source, f"{path.name} references {module}"
