"""Backtesting engine.

Simulates the live event flow (quote -> decision -> risk -> fill) over
historical bars with a MockBroker. The feed is strictly sequential: a bar
becomes visible only when the cursor reaches it, so look-ahead access is a
raised error, not a discipline. Results are pure data — identical bars,
config, and strategy produce identical results.
"""

from datetime import datetime
from decimal import Decimal
from typing import Any

from pydantic import BaseModel, ConfigDict, Field

from lossfunction.broker.base import OrderRequest, Quote
from lossfunction.broker.mock import MockBroker
from lossfunction.domain.order import Order, OrderSide
from lossfunction.domain.portfolio import Fill, Portfolio
from lossfunction.risk import OrderRejected, RiskManager
from lossfunction.strategy import MarketSnapshot, Strategy


class LookaheadError(Exception):
    """Attempted access to a bar at or beyond the feed cursor."""


class Bar(BaseModel):
    model_config = ConfigDict(frozen=True)

    symbol: str
    timestamp: datetime
    open: Decimal
    high: Decimal
    low: Decimal
    close: Decimal
    volume: int = 0


class BacktestFeed:
    """Sequential bar cursor; future bars are unreachable by construction."""

    def __init__(self, bars: list[Bar]) -> None:
        ordered = sorted(bars, key=lambda bar: bar.timestamp)
        if ordered != bars:
            msg = "bars must be pre-sorted by timestamp"
            raise ValueError(msg)
        self._bars = bars
        self._cursor = 0

    def has_next(self) -> bool:
        return self._cursor < len(self._bars)

    def next(self) -> Bar:
        if not self.has_next():
            msg = "feed exhausted"
            raise StopIteration(msg)
        bar = self._bars[self._cursor]
        self._cursor += 1
        return bar

    def bar_at(self, index: int) -> Bar:
        """Only past/current bars are addressable (look-ahead guard)."""
        if index >= self._cursor:
            msg = f"index {index} is at or beyond cursor {self._cursor}"
            raise LookaheadError(msg)
        return self._bars[index]

    @property
    def cursor(self) -> int:
        return self._cursor


class BacktestConfig(BaseModel):
    model_config = ConfigDict(frozen=True)

    initial_cash: Decimal = Field(default=Decimal("100000000"))
    commission_rate: Decimal = Field(default=Decimal("0.00015"))  # 0.015%
    tax_rate: Decimal = Field(default=Decimal("0.0015"))  # 0.15%, sells only


class FillRecord(BaseModel):
    model_config = ConfigDict(frozen=True)

    client_order_id: str
    symbol: str
    side: OrderSide
    quantity: int
    price: Decimal
    commission: Decimal
    tax: Decimal
    timestamp: datetime


class BacktestResult(BaseModel):
    model_config = ConfigDict(frozen=True)

    config: BacktestConfig
    cash: Decimal
    positions: dict[str, tuple[int, Decimal]]  # symbol -> (qty, avg price)
    total_commission: Decimal
    total_tax: Decimal
    realized_pnl: Decimal
    equity: Decimal
    equity_curve: tuple[tuple[datetime, Decimal], ...]
    fills: tuple[FillRecord, ...]
    decisions: tuple[dict[str, Any], ...]  # rationale + intent count per cycle


class BacktestEngine:
    """Runs a strategy over bars with fees, tax, and risk checks."""

    def __init__(
        self,
        strategy: Strategy,
        config: BacktestConfig | None = None,
        risk: RiskManager | None = None,
    ) -> None:
        self._strategy = strategy
        self._config = config or BacktestConfig()
        self._risk = risk

    async def run(self, bars: list[Bar]) -> BacktestResult:
        feed = BacktestFeed(bars)
        broker = MockBroker()
        portfolio = Portfolio()
        quotes: dict[str, Quote] = {}
        cash = self._config.initial_cash
        fills: list[FillRecord] = []
        decisions: list[dict[str, Any]] = []
        equity_curve: list[tuple[datetime, Decimal]] = []
        order_seq = 0

        while feed.has_next():
            bar = feed.next()
            quotes[bar.symbol] = Quote(
                symbol=bar.symbol, last_price=bar.close, timestamp=bar.timestamp
            )
            broker.set_price(bar.symbol, bar.close)

            snapshot = MarketSnapshot(
                quotes=dict(quotes),
                positions=portfolio.positions,
                as_of=bar.timestamp.isoformat(),
            )
            decision = self._strategy.decide(snapshot)
            decisions.append(
                {
                    "at": bar.timestamp.isoformat(),
                    "rationale": decision.rationale,
                    "intents": len(decision.intents),
                }
            )

            for intent in decision.intents:
                order_seq += 1
                order = Order(
                    client_order_id=f"bt-{order_seq:04d}",
                    symbol=intent.symbol,
                    side=intent.side,
                    order_type=intent.order_type,
                    quantity=intent.quantity,
                    limit_price=(Decimal(intent.limit_price) if intent.limit_price else None),
                )
                if self._risk is not None:
                    try:
                        self._risk.check_order(order, portfolio, quotes)
                    except OrderRejected:
                        continue
                ack = await broker.submit_order(
                    OrderRequest(
                        client_order_id=order.client_order_id,
                        symbol=order.symbol,
                        side=order.side,
                        order_type=order.order_type,
                        quantity=order.quantity,
                        limit_price=order.limit_price,
                    )
                )
                report = await broker.get_execution_report(ack.broker_order_id)
                if report.filled_quantity <= 0:
                    continue
                price = report.average_fill_price or bar.close
                notional = Decimal(report.filled_quantity) * price
                commission = notional * self._config.commission_rate
                tax = (
                    notional * self._config.tax_rate
                    if intent.side is OrderSide.SELL
                    else Decimal(0)
                )
                if intent.side is OrderSide.BUY:
                    cash -= notional + commission
                else:
                    cash += notional - commission - tax
                portfolio = portfolio.apply_fill(
                    Fill(
                        client_order_id=order.client_order_id,
                        symbol=order.symbol,
                        side=intent.side,
                        quantity=report.filled_quantity,
                        price=price,
                    )
                )
                fills.append(
                    FillRecord(
                        client_order_id=order.client_order_id,
                        symbol=order.symbol,
                        side=intent.side,
                        quantity=report.filled_quantity,
                        price=price,
                        commission=commission,
                        tax=tax,
                        timestamp=bar.timestamp,
                    )
                )

            equity = cash + sum(
                (
                    Decimal(position.quantity) * quotes[position.symbol].last_price
                    for position in portfolio.positions.values()
                    if position.quantity > 0 and position.symbol in quotes
                ),
                start=Decimal(0),
            )
            equity_curve.append((bar.timestamp, equity))

        return BacktestResult(
            config=self._config,
            cash=cash,
            positions={
                position.symbol: (position.quantity, position.average_price)
                for position in portfolio.positions.values()
                if position.quantity > 0
            },
            total_commission=sum((f.commission for f in fills), start=Decimal(0)),
            total_tax=sum((f.tax for f in fills), start=Decimal(0)),
            realized_pnl=portfolio.total_realized_pnl,
            equity=equity_curve[-1][1] if equity_curve else self._config.initial_cash,
            equity_curve=tuple(equity_curve),
            fills=tuple(fills),
            decisions=tuple(decisions),
        )
