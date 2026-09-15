"""Order status state machine — the sole owner of order lifecycle transitions.

Design contract (docs/architecture.md §3 failure paths):
- A submission timeout or lost broker answer moves the order to UNKNOWN, never
  to FAILED — reconciliation against the broker is the only way out.
- Terminal states (FILLED/CANCELLED/REJECTED) accept no further transitions.
- Every successful transition emits exactly one `OrderTransitionEvent` for the
  audit trail; an illegal transition raises and emits nothing.
"""

from collections.abc import Callable
from datetime import UTC, datetime

from pydantic import BaseModel, ConfigDict

from lossfunction.domain.order import OrderStatus

# from-status -> statuses it may move to
TRANSITIONS: dict[OrderStatus, frozenset[OrderStatus]] = {
    OrderStatus.PENDING: frozenset(
        {OrderStatus.SUBMITTED, OrderStatus.REJECTED, OrderStatus.CANCELLED, OrderStatus.UNKNOWN}
    ),
    OrderStatus.SUBMITTED: frozenset(
        {
            OrderStatus.PARTIALLY_FILLED,
            OrderStatus.FILLED,
            OrderStatus.CANCELLED,
            OrderStatus.REJECTED,
            OrderStatus.UNKNOWN,
        }
    ),
    OrderStatus.PARTIALLY_FILLED: frozenset(
        {
            OrderStatus.PARTIALLY_FILLED,
            OrderStatus.FILLED,
            OrderStatus.CANCELLED,
            OrderStatus.UNKNOWN,
        }
    ),
    OrderStatus.UNKNOWN: frozenset(
        {
            OrderStatus.SUBMITTED,
            OrderStatus.PARTIALLY_FILLED,
            OrderStatus.FILLED,
            OrderStatus.CANCELLED,
            OrderStatus.REJECTED,
        }
    ),
    OrderStatus.FILLED: frozenset(),
    OrderStatus.CANCELLED: frozenset(),
    OrderStatus.REJECTED: frozenset(),
}

# UNKNOWN is reachable only while a broker answer is outstanding.
_UNKNOWN_ENTRY_ALLOWED_FROM = frozenset({OrderStatus.PENDING, OrderStatus.SUBMITTED})


class IllegalTransitionError(Exception):
    """A transition the state table does not permit."""


class OrderTransitionEvent(BaseModel):
    """Audit record for one executed order-status transition."""

    model_config = ConfigDict(frozen=True)

    client_order_id: str
    from_status: OrderStatus
    to_status: OrderStatus
    reason: str
    occurred_at: datetime


class OrderStateMachine:
    """Tracks per-order status and enforces the transition table."""

    def __init__(
        self,
        on_event: Callable[[OrderTransitionEvent], None] | None = None,
        *,
        clock: Callable[[], datetime] | None = None,
    ) -> None:
        self._on_event = on_event
        self._clock = clock or (lambda: datetime.now(tz=UTC))
        self._status: dict[str, OrderStatus] = {}
        self._history: dict[str, list[OrderTransitionEvent]] = {}

    @staticmethod
    def can_transition(from_status: OrderStatus, to_status: OrderStatus) -> bool:
        return to_status in TRANSITIONS[from_status]

    def status(self, client_order_id: str) -> OrderStatus | None:
        return self._status.get(client_order_id)

    def history(self, client_order_id: str) -> list[OrderTransitionEvent]:
        return list(self._history.get(client_order_id, []))

    def register(self, client_order_id: str, status: OrderStatus) -> None:
        """Introduce an order at a starting status (PENDING, or a restored
        status after restart recovery)."""
        if client_order_id in self._status:
            msg = f"order {client_order_id} already registered"
            raise IllegalTransitionError(msg)
        self._status[client_order_id] = status
        self._history.setdefault(client_order_id, [])

    def transition(
        self, client_order_id: str, to_status: OrderStatus, *, reason: str
    ) -> OrderStatus:
        """Move an order to `to_status`, emitting one audit event."""
        current = self._require_known(client_order_id)
        if not self.can_transition(current, to_status):
            msg = f"illegal transition for {client_order_id}: {current} -> {to_status}"
            raise IllegalTransitionError(msg)
        self._status[client_order_id] = to_status
        event = OrderTransitionEvent(
            client_order_id=client_order_id,
            from_status=current,
            to_status=to_status,
            reason=reason,
            occurred_at=self._clock(),
        )
        self._history.setdefault(client_order_id, []).append(event)
        if self._on_event is not None:
            self._on_event(event)
        return to_status

    def mark_unknown(self, client_order_id: str, *, reason: str) -> OrderStatus:
        """Timeout/lost-answer path: PENDING|SUBMITTED -> UNKNOWN."""
        current = self._require_known(client_order_id)
        if current not in _UNKNOWN_ENTRY_ALLOWED_FROM:
            msg = (
                f"UNKNOWN entry only allowed from an outstanding submission, "
                f"not from {current} (order {client_order_id})"
            )
            raise IllegalTransitionError(msg)
        return self.transition(client_order_id, OrderStatus.UNKNOWN, reason=reason)

    def _require_known(self, client_order_id: str) -> OrderStatus:
        status = self._status.get(client_order_id)
        if status is None:
            msg = f"unknown order {client_order_id}; register() it first"
            raise IllegalTransitionError(msg)
        return status
