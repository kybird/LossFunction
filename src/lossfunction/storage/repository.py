"""Async repository over the trading schema (SQLite, WAL mode).

Every trading-state write (orders, fills, positions) records a matching row
in `audit_log` inside the same transaction — auditability is part of the
write path, not an afterthought layer. Quote ingestion is data capture, not
a state change, and is not audited per-row.

Money crosses the boundary as Decimal and is stored exactly as INTEGER in
1e-4 KRW units. Timestamps cross as tz-aware datetimes and are stored as
UTC ISO 8601 text.
"""

import asyncio
import json
from contextlib import asynccontextmanager
from datetime import UTC, datetime
from decimal import ROUND_HALF_EVEN, Decimal
from pathlib import Path
from typing import Any

import aiosqlite

from lossfunction.broker.base import Quote
from lossfunction.storage.migrations import migrate

_MONEY_EXP = Decimal(10000)


def money_to_int(value: Decimal) -> int:
    """Decimal KRW -> INTEGER in 1e-4 KRW units (exact for 4dp)."""
    return int(value.scaleb(4).to_integral_value(rounding=ROUND_HALF_EVEN))


def int_to_money(value: int) -> Decimal:
    """INTEGER in 1e-4 KRW units -> Decimal KRW."""
    return Decimal(value) / _MONEY_EXP


def _timestamp(moment: datetime) -> str:
    return moment.astimezone(UTC).isoformat()


def _parse_timestamp(raw: str) -> datetime:
    return datetime.fromisoformat(raw)


class OrderRecord:
    """A persisted order row."""

    def __init__(self, row: aiosqlite.Row) -> None:
        self.client_order_id: str = row["client_order_id"]
        self.broker_order_id: str | None = row["broker_order_id"]
        self.symbol: str = row["symbol"]
        self.side: str = row["side"]
        self.order_type: str = row["order_type"]
        self.quantity: int = row["quantity"]
        self.limit_price: Decimal | None = (
            int_to_money(row["limit_price"]) if row["limit_price"] is not None else None
        )
        self.status: str = row["status"]
        self.mode: str = row["mode"]
        self.created_at: datetime = _parse_timestamp(row["created_at"])
        self.updated_at: datetime = _parse_timestamp(row["updated_at"])


