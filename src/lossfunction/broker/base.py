"""Abstract broker interface.

`Broker` is the seam between the trading core (strategy/risk/order state) and
any venue implementation — KIS live, KIS paper (mock domain), or the in-memory
mock used by tests and the backtester. Implementations must be safe to call
from an async runtime; blocking implementations should offload to a thread.
"""

from abc import ABC, abstractmethod
from collections.abc import Callable
from datetime import datetime
from decimal import Decimal

from pydantic import BaseModel, ConfigDict, Field

from lossfunction.domain.types import OrderSide, OrderType, Quantity, Symbol


class Quote(BaseModel):
    """A point-in-time price snapshot for one symbol."""

    model_config = ConfigDict(frozen=True)

    symbol: Symbol
    last_price: Decimal = Field(gt=0)
    timestamp: datetime


class OrderRequest(BaseModel):
    """A pre-risk-checked order intent ready for submission.

    `client_order_id` is the idempotency key: brokers must treat resubmission
    of the same id as the same order, never a duplicate one.
    """

    model_config = ConfigDict(frozen=True)

    client_order_id: str
    symbol: Symbol
    side: OrderSide
    order_type: OrderType
    quantity: Quantity = Field(gt=0)
    limit_price: Decimal | None = Field(default=None, gt=0)


class OrderAck(BaseModel):
    """Broker acceptance of an order submission."""

    model_config = ConfigDict(frozen=True)

    client_order_id: str
    broker_order_id: str


class ExecutionReport(BaseModel):
    """Current execution state of an order as known by the broker.

    `filled_quantity`/`average_fill_price` are cumulative. An order with
    remaining quantity > 0 and open status is an open order that restart
    reconciliation must resolve.
    """

    model_config = ConfigDict(frozen=True)

    broker_order_id: str
    client_order_id: str
    symbol: Symbol
    side: OrderSide
    order_type: OrderType
    order_quantity: Quantity
    filled_quantity: Quantity = 0
    average_fill_price: Decimal | None = None
    open: bool = True
    timestamp: datetime


class Position(BaseModel):
    """A held quantity of one symbol with its average acquisition price."""

    model_config = ConfigDict(frozen=True)

    symbol: Symbol
    quantity: Quantity
    average_price: Decimal


QuoteCallback = Callable[[Quote], None]


class Broker(ABC):
    """Abstract venue interface for orders, balances, and quotes."""

    @abstractmethod
    async def submit_order(self, request: OrderRequest) -> OrderAck:
        """Submit an order; idempotent on `client_order_id`."""
        raise NotImplementedError

    @abstractmethod
    async def cancel_order(self, broker_order_id: str) -> None:
        """Request cancellation of an open order."""
        raise NotImplementedError

    @abstractmethod
    async def get_execution_report(self, broker_order_id: str) -> ExecutionReport:
        """Fetch the broker-side execution state of one order.

        The source of truth after timeouts and restarts — reconciliation
        adjusts local state to this, never the other way around.
        """
        raise NotImplementedError

    @abstractmethod
    async def get_positions(self) -> list[Position]:
        """List current positions held at the venue."""
        raise NotImplementedError

    @abstractmethod
    async def get_quote(self, symbol: Symbol) -> Quote:
        """Fetch a recent quote snapshot for one symbol."""
        raise NotImplementedError
