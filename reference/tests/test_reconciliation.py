"""Reconciliation and duplicate-order protection tests."""

from datetime import UTC, datetime
from decimal import Decimal

import httpx
import pytest

from lossfunction.broker.base import (
    Broker,
    ExecutionReport,
    OrderAck,
    OrderRequest,
    Position,
    Quote,
)
from lossfunction.broker.kis import KISAPIError
from lossfunction.broker.kis.rest import APIErrorKind
from lossfunction.domain.order import Order, OrderSide, OrderStatus, OrderType
from lossfunction.execution import (
    DuplicateOrderError,
    OrderGateway,
    OrderStateMachine,
    OrderSubmitTimeout,
    Reconciler,
)


def _order(client_order_id: str, **overrides: object) -> Order:
    defaults: dict[str, object] = {
        "client_order_id": client_order_id,
        "symbol": "005930",
        "side": OrderSide.BUY,
        "order_type": OrderType.LIMIT,
        "quantity": 10,
        "limit_price": Decimal("79000"),
    }
    defaults.update(overrides)
    return Order(**defaults)  # type: ignore[arg-type]


def _report(
    *,
    order_quantity: int = 10,
    filled_quantity: int = 10,
    open_: bool = False,
) -> ExecutionReport:
    return ExecutionReport(
        broker_order_id="B-1",
        client_order_id="c-1",
        symbol="005930",
        side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        order_quantity=order_quantity,
        filled_quantity=filled_quantity,
        average_fill_price=Decimal("79000") if filled_quantity else None,
        open=open_,
        timestamp=datetime.now(tz=UTC),
    )


class ScriptedBroker(Broker):
    """Broker with configurable submit outcome and preset reports."""

    def __init__(
        self,
        *,
        submit_error: Exception | None = None,
        reports: dict[str, ExecutionReport] | None = None,
        missing_orders: set[str] | None = None,
    ) -> None:
        self.submit_error = submit_error
        self.reports = reports or {}
        self.missing_orders = missing_orders or set()
        self.submitted: list[OrderRequest] = []

    async def submit_order(self, request: OrderRequest) -> OrderAck:
        self.submitted.append(request)
        if self.submit_error is not None:
            raise self.submit_error
        return OrderAck(client_order_id=request.client_order_id, broker_order_id="B-1")

    async def cancel_order(self, broker_order_id: str) -> None:
        raise NotImplementedError

    async def get_execution_report(self, broker_order_id: str) -> ExecutionReport:
        if broker_order_id in self.missing_orders:
            raise LookupError(broker_order_id)
        return self.reports[broker_order_id]

    async def get_positions(self) -> list[Position]:
        return []

    async def get_quote(self, symbol: str) -> Quote:
        raise NotImplementedError


def _gateway(broker: Broker) -> tuple[OrderGateway, OrderStateMachine]:
    machine = OrderStateMachine()
    return OrderGateway(broker, machine), machine


# ── AC3: duplicate submissions are blocked ─────────────────────────


async def test_duplicate_client_order_id_blocked() -> None:
    broker = ScriptedBroker()
    gateway, machine = _gateway(broker)

    await gateway.submit(_order("c-1"))
    with pytest.raises(DuplicateOrderError, match="duplicate order"):
        await gateway.submit(_order("c-1"))
    assert len(broker.submitted) == 1  # second attempt never reached the broker


async def test_distinct_orders_both_accepted() -> None:
    broker = ScriptedBroker()
    gateway, _ = _gateway(broker)
    await gateway.submit(_order("c-1"))
    await gateway.submit(_order("c-2"))
    assert len(broker.submitted) == 2


# ── AC1: timeout → UNKNOWN, no retry before reconciliation ─────────


