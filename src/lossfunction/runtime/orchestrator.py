"""Trading runtime orchestrator.

Wires the full event flow (docs/architecture.md §3):

    quote -> strategy decision -> risk check -> gateway submit
         -> execution sync -> portfolio update

and owns restart recovery: pending orders are re-registered at UNKNOWN and
reconciled against the broker, then the portfolio is rebuilt from
broker-side positions. The runtime is deliberately storage-agnostic —
persistence hooks are callbacks; tests run it entirely against a MockBroker.
"""

import logging
from decimal import Decimal

from lossfunction.broker.base import Broker, ExecutionReport, Quote
from lossfunction.domain.order import Order, OrderSide, OrderStatus, OrderType
from lossfunction.domain.portfolio import Portfolio, PositionState
from lossfunction.execution import (
    DuplicateOrderError,
    OrderGateway,
    OrderStateMachine,
    OrderSubmitTimeout,
    Reconciler,
)
from lossfunction.risk import OrderRejected, RiskManager
from lossfunction.strategy import DecisionLayer, MarketSnapshot, Strategy

logger = logging.getLogger(__name__)


class TradingRuntime:
    """Coordinates strategy, risk, execution, and recovery."""

    def __init__(
        self,
        *,
        broker: Broker,
        strategy: Strategy,
        risk: RiskManager,
        machine: OrderStateMachine | None = None,
        order_prefix: str = "ord",
        on_transition=None,
    ) -> None:
        self._broker = broker
        self._machine = machine or OrderStateMachine(on_event=on_transition)
        self._gateway = OrderGateway(broker, self._machine)
        self._reconciler = Reconciler(broker, self._machine)
        self._decisions = DecisionLayer(strategy)
        self._risk = risk
        self._quotes: dict[str, Quote] = {}
        self._portfolio = Portfolio()
        self._order_seq = 0
        self._order_prefix = order_prefix
        # client_order_id -> broker_order_id of submitted-but-unsynced orders
        self._open_local: dict[str, str] = {}

    # ── state accessors ─────────────────────────────────────────────

    @property
    def portfolio(self) -> Portfolio:
        return self._portfolio

    @property
    def machine(self) -> OrderStateMachine:
        return self._machine

    def status_of(self, client_order_id: str) -> OrderStatus | None:
        return self._machine.status(client_order_id)

    def open_local_orders(self) -> dict[str, str]:
        """client_order_id -> broker_order_id for recovery after a crash."""
        return dict(self._open_local)

    # ── event flow ──────────────────────────────────────────────────

    def on_quote(self, quote: Quote) -> MarketSnapshot:
        """Record a quote; returns the snapshot it produced (for tests)."""
        self._quotes[quote.symbol] = quote
        return self.snapshot()

    def snapshot(self) -> MarketSnapshot:
        return MarketSnapshot(
            quotes=dict(self._quotes),
            positions=self._portfolio.positions,
            as_of=f"{self._order_prefix}-session",
        )

    async def run_decision_cycle(self) -> list[Order]:
        """One strategy -> risk -> submit pass; returns submitted orders."""
        snapshot = self.snapshot()
        decision = self._decisions.decide(snapshot)
        submitted: list[Order] = []
        for intent in decision.intents:
            order = self._new_order(
                intent.symbol, intent.side, intent.order_type, intent.quantity, intent.limit_price
            )
            try:
                self._risk.check_order(order, self._portfolio, self._quotes)
            except OrderRejected as exc:
                logger.warning("risk rejected %s: %s", order.client_order_id, exc)
                continue
            try:
                ack = await self._gateway.submit(order)
            except OrderSubmitTimeout:
                # Order sits in UNKNOWN; reconcile before anything else.
                logger.warning(
                    "submit timeout for %s; awaiting reconciliation", order.client_order_id
                )
                continue
            except DuplicateOrderError:  # pragma: no cover - defensive
                logger.warning("duplicate order id for %s", order.client_order_id)
                continue
            self._open_local[order.client_order_id] = ack.broker_order_id
            submitted.append(order)
        return submitted

    def _new_order(
        self,
        symbol: str,
        side: OrderSide,
        order_type: OrderType,
        quantity: int,
        limit_price: str | None,
    ) -> Order:
        self._order_seq += 1
        client_order_id = f"{self._order_prefix}-{self._order_seq:04d}"
        return Order(
            client_order_id=client_order_id,
            symbol=symbol,
            side=side,
            order_type=order_type,
            quantity=quantity,
            limit_price=Decimal(limit_price) if limit_price else None,
        )

    async def sync_execution(self, client_order_id: str) -> OrderStatus | None:
        """Pull the broker-side execution state for one open local order.

        Submitted/partially-filled orders advance from the broker's report;
        UNKNOWN orders go through reconciliation (which also decides
        REJECTED when the broker has no record). Terminal statuses apply any
        discovered fills to the portfolio and clear the local open set.
        """
        broker_order_id = self._open_local.get(client_order_id)
        if broker_order_id is None:
            return self._machine.status(client_order_id)

        status = self._machine.status(client_order_id)
        if status in (OrderStatus.SUBMITTED, OrderStatus.PARTIALLY_FILLED):
            report = await self._broker.get_execution_report(broker_order_id)
            target = Reconciler.target_status(
                report.filled_quantity, report.order_quantity, report.open
            )
            if target is not status:
                status = self._machine.transition(
                    client_order_id,
                    target,
                    reason=(
                        f"execution sync: filled {report.filled_quantity}/"
                        f"{report.order_quantity}, open={report.open}"
                    ),
                )
        elif status is OrderStatus.UNKNOWN:
            status = await self._reconciler.reconcile(client_order_id, broker_order_id)

        if status not in (OrderStatus.SUBMITTED, OrderStatus.PARTIALLY_FILLED):
            await self._apply_fills(client_order_id, broker_order_id)
            self._open_local.pop(client_order_id, None)
        return status

    async def sync_all(self) -> dict[str, OrderStatus]:
        return {
            client_order_id: await self.sync_execution(client_order_id)
            for client_order_id in list(self._open_local)
        }

    async def _apply_fills(self, client_order_id: str, broker_order_id: str) -> None:
        report: ExecutionReport = await self._broker.get_execution_report(broker_order_id)
        order_status = self._machine.status(client_order_id)
        if order_status is not OrderStatus.FILLED or report.filled_quantity <= 0:
            return
        # Reconstruct the fill's side from the order lineage via the report.
        from lossfunction.domain.portfolio import Fill

        fill = Fill(
            client_order_id=client_order_id,
            symbol=report.symbol,
            side=report.side,
            quantity=report.filled_quantity,
            price=report.average_fill_price or Decimal(0),
        )
        self._portfolio = self._portfolio.apply_fill(fill)

    # ── restart recovery ────────────────────────────────────────────

    async def recover(self, pending: dict[str, str]) -> dict[str, OrderStatus]:
        """Restart recovery: reconcile pending orders, rebuild portfolio.

        `pending` maps client_order_id -> broker_order_id for orders that were
        submitted (or whose submission answer was outstanding) when the
        previous process died — see docs/recovery.md.
        """
        results = await self._reconciler.reconcile_pending(pending)
        await self.rebuild_portfolio()
        for client_order_id, status in results.items():
            if status in (OrderStatus.SUBMITTED, OrderStatus.PARTIALLY_FILLED):
                self._open_local[client_order_id] = pending[client_order_id]
        return results

    async def rebuild_portfolio(self) -> Portfolio:
        """Replace local portfolio state with broker-side positions."""
        positions = await self._broker.get_positions()
        self._portfolio = Portfolio(
            positions={
                position.symbol: PositionState(
                    symbol=position.symbol,
                    quantity=position.quantity,
                    average_price=position.average_price,
                )
                for position in positions
            }
        )
        return self._portfolio
