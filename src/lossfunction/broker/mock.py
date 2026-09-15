"""In-memory broker for tests and the backtester.

Fills market orders immediately at the configured quote price and keeps limit
orders open. Deterministic: same script of calls produces the same state.
"""

from collections.abc import Callable
from datetime import UTC, datetime
from decimal import Decimal

from lossfunction.broker.base import (
    Broker,
    ExecutionReport,
    OrderAck,
    OrderRequest,
    Position,
    Quote,
)
from lossfunction.domain.types import OrderSide, OrderType, Quantity, Symbol

QuoteCallback = Callable[[Quote], None]


class MockBroker(Broker):
    """Deterministic in-memory `Broker` implementation."""

    def __init__(self, prices: dict[str, str] | None = None) -> None:
        self._quotes: dict[str, Decimal] = {
            symbol: Decimal(price) for symbol, price in (prices or {}).items()
        }
        self._next_broker_id = 1
        self._acks: dict[str, OrderAck] = {}  # client_order_id -> ack
        self._reports: dict[str, ExecutionReport] = {}  # broker_order_id -> report
        self._positions: dict[str, tuple[Quantity, Decimal]] = {}

    def set_price(self, symbol: str, price: str | Decimal) -> None:
        """Set the quote price used for subsequent fills."""
        self._quotes[symbol] = Decimal(str(price))

    async def submit_order(self, request: OrderRequest) -> OrderAck:
        existing = self._acks.get(request.client_order_id)
        if existing is not None:
            return existing

        price = self._quotes.get(request.symbol)
        if price is None:
            msg = f"no quote configured for symbol {request.symbol}"
            raise KeyError(msg)

        ack = OrderAck(
            client_order_id=request.client_order_id,
            broker_order_id=f"MOCK-{self._next_broker_id}",
        )
        self._next_broker_id += 1

        if request.order_type is OrderType.MARKET:
            report = ExecutionReport(
                broker_order_id=ack.broker_order_id,
                client_order_id=request.client_order_id,
                symbol=request.symbol,
                side=request.side,
                order_type=request.order_type,
                order_quantity=request.quantity,
                filled_quantity=request.quantity,
                average_fill_price=price,
                open=False,
                timestamp=self._now(),
            )
            self._apply_fill(request.symbol, request.side, request.quantity, price)
        else:
            report = ExecutionReport(
                broker_order_id=ack.broker_order_id,
                client_order_id=request.client_order_id,
                symbol=request.symbol,
                side=request.side,
                order_type=request.order_type,
                order_quantity=request.quantity,
                open=True,
                timestamp=self._now(),
            )

        self._acks[request.client_order_id] = ack
        self._reports[ack.broker_order_id] = report
        return ack

    async def cancel_order(self, broker_order_id: str) -> None:
        report = self._reports[broker_order_id]
        if not report.open:
            msg = f"order {broker_order_id} is not open"
            raise ValueError(msg)
        self._reports[broker_order_id] = report.model_copy(
            update={"open": False, "timestamp": self._now()},
        )

    async def get_execution_report(self, broker_order_id: str) -> ExecutionReport:
        return self._reports[broker_order_id]

    async def get_positions(self) -> list[Position]:
        return [
            Position(symbol=symbol, quantity=qty, average_price=avg_price)
            for symbol, (qty, avg_price) in sorted(self._positions.items())
        ]

    async def get_quote(self, symbol: Symbol) -> Quote:
        price = self._quotes.get(symbol)
        if price is None:
            msg = f"no quote configured for symbol {symbol}"
            raise KeyError(msg)
        return Quote(symbol=symbol, last_price=price, timestamp=self._now())

    def _apply_fill(self, symbol: str, side: OrderSide, quantity: Quantity, price: Decimal) -> None:
        current = self._positions.get(symbol)
        if side is OrderSide.BUY:
            if current is None:
                self._positions[symbol] = (quantity, price)
                return
            qty, avg_price = current
            total_cost = avg_price * qty + price * quantity
            new_qty = qty + quantity
            self._positions[symbol] = (new_qty, total_cost / new_qty)
        else:
            if current is None or current[0] < quantity:
                msg = f"insufficient position for {symbol} to sell"
                raise ValueError(msg)
            qty, avg_price = current
            remaining = qty - quantity
            if remaining == 0:
                del self._positions[symbol]
            else:
                self._positions[symbol] = (remaining, avg_price)

    @staticmethod
    def _now() -> datetime:
        return datetime.now(tz=UTC)
