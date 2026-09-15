"""Synthetic paper market — the demo loop.

Feeds deterministic random-walk quotes through the real pipeline
(quote -> decision -> risk -> gateway -> mock fill -> portfolio) and persists
everything to SQLite so the status page shows a living system. Only runs on
the in-memory paper backend; a kill switch blocks new orders exactly as it
would in production wiring.
"""

import asyncio
import logging
import random
from datetime import UTC, datetime
from decimal import Decimal

from lossfunction.broker.base import Broker, Quote
from lossfunction.risk import RiskManager
from lossfunction.runtime.orchestrator import TradingRuntime
from lossfunction.storage import Repository
from lossfunction.strategy import EntryPriceStrategy

logger = logging.getLogger("lossfunction.demo")

_SYMBOLS = {"005930": 80000, "035420": 41000, "069500": 100000}
_ENTRY_PRICES = {"005930": "79600", "035420": "40800", "069500": "99500"}
_QUANTITY = 5


class DemoLoop:
    """One `tick()` per interval; each tick is a full decision cycle."""

    def __init__(
        self,
        *,
        broker: Broker,
        risk: RiskManager,
        repository: Repository,
        interval_seconds: float = 3.0,
        seed: int = 7,
    ) -> None:
        self._runtime = TradingRuntime(
            broker=broker,
            strategy=EntryPriceStrategy(_ENTRY_PRICES, quantity=_QUANTITY),
            risk=risk,
        )
        self._risk = risk
        self._broker = broker
        self._repository = repository
        self._interval = interval_seconds
        self._rng = random.Random(seed)
        self._prices = dict(_SYMBOLS)
        self._persisted_status: dict[str, str] = {}
        self._persisted_fills: set[str] = set()

    async def run(self) -> None:
        while True:
            try:
                await self.tick()
            except asyncio.CancelledError:
                raise
            except Exception:
                logger.exception("demo tick failed; continuing")
            await asyncio.sleep(self._interval)

    async def tick(self) -> None:
        """Advance the synthetic market by one step (public for tests)."""
        now = datetime.now(tz=UTC)
        for symbol in sorted(self._prices):
            drift = self._rng.uniform(-0.004, 0.004)
            self._prices[symbol] = int(round(self._prices[symbol] * (1 + drift)))

        quotes = [
            Quote(symbol=symbol, last_price=Decimal(price), timestamp=now)
            for symbol, price in self._prices.items()
        ]
        for quote in quotes:
            # The mock broker fills against configured prices; demo only ever
            # runs on MockBroker (guarded in the entrypoint).
            if hasattr(self._broker, "set_price"):
                self._broker.set_price(quote.symbol, quote.last_price)
            self._runtime.on_quote(quote)
            await self._repository.insert_quote(quote)

        submitted = await self._runtime.run_decision_cycle()
        broker_ids = self._runtime.open_local_orders()  # captured before sync
        for order in submitted:
            await self._repository.create_order(
                client_order_id=order.client_order_id,
                symbol=order.symbol,
                side=order.side.value,
                order_type=order.order_type.value,
                quantity=order.quantity,
                limit_price=order.limit_price,
                status="submitted",
                mode="paper",
            )
            self._persisted_status[order.client_order_id] = "submitted"

        statuses = await self._runtime.sync_all()
        for client_order_id, status in statuses.items():
            if self._persisted_status.get(client_order_id) != status.value:
                await self._repository.update_order_status(client_order_id, status.value)
                self._persisted_status[client_order_id] = status.value
            if status.value == "filled" and client_order_id not in self._persisted_fills:
                await self._record_fill(client_order_id, broker_ids.get(client_order_id))

        for position in self._runtime.portfolio.positions.values():
            if position.quantity > 0:
                await self._repository.upsert_position(
                    position.symbol, position.quantity, position.average_price
                )

    async def _record_fill(self, client_order_id: str, broker_order_id: str | None) -> None:
        if broker_order_id is None:
            return
        report = await self._broker.get_execution_report(broker_order_id)
        if report.filled_quantity > 0 and report.average_fill_price:
            await self._repository.record_fill(
                client_order_id=client_order_id,
                quantity=report.filled_quantity,
                price=report.average_fill_price,
                executed_at=report.timestamp,
            )
        self._persisted_fills.add(client_order_id)
