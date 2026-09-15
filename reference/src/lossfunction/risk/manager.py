"""Pre-trade risk checks.

Every order passes `RiskManager.check_order` before the gateway submits it.
Checks are pure functions of (order, portfolio, quotes, limits, kill state) —
deterministic and testable without infrastructure.
"""

from datetime import UTC, datetime, timedelta
from decimal import Decimal
from enum import StrEnum
from typing import Any

from pydantic import BaseModel, ConfigDict, Field

from lossfunction.broker.base import Quote
from lossfunction.domain.order import Order, OrderSide
from lossfunction.domain.portfolio import Portfolio


class RiskRejectionReason(StrEnum):
    KILL_SWITCH = "kill_switch"
    STALE_MARKET_DATA = "stale_market_data"
    NO_QUOTE = "no_quote"
    ORDER_NOTIONAL_EXCEEDED = "order_notional_exceeded"
    POSITION_QUANTITY_EXCEEDED = "position_quantity_exceeded"
    GROSS_EXPOSURE_EXCEEDED = "gross_exposure_exceeded"
    DAILY_LOSS_LIMIT_EXCEEDED = "daily_loss_limit_exceeded"


class OrderRejected(Exception):
    """An order blocked by the risk layer; never retried as-is."""

    def __init__(self, reason: RiskRejectionReason, detail: str) -> None:
        super().__init__(f"{reason.value}: {detail}")
        self.reason = reason
        self.detail = detail


class RiskLimits(BaseModel):
    """Numeric guardrails (KRW quantities/prices)."""

    model_config = ConfigDict(frozen=True)

    max_order_notional: Decimal = Field(gt=0)
    max_position_quantity: int = Field(gt=0)
    max_gross_exposure: Decimal = Field(gt=0)
    daily_loss_limit: Decimal = Field(gt=0)
    stale_quote_max_age: timedelta = Field(default=timedelta(seconds=10))


class RiskManager:
    """Stateful only for the kill switch; all else derives from inputs."""

    def __init__(
        self,
        limits: RiskLimits,
        *,
        clock: Any = None,
    ) -> None:
        self._limits = limits
        self._clock = clock or (lambda: datetime.now(tz=UTC))
        self._kill_active = False
        self._kill_reason: str | None = None

    @property
    def kill_switch_active(self) -> bool:
        return self._kill_active

    @property
    def kill_reason(self) -> str | None:
        return self._kill_reason

    def activate_kill_switch(self, reason: str) -> None:
        self._kill_active = True
        self._kill_reason = reason

    def deactivate_kill_switch(self) -> None:
        self._kill_active = False
        self._kill_reason = None

    def check_order(
        self,
        order: Order,
        portfolio: Portfolio,
        quotes: dict[str, Quote],
    ) -> Decimal:
        """Validate one order; returns the order notional or raises.

        Raises `OrderRejected` with a machine-readable reason on any breach.
        """
        if self._kill_active:
            raise OrderRejected(
                RiskRejectionReason.KILL_SWITCH,
                f"kill switch active: {self._kill_reason}",
            )

        quote = quotes.get(order.symbol)
        if quote is None:
            raise OrderRejected(
                RiskRejectionReason.NO_QUOTE,
                f"no quote available for {order.symbol}",
            )
        age = self._clock() - quote.timestamp
        if age > self._limits.stale_quote_max_age:
            raise OrderRejected(
                RiskRejectionReason.STALE_MARKET_DATA,
                f"quote for {order.symbol} is {age.total_seconds():.0f}s old "
                f"(max {self._limits.stale_quote_max_age.total_seconds():.0f}s)",
            )

        price = order.limit_price if order.limit_price is not None else quote.last_price
        notional = Decimal(order.quantity) * price
        if notional > self._limits.max_order_notional:
            raise OrderRejected(
                RiskRejectionReason.ORDER_NOTIONAL_EXCEEDED,
                f"order notional {notional} > max {self._limits.max_order_notional}",
            )

        current = portfolio.position(order.symbol)
        held = current.quantity if current is not None else 0
        if order.side is OrderSide.BUY:
            if held + order.quantity > self._limits.max_position_quantity:
                raise OrderRejected(
                    RiskRejectionReason.POSITION_QUANTITY_EXCEEDED,
                    f"projected {order.symbol} quantity {held + order.quantity} "
                    f"> max {self._limits.max_position_quantity}",
                )
            gross_after = self._gross_exposure(portfolio, quotes) + notional
            if gross_after > self._limits.max_gross_exposure:
                raise OrderRejected(
                    RiskRejectionReason.GROSS_EXPOSURE_EXCEEDED,
                    f"gross exposure {gross_after} > max {self._limits.max_gross_exposure}",
                )

        realized = portfolio.total_realized_pnl
        if realized < -self._limits.daily_loss_limit:
            raise OrderRejected(
                RiskRejectionReason.DAILY_LOSS_LIMIT_EXCEEDED,
                f"realized loss {realized} beyond limit -{self._limits.daily_loss_limit}",
            )

        return notional

    @staticmethod
    def _gross_exposure(portfolio: Portfolio, quotes: dict[str, Quote]) -> Decimal:
        total = Decimal(0)
        for position in portfolio.positions.values():
            if position.quantity <= 0:
                continue
            quote = quotes.get(position.symbol)
            price = quote.last_price if quote else position.average_price
            total += Decimal(position.quantity) * price
        return total
