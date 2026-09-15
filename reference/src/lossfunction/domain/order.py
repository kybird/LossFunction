"""Order aggregate with creation/amendment/cancellation domain rules.

Pure domain: no I/O, no clocks, no infrastructure. The full status transition
table (timeout → UNKNOWN, reconciliation) lives in the execution layer; this
module enforces only order-shaped invariants and amend/cancel guards.
"""

from enum import StrEnum

from pydantic import BaseModel, ConfigDict, model_validator

from lossfunction.domain.errors import InvalidOrderError
from lossfunction.domain.types import OrderSide, OrderType, Price, Quantity, Symbol

# Partially-filled orders remain amendable: the amendment applies to the
# remaining quantity and must stay >= the already-filled part.
_AMENDABLE = frozenset({"pending", "submitted", "partially_filled"})


class OrderStatus(StrEnum):
    PENDING = "pending"
    SUBMITTED = "submitted"
    PARTIALLY_FILLED = "partially_filled"
    FILLED = "filled"
    CANCELLED = "cancelled"
    REJECTED = "rejected"
    UNKNOWN = "unknown"


class Order(BaseModel):
    """An order intent and its lifecycle data."""

    model_config = ConfigDict(frozen=True, validate_assignment=True)

    client_order_id: str
    symbol: Symbol
    side: OrderSide
    order_type: OrderType
    quantity: Quantity
    limit_price: Price | None = None
    status: OrderStatus = OrderStatus.PENDING
    filled_quantity: Quantity = 0

    @model_validator(mode="after")
    def _validate_shape(self) -> "Order":
        if self.quantity <= 0:
            msg = f"order quantity must be positive, got {self.quantity}"
            raise ValueError(msg)
        if self.order_type is OrderType.LIMIT:
            if self.limit_price is None or self.limit_price <= 0:
                msg = "limit orders require a positive limit_price"
                raise ValueError(msg)
        elif self.limit_price is not None:
            msg = "market orders must not carry a limit_price"
            raise ValueError(msg)
        if self.filled_quantity < 0 or self.filled_quantity > self.quantity:
            msg = f"filled_quantity {self.filled_quantity} outside [0, {self.quantity}]"
            raise ValueError(msg)
        return self

    def amend(
        self,
        *,
        quantity: int | None = None,
        limit_price: Price | None = None,
    ) -> "Order":
        """Return a new Order with amended terms (original stays untouched)."""
        self._require_amendable("amend")
        new_quantity = self.quantity if quantity is None else quantity
        if new_quantity < self.filled_quantity:
            msg = f"amended quantity {new_quantity} below already-filled {self.filled_quantity}"
            raise InvalidOrderError(msg)
        return self.model_copy(
            update={
                "quantity": new_quantity,
                "limit_price": limit_price if limit_price is not None else self.limit_price,
            }
        )

    def cancel(self) -> "Order":
        """Return a new Order marked cancelled (remainder after partial fills)."""
        if self.status in (OrderStatus.FILLED, OrderStatus.CANCELLED):
            msg = f"cannot cancel order in terminal status {self.status}"
            raise InvalidOrderError(msg)
        if self.status is OrderStatus.REJECTED:
            msg = "cannot cancel a rejected order"
            raise InvalidOrderError(msg)
        return self.model_copy(update={"status": OrderStatus.CANCELLED})

    def _require_amendable(self, action: str) -> None:
        if self.status not in _AMENDABLE:
            msg = f"cannot {action} order in status {self.status}"
            raise InvalidOrderError(msg)