class Repository:
    """Async CRUD + audit trail for the trading schema."""

    def __init__(self, db: aiosqlite.Connection) -> None:
        self._db = db
        self._write_lock = asyncio.Lock()

    @classmethod
    async def connect(cls, path: str | Path) -> "Repository":
        """Open (creating if needed) the SQLite database in WAL mode."""
        if isinstance(path, str):
            path = Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        db = await aiosqlite.connect(path)
        db.row_factory = aiosqlite.Row
        await db.execute("PRAGMA journal_mode=WAL")
        await db.execute("PRAGMA busy_timeout=5000")
        await db.execute("PRAGMA foreign_keys=ON")
        return cls(db)

    async def close(self) -> None:
        await self._db.close()

    async def migrate(self) -> list[int]:
        async with self._write_lock:
            return await migrate(self._db)

    @asynccontextmanager
    async def _transaction(self):
        """Serialized write transaction (single writer by design)."""
        async with self._write_lock:
            await self._db.execute("BEGIN IMMEDIATE")
            try:
                yield self._db
            except Exception:
                await self._db.rollback()
                raise
            await self._db.commit()

    # ── quotes ─────────────────────────────────────────────────────

    async def insert_quote(self, quote: Quote) -> None:
        now = _timestamp(datetime.now(tz=UTC))
        async with self._transaction() as db:
            await db.execute(
                "INSERT INTO quotes (symbol, price, quoted_at, received_at) VALUES (?, ?, ?, ?)",
                (
                    quote.symbol,
                    money_to_int(quote.last_price),
                    _timestamp(quote.timestamp),
                    now,
                ),
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
        now = _timestamp(datetime.now(tz=UTC))
        async with self._transaction() as db:
            await db.execute(
                """
                INSERT INTO orders (
                    client_order_id, symbol, side, order_type,
                    quantity, limit_price, status, mode, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    client_order_id,
                    symbol,
                    side,
                    order_type,
                    quantity,
                    money_to_int(limit_price) if limit_price is not None else None,
                    status,
                    mode,
                    now,
                    now,
                ),
            )
            await self._audit(
                db,
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
        now = _timestamp(datetime.now(tz=UTC))
        async with self._transaction() as db:
            cursor = await db.execute(
                "UPDATE orders SET status = ?, "
                "broker_order_id = COALESCE(?, broker_order_id), updated_at = ? "
                "WHERE client_order_id = ?",
                (status, broker_order_id, now, client_order_id),
            )
            updated = cursor.rowcount == 1
            if updated:
                await self._audit(
                    db,
                    "order.status_changed",
                    client_order_id,
                    {"status": status, "broker_order_id": broker_order_id},
                )
            return updated

    async def get_order(self, client_order_id: str) -> OrderRecord | None:
        cursor = await self._db.execute(
            "SELECT * FROM orders WHERE client_order_id = ?", (client_order_id,)
        )
        row = await cursor.fetchone()
        return OrderRecord(row) if row is not None else None

    async def list_orders(self, *, status: str | None = None) -> list[OrderRecord]:
        if status is None:
            cursor = await self._db.execute("SELECT * FROM orders ORDER BY created_at")
            rows = await cursor.fetchall()
        else:
            cursor = await self._db.execute(
                "SELECT * FROM orders WHERE status = ? ORDER BY created_at", (status,)
            )
            rows = await cursor.fetchall()
        return [OrderRecord(row) for row in rows]

    async def list_recent_orders(self, limit: int = 50) -> list[OrderRecord]:
        cursor = await self._db.execute(
            "SELECT * FROM orders ORDER BY created_at DESC, rowid DESC LIMIT ?",
            (limit,),
        )
        rows = await cursor.fetchall()
        return [OrderRecord(row) for row in rows]

    async def latest_quotes(self) -> dict[str, Decimal]:
        """Latest stored price per symbol (for mark-to-market display)."""
        cursor = await self._db.execute(
            "SELECT symbol, price FROM quotes "
            "WHERE id IN (SELECT MAX(id) FROM quotes GROUP BY symbol)"
        )
        rows = await cursor.fetchall()
        return {row["symbol"]: int_to_money(row["price"]) for row in rows}

    async def latest_audit(self, limit: int = 20) -> list[dict[str, Any]]:
        cursor = await self._db.execute(
            "SELECT * FROM audit_log ORDER BY id DESC LIMIT ?", (limit,)
        )
        rows = await cursor.fetchall()
        events = [
            {
                "id": row["id"],
                "event_type": row["event_type"],
                "subject": row["subject"],
                "payload": json.loads(row["payload"]),
                "occurred_at": _parse_timestamp(row["occurred_at"]),
            }
            for row in rows
        ]
        return list(reversed(events))

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
        async with self._transaction() as db:
            cursor = await db.execute(
                "INSERT INTO fills (client_order_id, quantity, price, executed_at) "
                "VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING RETURNING id",
                (
                    client_order_id,
                    quantity,
                    money_to_int(price),
                    _timestamp(executed_at),
                ),
            )
            row = await cursor.fetchone()
            if row is None:
                return False
            await self._audit(
                db,
                "fill.recorded",
                client_order_id,
                {"quantity": quantity, "price": str(price)},
            )
            return True

    async def list_fills(self, client_order_id: str) -> list[dict[str, Any]]:
        cursor = await self._db.execute(
            "SELECT * FROM fills WHERE client_order_id = ? ORDER BY executed_at",
            (client_order_id,),
        )
        rows = await cursor.fetchall()
        return [
            {
                "id": row["id"],
                "client_order_id": row["client_order_id"],
                "quantity": row["quantity"],
                "price": int_to_money(row["price"]),
                "executed_at": _parse_timestamp(row["executed_at"]),
            }
            for row in rows
        ]

    async def list_recent_fills(self, limit: int = 50) -> list[dict[str, Any]]:
        cursor = await self._db.execute(
            "SELECT * FROM fills ORDER BY executed_at DESC, id DESC LIMIT ?",
            (limit,),
        )
        rows = await cursor.fetchall()
        return [
            {
                "id": row["id"],
                "client_order_id": row["client_order_id"],
                "quantity": row["quantity"],
                "price": int_to_money(row["price"]),
                "executed_at": _parse_timestamp(row["executed_at"]),
            }
            for row in rows
        ]

    # ── positions ──────────────────────────────────────────────────

    async def upsert_position(self, symbol: str, quantity: int, average_price: Decimal) -> None:
        now = _timestamp(datetime.now(tz=UTC))
        async with self._transaction() as db:
            await db.execute(
                "INSERT INTO positions (symbol, quantity, average_price, as_of) "
                "VALUES (?, ?, ?, ?) "
                "ON CONFLICT (symbol) DO UPDATE SET "
                "quantity = excluded.quantity, "
                "average_price = excluded.average_price, as_of = excluded.as_of",
                (symbol, quantity, money_to_int(average_price), now),
            )
            await self._audit(
                db,
                "position.updated",
                symbol,
                {"quantity": quantity, "average_price": str(average_price)},
            )

    async def get_positions(self) -> list[dict[str, Any]]:
        cursor = await self._db.execute("SELECT * FROM positions ORDER BY symbol")
        rows = await cursor.fetchall()
        return [
            {
                "symbol": row["symbol"],
                "quantity": row["quantity"],
                "average_price": int_to_money(row["average_price"]),
                "as_of": _parse_timestamp(row["as_of"]),
            }
            for row in rows
        ]

    # ── audit ──────────────────────────────────────────────────────

    async def record_event(self, event_type: str, subject: str, payload: dict[str, Any]) -> None:
        """Persist an operational event (controls, lifecycle) to the audit log."""
        async with self._transaction() as db:
            await self._audit(db, event_type, subject, payload)

    async def record_analysis(self, kind: str, payload: dict[str, Any]) -> None:
        """Persist a model-analysis result (GLM/MLP) for auditability."""
        async with self._transaction() as db:
            await self._audit(db, f"analysis.{kind}", f"analysis:{kind}", payload)

    async def get_audit_log(self, subject: str | None = None) -> list[dict[str, Any]]:
        if subject is None:
            cursor = await self._db.execute("SELECT * FROM audit_log ORDER BY id")
        else:
            cursor = await self._db.execute(
                "SELECT * FROM audit_log WHERE subject = ? ORDER BY id", (subject,)
            )
        rows = await cursor.fetchall()
        return [
            {
                "id": row["id"],
                "event_type": row["event_type"],
                "subject": row["subject"],
                "payload": json.loads(row["payload"]),
                "occurred_at": _parse_timestamp(row["occurred_at"]),
            }
            for row in rows
        ]

    @staticmethod
    async def _audit(
        db: aiosqlite.Connection,
        event_type: str,
        subject: str,
        payload: dict[str, Any],
    ) -> None:
        await db.execute(
            "INSERT INTO audit_log (event_type, subject, payload, occurred_at) VALUES (?, ?, ?, ?)",
            (
                event_type,
                subject,
                json.dumps(payload),
                _timestamp(datetime.now(tz=UTC)),
            ),
        )
