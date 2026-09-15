"""Runtime orchestrator end-to-end and restart-recovery tests (MockBroker)."""

from datetime import UTC, datetime, timedelta
from decimal import Decimal

from lossfunction.broker.mock import MockBroker
from lossfunction.domain.order import OrderStatus
from lossfunction.risk import RiskLimits, RiskManager
from lossfunction.runtime import TradingRuntime
from lossfunction.strategy import EntryPriceStrategy

NOW = datetime(2026, 9, 14, 0, 30, tzinfo=UTC)

LIMITS = RiskLimits(
    max_order_notional=Decimal("10000000"),
    max_position_quantity=100,
    max_gross_exposure=Decimal("30000000"),
    daily_loss_limit=Decimal("500000"),
    stale_quote_max_age=timedelta(seconds=60),
)


def _runtime(broker: MockBroker, clock=lambda: NOW) -> TradingRuntime:
    return TradingRuntime(
        broker=broker,
        strategy=EntryPriceStrategy({"005930": "80000"}, quantity=10),
        risk=RiskManager(LIMITS, clock=clock),
    )


def _quote(broker_price: str, at: datetime = NOW):
    from lossfunction.broker.base import Quote

    return Quote(symbol="005930", last_price=Decimal(broker_price), timestamp=at)


# ── AC1: end-to-end event flow on a mock broker ────────────────────


async def test_end_to_end_flow() -> None:
    broker = MockBroker(prices={"005930": "79000"})
    runtime = _runtime(broker)

    runtime.on_quote(_quote("79000"))
    submitted = await runtime.run_decision_cycle()
    assert len(submitted) == 1
    assert submitted[0].client_order_id == "ord-0001"

    statuses = await runtime.sync_all()
    assert statuses == {"ord-0001": OrderStatus.FILLED}
    position = runtime.portfolio.position("005930")
    assert position is not None
    assert position.quantity == 10
    assert position.average_price == Decimal("79000")

    # Second cycle: position held → no new signal.
    runtime.on_quote(_quote("78500"))
    assert await runtime.run_decision_cycle() == []


async def test_risk_rejection_stops_submission() -> None:
    broker = MockBroker(prices={"005930": "79000"})
    runtime = TradingRuntime(
        broker=broker,
        strategy=EntryPriceStrategy({"005930": "80000"}, quantity=10),
        risk=RiskManager(
            LIMITS.model_copy(update={"max_order_notional": Decimal("100")}),
            clock=lambda: NOW,
        ),
    )
    runtime.on_quote(_quote("79000"))
    assert await runtime.run_decision_cycle() == []
    assert runtime.status_of("ord-0001") is None  # never submitted
    assert broker.get_positions.__self__ is not None


async def test_stale_quote_blocks_cycle() -> None:
    broker = MockBroker(prices={"005930": "79000"})
    runtime = _runtime(broker)
    runtime.on_quote(_quote("79000", at=NOW - timedelta(seconds=120)))
    assert await runtime.run_decision_cycle() == []


async def test_kill_switch_blocks_everything() -> None:
    broker = MockBroker(prices={"005930": "79000"})
    runtime = _runtime(broker)
    runtime._risk.activate_kill_switch("test halt")
    runtime.on_quote(_quote("79000"))
    assert await runtime.run_decision_cycle() == []


async def test_client_order_ids_are_unique_and_sequential() -> None:
    broker = MockBroker(prices={"005930": "79000"})
    runtime = TradingRuntime(
        broker=broker,
        strategy=EntryPriceStrategy({"005930": "80000"}, quantity=1),
        risk=RiskManager(LIMITS, clock=lambda: NOW),
    )
    runtime.on_quote(_quote("79000"))
    first = await runtime.run_decision_cycle()
    await runtime.sync_all()
    runtime.on_quote(_quote("78500"))
    # Flatten the position by hand, then a fresh signal can fire again.
    await runtime.rebuild_portfolio()
    second = await runtime.run_decision_cycle()
    ids = [o.client_order_id for o in first + second]
    assert len(ids) == len(set(ids))


# ── AC2: restart recovery ──────────────────────────────────────────


async def test_restart_recovery_reconciles_and_rebuilds() -> None:
    # Process 1: submit and fill an order.
    broker = MockBroker(prices={"005930": "79000"})
    runtime1 = _runtime(broker)
    runtime1.on_quote(_quote("79000"))
    await runtime1.run_decision_cycle()
    pending = runtime1.open_local_orders()  # what storage would persist

    # Process 2: fresh runtime, same broker (world kept, state lost).
    runtime2 = _runtime(broker)
    assert runtime2.portfolio.positions == {}

    results = await runtime2.recover(pending)
    assert results == {"ord-0001": OrderStatus.FILLED}
    position = runtime2.portfolio.position("005930")
    assert position is not None
    assert position.quantity == 10
    assert position.average_price == Decimal("79000")


async def test_restart_recovery_with_open_limit_order() -> None:
    broker = MockBroker(prices={"005930": "79000"})
    runtime1 = TradingRuntime(
        broker=broker,
        strategy=EntryPriceStrategy({"005930": "80000"}, quantity=10),
        risk=RiskManager(LIMITS, clock=lambda: NOW),
    )
    runtime1.on_quote(_quote("79000"))

    # Hand-craft an open (unfilled) limit order at the broker.
    from lossfunction.broker.base import OrderRequest
    from lossfunction.domain.order import OrderSide, OrderType

    ack = await broker.submit_order(
        OrderRequest(
            client_order_id="ord-9001",
            symbol="005930",
            side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=4,
            limit_price=Decimal("70000"),
        )
    )

    runtime2 = _runtime(broker)
    results = await runtime2.recover({"ord-9001": ack.broker_order_id})
    assert results == {"ord-9001": OrderStatus.SUBMITTED}
    # Still-open orders remain in the local open set for later syncing.
    assert runtime2.open_local_orders() == {"ord-9001": ack.broker_order_id}


async def test_restart_recovery_blocked_from_resubmitting() -> None:
    """The recovered order id must not be reusable by a new cycle."""
    broker = MockBroker(prices={"005930": "79000"})
    runtime = _runtime(broker)
    await runtime.recover({"ord-0001": "MOCK-1"})  # resolves to FILLED

    # Fresh cycle must use a fresh id, never ord-0001 again.
    runtime.on_quote(_quote("79000"))
    submitted = await runtime.run_decision_cycle()
    assert all(o.client_order_id != "ord-0001" for o in submitted)
