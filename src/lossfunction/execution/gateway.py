"""Order submission gateway — duplicate protection and timeout discipline.

The only path through which orders reach a broker:
- `submit` refuses any client_order_id it has already seen (idempotency) —
  including orders sitting in UNKNOWN after a timeout. Retrying a timed-out
  order means reconciling it, never resubmitting it.
- A definitive broker rejection (business reject, bad credentials) moves the
  order to REJECTED. Any other failure (timeout, network, server error) moves
  it to UNKNOWN — the broker may or may not have accepted it.
"""

from lossfunction.broker.base import Broker, OrderAck, OrderRequest
from lossfunction.broker.kis.rest import APIErrorKind, KISAPIError
from lossfunction.domain.order import Order, OrderStatus
from lossfunction.execution.state_machine import (
    IllegalTransitionError,
    OrderStateMachine,
)

_DEFINITIVE_REJECTION = frozenset({APIErrorKind.API_REJECT, APIErrorKind.INVALID_TOKEN})


class DuplicateOrderError(Exception):
    """A different submission with an already-used client_order_id."""


class OrderSubmitTimeout(Exception):
    """No definitive broker answer; the order now sits in UNKNOWN."""


class OrderGateway:
    """Submits orders through the state machine with idempotency."""

    def __init__(self, broker: Broker, machine: OrderStateMachine) -> None:
        self._broker = broker
        self._machine = machine
        self._broker_ids: dict[str, str] = {}  # client_order_id -> broker id

    def broker_order_id(self, client_order_id: str) -> str | None:
        return self._broker_ids.get(client_order_id)

    async def submit(self, order: Order) -> OrderAck:
        client_order_id = order.client_order_id
        if client_order_id in self._broker_ids or self._machine.status(client_order_id) is not None:
            msg = (
                f"duplicate order submission blocked: client_order_id "
                f"{client_order_id!r} already used"
            )
            raise DuplicateOrderError(msg)

        self._machine.register(client_order_id, OrderStatus.PENDING)
        request = OrderRequest(
            client_order_id=client_order_id,
            symbol=order.symbol,
            side=order.side,
            order_type=order.order_type,
            quantity=order.quantity,
            limit_price=order.limit_price,
        )
        try:
            ack = await self._broker.submit_order(request)
        except KISAPIError as exc:
            if exc.kind in _DEFINITIVE_REJECTION:
                self._machine.transition(
                    client_order_id,
                    OrderStatus.REJECTED,
                    reason=f"broker rejected: {exc.msg_cd or exc.kind}",
                )
                raise
            self._machine.mark_unknown(client_order_id, reason=f"no definitive answer: {exc}")
            raise OrderSubmitTimeout(str(exc)) from exc
        except Exception as exc:  # timeout / transport failure / server error
            self._machine.mark_unknown(client_order_id, reason=f"no definitive answer: {exc}")
            raise OrderSubmitTimeout(str(exc)) from exc

        self._machine.transition(
            client_order_id,
            OrderStatus.SUBMITTED,
            reason=f"broker ack {ack.broker_order_id}",
        )
        self._broker_ids[client_order_id] = ack.broker_order_id
        return ack

    def is_blocked_for_retry(self, client_order_id: str) -> bool:
        """True while an order is UNKNOWN and awaiting reconciliation."""
        return self._machine.status(client_order_id) is OrderStatus.UNKNOWN


class Reconciler:
    """Resolves UNKNOWN/open orders against broker-side truth."""

    def __init__(self, broker: Broker, machine: OrderStateMachine) -> None:
        self._broker = broker
        self._machine = machine

    async def reconcile(self, client_order_id: str, broker_order_id: str) -> OrderStatus:
        """Resolve one order from UNKNOWN using the broker's report."""
        status = self._machine.status(client_order_id)
        if status is None:
            msg = f"unknown order {client_order_id}; register it first"
            raise IllegalTransitionError(msg)
        if status is not OrderStatus.UNKNOWN:
            return status  # already resolved; nothing to do

        try:
            report = await self._broker.get_execution_report(broker_order_id)
        except LookupError:
            # Broker has no record: the order never reached it.
            return self._machine.transition(
                client_order_id,
                OrderStatus.REJECTED,
                reason="not found at broker during reconciliation",
            )

        target = self._target_status(report.filled_quantity, report.order_quantity, report.open)
        return self._machine.transition(
            client_order_id,
            target,
            reason=(
                f"reconciled: filled {report.filled_quantity}/"
                f"{report.order_quantity}, open={report.open}"
            ),
        )

    async def reconcile_pending(self, pending: dict[str, str]) -> dict[str, OrderStatus]:
        """Restart recovery: reconcile client->broker id pairs left open.

        Orders are registered directly at UNKNOWN (their submission answer
        was outstanding when the process died) and then resolved.
        """
        results: dict[str, OrderStatus] = {}
        for client_order_id, broker_order_id in pending.items():
            if self._machine.status(client_order_id) is None:
                self._machine.register(client_order_id, OrderStatus.UNKNOWN)
            results[client_order_id] = await self.reconcile(client_order_id, broker_order_id)
        return results

    @staticmethod
    def _target_status(filled_quantity: int, order_quantity: int, open_: bool) -> OrderStatus:
        if not open_ and filled_quantity >= order_quantity:
            return OrderStatus.FILLED
        if open_:
            return OrderStatus.PARTIALLY_FILLED if filled_quantity > 0 else OrderStatus.SUBMITTED
        return OrderStatus.CANCELLED  # closed with unfilled remainder
