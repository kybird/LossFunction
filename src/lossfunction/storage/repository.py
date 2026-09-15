"""Async repository over the trading schema.

Every trading-state write (orders, fills, positions) records a matching row in
`audit_log` inside the same transaction — auditability is not an afterthought
layer, it is part of the write path. Quote ingestion is data capture, not a
state change, and is not audited per-row.
"""

from datetime import UTC, datetime
from decimal import Decimal
from typing import Any

import asyncpg

from lossfunction.broker.base import Quote
from lossfunction.storage.migrations import migrate


class OrderRecord:
    """A persisted order row."""

    def __init__(self, row: asyncpg.Record) -> None:
        self.client_order_id: str = row["client_order_id"]
        self.broker_order_id: str | None = row["broker_order_id"]
        self.symbol: str = row["symbol"]
        self.side: str = row["side"]
        self.order_type: str = row["order_type"]
        self.quantity: int = row["quantity"]
        self.limit_price: Decimal | None = row["limit_price"]
        self.status: str = row["status"]
        self.mode: str = row["mode"]
        self.created_at: datetime = row["created_at"]
        self.updated_at: datetime = row["updated_at"]


class Repository:
    """Async CRUD + audit trail for the trading schema."""

    def __init__(self, pool: asyncpg.Pool) -> None:
        self._pool = pool

    @classmethod
    async def connect(cls, dsn: str, *, min_size: int = 1, max_size: int = 5) -> "Repository":
        pool = await asyncpg.create_pool(dsn, min_size=min_size, max_size=max_size)
        return cls(pool)

    async def close(self) -> None:
        await self._pool.close()

    async def migrate(self) -> list[int]:
        return await migrate(self._pool)

    # ── quotes ─────────────────────────────────────────────────────

    async def insert_quote(self, quote: Quote) -> None:
        async with self._pool.acquire() as connection:
            await connection.execute(
                "INSERT INTO quotes (symbol, price, quoted_at) VALUES ($1, $2, $3)",
                quote.symbol,
                quote.last_price,
                quote.timestamp,
            )

    # ── orders ─────────────────────────────────────────────────────

    async def create_order(
        self,
        *,
        client_order_id: str,
        symbol: str,
        side: str,
        order_type: str,
        quantity: int,
        limit_price: Decimal | None,
        status: str,
        mode: str = "paper",
    ) -> None:
        async with self._pool.acquire() as connection, connection.transaction():
            await connection.execute(
                """
                    INSERT INTO orders (
                        client_order_id, symbol, side, order_type,
                        quantity, limit_price, status, mode
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    """,
                client_order_id,
                symbol,
                side,
                order_type,
                quantity,
                limit_price,
                status,
                mode,
            )
            await self._audit(
                connection,
                "order.created",
                client_order_id,
                {
                    "symbol": symbol,
                    "side": side,
                    "order_type": order_type,
                    "quantity": quantity,
                    "limit_price": str(limit_price) if limit_price else None,
                    "status": status,
                    "mode": mode,
                },
            )

    async def update_order_status(
        self,
        client_order_id: str,
        status: str,
        *,
        broker_order_id: str | None = None,
    ) -> bool:
        """Update status (and optionally broker id); returns False if unknown order."""
        async with self._pool.acquire() as connection, connection.transaction():
            row = await connection.execute(
                """
                    UPDATE orders
                       SET status = $2,
                           broker_order_id = COALESCE($3, broker_order_id),
                           updated_at = now()
                     WHERE client_order_id = $1
                    """,
                client_order_id,
                status,
                broker_order_id,
            )
            updated = row.endswith(" 1")
            if updated:
                await self._audit(
                    connection,
                    "order.status_changed",
                    client_order_id,
                    {"status": status, "broker_order_id": broker_order_id},
                )
            return updated

    async def get_order(self, client_order_id: str) -> OrderRecord | None:
        async with self._pool.acquire() as connection:
            row = await connection.fetchrow(
                "SELECT * FROM orders WHERE client_order_id = $1", client_order_id
            )
        return OrderRecord(row) if row is not None else None

    async def list_orders(self, *, status: str | None = None) -> list[OrderRecord]:
        async with self._pool.acquire() as connection:
            if status is None:
                rows = await connection.fetch("SELECT * FROM orders ORDER BY created_at")
            else:
                rows = await connection.fetch(
                    "SELECT * FROM orders WHERE status = $1 ORDER BY created_at", status
                )
        return [OrderRecord(row) for row in rows]

    # ── fills ──────────────────────────────────────────────────────

    async def record_fill(
        self,
        *,
        client_order_id: str,
        quantity: int,
        price: Decimal,
        executed_at: datetime | None = None,
    ) -> bool:
        """Insert a fill (idempotent on its natural key)."""
        executed_at = executed_at or datetime.now(tz=UTC)
        async with self._pool.acquire() as connection, connection.transaction():
            inserted = await connection.fetchval(
                """
                    INSERT INTO fills (client_order_id, quantity, price, executed_at)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT DO NOTHING
                    RETURNING id
                    """,
                client_order_id,
                quantity,
                price,
                executed_at,
            )
            if inserted is not None:
                await self._audit(
                    connection,
                    "fill.recorded",
                    client_order_id,
                    {"quantity": quantity, "price": str(price)},
                )
                return True
            return False

    async def list_fills(self, client_order_id: str) -> list[dict[str, Any]]:
        async with self._pool.acquire() as connection:
            rows = await connection.fetch(
                "SELECT * FROM fills WHERE client_order_id = $1 ORDER BY executed_at",
                client_order_id,
            )
        return [dict(row) for row in rows]

    # ── positions ──────────────────────────────────────────────────

    async def upsert_position(self, symbol: str, quantity: int, average_price: Decimal) -> None:
        async with self._pool.acquire() as connection, connection.transaction():
            await connection.execute(
                """
                    INSERT INTO positions (symbol, quantity, average_price, as_of)
                    VALUES ($1, $2, $3, now())
                    ON CONFLICT (symbol) DO UPDATE
                       SET quantity = EXCLUDED.quantity,
                           average_price = EXCLUDED.average_price,
                           as_of = now()
                    """,
                symbol,
                quantity,
                average_price,
            )
            await self._audit(
                connection,
                "position.updated",
                symbol,
                {"quantity": quantity, "average_price": str(average_price)},
            )

    async def get_positions(self) -> list[dict[str, Any]]:
        async with self._pool.acquire() as connection:
            rows = await connection.fetch("SELECT * FROM positions ORDER BY symbol")
        return [dict(row) for row in rows]

    # ── audit ──────────────────────────────────────────────────────

    async def record_analysis(self, kind: str, payload: dict[str, Any]) -> None:
        """Persist a model-analysis result (GLM/MLP) for auditability."""
        async with self._pool.acquire() as connection:
            await self._audit(connection, f"analysis.{kind}", f"analysis:{kind}", payload)

    async def get_audit_log(self, subject: str | None = None) -> list[dict[str, Any]]:
        async with self._pool.acquire() as connection:
            if subject is None:
                rows = await connection.fetch("SELECT * FROM audit_log ORDER BY id")
            else:
                rows = await connection.fetch(
                    "SELECT * FROM audit_log WHERE subject = $1 ORDER BY id", subject
                )
        return [self._decode(row) for row in rows]

    @staticmethod
    def _decode(row: asyncpg.Record) -> dict[str, Any]:
        """Normalize a row: asyncpg returns jsonb columns as text."""
        import json

        record = dict(row)
        if isinstance(record.get("payload"), str):
            record["payload"] = json.loads(record["payload"])
        return record

    @staticmethod
    async def _audit(
        connection: asyncpg.Connection,
        event_type: str,
        subject: str,
        payload: dict[str, Any],
    ) -> None:
        import json

        await connection.execute(
            "INSERT INTO audit_log (event_type, subject, payload) VALUES ($1, $2, $3)",
            event_type,
            subject,
            json.dumps(payload),
        )
