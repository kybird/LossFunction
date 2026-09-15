"""Portfolio aggregate — position aggregation over fills.

Exact arithmetic via Decimal; average price blends on buys and survives sells.
Realized P&L accrues on sells against the average price at sell time.
"""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict

from lossfunction.domain.errors import InsufficientPositionError
from lossfunction.domain.types import OrderSide, Price, Quantity, Symbol


class Fill(BaseModel):
    """An execution that has happened (broker-reported)."""

    model_config = ConfigDict(frozen=True)

    client_order_id: str
    symbol: Symbol
    side: OrderSide
    quantity: Quantity
    price: Price


class PositionState(BaseModel):
    """Mutable-by-replacement position snapshot inside the portfolio."""

    model_config = ConfigDict(frozen=True)

    symbol: Symbol
    quantity: Quantity
    average_price: Price
    realized_pnl: Price = Price(0)


class Portfolio(BaseModel):
    """Aggregate of positions; updated only through `apply_fill`."""

    model_config = ConfigDict(frozen=True)

    positions: dict[str, PositionState] = {}

    def position(self, symbol: str) -> PositionState | None:
        return self.positions.get(symbol)

    @property
    def total_realized_pnl(self) -> Price:
        return Price(sum((p.realized_pnl for p in self.positions.values()), start=Price(0)))

    def apply_fill(self, fill: Fill) -> Portfolio:
        """Return a new Portfolio with the fill applied."""
        current = self.positions.get(fill.symbol)
        if fill.side is OrderSide.BUY:
            if current is None:
                new_position = PositionState(
                    symbol=fill.symbol,
                    quantity=fill.quantity,
                    average_price=fill.price,
                )
            else:
                total_cost = current.average_price * current.quantity + fill.price * fill.quantity
                new_quantity = current.quantity + fill.quantity
                new_position = PositionState(
                    symbol=fill.symbol,
                    quantity=new_quantity,
                    average_price=Price(total_cost / new_quantity),
                    realized_pnl=current.realized_pnl,
                )
            return self._with(new_position)

        if current is None or current.quantity < fill.quantity:
            held = 0 if current is None else current.quantity
            msg = f"insufficient position for {fill.symbol}: sell {fill.quantity}, held {held}"
            raise InsufficientPositionError(msg)

        realized = (fill.price - current.average_price) * fill.quantity
        remaining = current.quantity - fill.quantity
        # Fully-closed positions stay in the map with quantity 0 (mirrors KIS
        # inquire-balance, which shows same-day full sells as 0-quantity rows
        # until D-2); realized P&L is preserved on the entry.
        new_position = PositionState(
            symbol=fill.symbol,
            quantity=remaining,
            average_price=current.average_price,
            realized_pnl=current.realized_pnl + realized,
        )
        return self._with(new_position)

    def _with(self, position: PositionState) -> Portfolio:
        updated = self.positions.copy()
        updated[position.symbol] = position
        return Portfolio(positions=updated)