async def test_submit_timeout_moves_to_unknown_and_blocks_retry() -> None:
    broker = ScriptedBroker(submit_error=httpx.ConnectError("simulated timeout"))
    gateway, machine = _gateway(broker)

    with pytest.raises(OrderSubmitTimeout):
        await gateway.submit(_order("c-1"))
    assert machine.status("c-1") is OrderStatus.UNKNOWN
    assert gateway.is_blocked_for_retry("c-1")

    # Retry of the same logical order is refused — reconciliation is the way out.
    with pytest.raises(DuplicateOrderError, match="duplicate order"):
        await gateway.submit(_order("c-1"))
    assert len(broker.submitted) == 1


async def test_definitive_rejection_moves_to_rejected() -> None:
    rejection = KISAPIError(
        APIErrorKind.API_REJECT, "KIS rejected order", rt_cd="1", msg_cd="40150"
    )
    broker = ScriptedBroker(submit_error=rejection)
    gateway, machine = _gateway(broker)

    with pytest.raises(KISAPIError):
        await gateway.submit(_order("c-1"))
    assert machine.status("c-1") is OrderStatus.REJECTED


# ── reconciliation of UNKNOWN orders ────────────────────────────────


async def test_reconcile_resolves_filled() -> None:
    broker = ScriptedBroker(reports={"B-1": _report(filled_quantity=10)})
    gateway, machine = _gateway(broker)
    broker.submit_error = httpx.ConnectError("timeout")

    with pytest.raises(OrderSubmitTimeout):
        await gateway.submit(_order("c-1"))
    status = await Reconciler(broker, machine).reconcile("c-1", "B-1")
    assert status is OrderStatus.FILLED
    assert machine.status("c-1") is OrderStatus.FILLED


async def test_reconcile_resolves_partial_and_cancelled() -> None:
    for report, expected in (
        (_report(filled_quantity=4, open_=True), OrderStatus.PARTIALLY_FILLED),
        (_report(filled_quantity=0, open_=True), OrderStatus.SUBMITTED),
        (_report(filled_quantity=4, open_=False), OrderStatus.CANCELLED),
    ):
        broker = ScriptedBroker(reports={"B-1": report})
        machine = OrderStateMachine()
        machine.register("c-1", OrderStatus.PENDING)
        machine.mark_unknown("c-1", reason="timeout")
        status = await Reconciler(broker, machine).reconcile("c-1", "B-1")
        assert status is expected


async def test_reconcile_order_missing_at_broker_is_rejected() -> None:
    broker = ScriptedBroker(missing_orders={"B-1"})
    machine = OrderStateMachine()
    machine.register("c-1", OrderStatus.PENDING)
    machine.mark_unknown("c-1", reason="timeout")

    status = await Reconciler(broker, machine).reconcile("c-1", "B-1")
    assert status is OrderStatus.REJECTED


async def test_reconcile_is_idempotent() -> None:
    broker = ScriptedBroker(reports={"B-1": _report()})
    machine = OrderStateMachine()
    machine.register("c-1", OrderStatus.PENDING)
    machine.mark_unknown("c-1", reason="timeout")
    reconciler = Reconciler(broker, machine)

    first = await reconciler.reconcile("c-1", "B-1")
    again = await reconciler.reconcile("c-1", "B-1")
    assert first is again is OrderStatus.FILLED
    assert len(machine.history("c-1")) == 2  # unknown + resolve, no duplicates


# ── AC2: restart-time open-order reconciliation ────────────────────


async def test_restart_reconciles_pending_orders() -> None:
    broker = ScriptedBroker(
        reports={
            "B-1": _report(filled_quantity=10),
            "B-2": _report(order_quantity=7, filled_quantity=0, open_=True),
        }
    )
    machine = OrderStateMachine()  # fresh process, empty machine
    reconciler = Reconciler(broker, machine)

    results = await reconciler.reconcile_pending({"c-1": "B-1", "c-2": "B-2"})
    assert results == {
        "c-1": OrderStatus.FILLED,
        "c-2": OrderStatus.SUBMITTED,
    }
    reasons = [e.reason for e in machine.history("c-1")]
    assert any("reconciled" in r for r in reasons)
