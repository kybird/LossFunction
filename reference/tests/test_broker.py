"""Broker abstraction conformance and MockBroker behavior tests."""

from datetime import UTC, datetime
from decimal import Decimal

import pytest

from lossfunction.broker import (
    Broker,
    ExecutionReport,
    MockBroker,
    OrderRequest,
)
from lossfunction.domain.types import OrderSide, OrderType


def _request(client_order_id: str, **overrides: object) -> OrderRequest:
    defaults: dict[str, object] = {
        "client_order_id": client_order_id,
        "symbol": "005930",
        "side": OrderSide.BUY,
        "order_type": OrderType.MARKET,
        "quantity": 10,
    }
    defaults.update(overrides)
    return OrderRequest(**defaults)  # type: ignore[arg-type]


def test_broker_is_abstract() -> None:
    with pytest.raises(TypeError):
        Broker()  # type: ignore[abstract]


def test_mock_broker_implements_interface() -> None:
    """Instantiation itself proves every abstract method is implemented."""
    broker = MockBroker(prices={"005930": "80000"})
    assert isinstance(broker, Broker)


async def test_market_order_fills_and_updates_position() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    ack = await broker.submit_order(_request("c-1"))
    report = await broker.get_execution_report(ack.broker_order_id)
    assert report.filled_quantity == 10
    assert report.average_fill_price == Decimal("80000")
    assert not report.open

    positions = await broker.get_positions()
    assert len(positions) == 1
    assert positions[0].symbol == "005930"
    assert positions[0].quantity == 10
    assert positions[0].average_price == Decimal("80000")


async def test_submit_is_idempotent_on_client_order_id() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    first = await broker.submit_order(_request("c-1"))
    again = await broker.submit_order(_request("c-1"))
    assert first == again

    positions = await broker.get_positions()
    assert positions[0].quantity == 10  # not 20 — no duplicate fill


async def test_average_price_blends_across_buys() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    await broker.submit_order(_request("c-1", quantity=10))
    broker.set_price("005930", "82000")
    await broker.submit_order(_request("c-2", quantity=10))

    positions = await broker.get_positions()
    assert positions[0].quantity == 20
    assert positions[0].average_price == Decimal("81000")


async def test_limit_order_stays_open_until_cancelled() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    ack = await broker.submit_order(
        _request("c-2", order_type=OrderType.LIMIT, limit_price=Decimal("79000"))
    )
    report = await broker.get_execution_report(ack.broker_order_id)
    assert report.open
    assert report.filled_quantity == 0

    await broker.cancel_order(ack.broker_order_id)
    report = await broker.get_execution_report(ack.broker_order_id)
    assert not report.open


async def test_cancel_rejects_closed_order() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    ack = await broker.submit_order(_request("c-1"))
    with pytest.raises(ValueError, match="not open"):
        await broker.cancel_order(ack.broker_order_id)


async def test_sell_requires_position() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    with pytest.raises(ValueError, match="insufficient position"):
        await broker.submit_order(_request("c-3", side=OrderSide.SELL, quantity=5))


async def test_sell_reduces_position_and_keeps_average_price() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    await broker.submit_order(_request("c-1", quantity=10))
    await broker.submit_order(_request("c-2", side=OrderSide.SELL, quantity=4))

    positions = await broker.get_positions()
    assert positions[0].quantity == 6
    assert positions[0].average_price == Decimal("80000")


async def test_quote_retrieval() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    quote = await broker.get_quote("005930")
    assert quote.last_price == Decimal("80000")
    assert quote.timestamp.tzinfo is not None


async def test_unknown_symbol_rejected() -> None:
    broker = MockBroker(prices={"005930": "80000"})
    with pytest.raises(KeyError):
        await broker.get_quote("999999")
    with pytest.raises(KeyError):
        await broker.submit_order(_request("c-4", symbol="999999"))


def test_execution_report_is_frozen() -> None:
    report = ExecutionReport(
        broker_order_id="MOCK-1",
        client_order_id="c-1",
        symbol="005930",
        side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        order_quantity=10,
        open=False,
        timestamp=datetime.now(tz=UTC),
    )
    with pytest.raises(Exception, match="Instance is frozen"):  # noqa: PT011
        report.open = True  # type: ignore[misc]


def test_symbol_pattern_is_enforced() -> None:
    with pytest.raises(ValueError):
        _request("c-5", symbol="AAPL")
