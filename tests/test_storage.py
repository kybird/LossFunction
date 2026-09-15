"""Storage integration tests against a real PostgreSQL.

Requires a reachable database (LOSSFUNCTION_TEST_DSN or the default local dev
server on 127.0.0.1:5433 — see scripts/dev_postgres.sh). Skipped otherwise.
"""

import os
from datetime import UTC, datetime
from decimal import Decimal

import pytest

from lossfunction.broker.base import Quote
from lossfunction.storage import Repository

DSN = os.environ.get(
    "LOSSFUNCTION_TEST_DSN", "postgresql://postgres@127.0.0.1:5433/lossfunction_test"
)

pytestmark = pytest.mark.integration


def _pg_available() -> bool:
    import socket
    from urllib.parse import urlparse

    parsed = urlparse(DSN)
    host = parsed.hostname or "127.0.0.1"
    port = parsed.port or 5432
    try:
        with socket.create_connection((host, port), timeout=1):
            return True
    except OSError:
        return False


@pytest.mark.asyncio
async def test_migrations_are_idempotent() -> None:
    if not _pg_available():
        pytest.skip("PostgreSQL not reachable")
    repo = await Repository.connect(DSN)
    try:
        first = await repo.migrate()
        second = await repo.migrate()
        # First run on a fresh DB applies version 1; a second run applies nothing.
        assert 1 in first or first == []
        assert second == []

        async with repo._pool.acquire() as connection:  # noqa: SLF001
            versions = await connection.fetch(
                "SELECT version FROM schema_migrations ORDER BY version"
            )
        assert [row["version"] for row in versions] == [1]

        # Schema objects exist.
        async with repo._pool.acquire() as connection:  # noqa: SLF001
            tables = {
                row["table_name"]
                for row in await connection.fetch(
                    "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
                )
            }
        assert {
            "quotes",
            "candles",
            "orders",
            "fills",
            "positions",
            "audit_log",
            "schema_migrations",
        } <= tables
    finally:
        await repo.close()


@pytest.mark.asyncio
async def test_order_lifecycle_audited() -> None:
    if not _pg_available():
        pytest.skip("PostgreSQL not reachable")
    repo = await Repository.connect(DSN)
    try:
        await repo.migrate()
        async with repo._pool.acquire() as connection:  # noqa: SLF001
            await connection.execute("DELETE FROM audit_log")
            await connection.execute("DELETE FROM fills")
            await connection.execute("DELETE FROM orders")
            await connection.execute("DELETE FROM positions")

        await repo.create_order(
            client_order_id="it-1",
            symbol="005930",
            side="buy",
            order_type="limit",
            quantity=10,
            limit_price=Decimal("79000"),
            status="pending",
        )
        assert await repo.update_order_status("it-1", "submitted", broker_order_id="ODNO-77")
        order = await repo.get_order("it-1")
        assert order is not None
        assert order.status == "submitted"
        assert order.broker_order_id == "ODNO-77"
        assert order.mode == "paper"

        # Unknown order: update reports False, no audit row.
        assert not await repo.update_order_status("nope", "filled")

        filled = await repo.record_fill(
            client_order_id="it-1",
            quantity=10,
            price=Decimal("79000"),
            executed_at=datetime(2026, 9, 14, 0, 30, tzinfo=UTC),
        )
        assert filled
        duplicate = await repo.record_fill(
            client_order_id="it-1",
            quantity=10,
            price=Decimal("79000"),
            executed_at=datetime(2026, 9, 14, 0, 30, tzinfo=UTC),
        )
        assert not duplicate  # natural-key idempotency blocks duplicates
        assert len(await repo.list_fills("it-1")) == 1

        await repo.upsert_position("005930", 10, Decimal("79000"))
        await repo.upsert_position("005930", 6, Decimal("79100"))
        positions = await repo.get_positions()
        assert positions == [
            {
                "symbol": "005930",
                "quantity": 6,
                "average_price": Decimal("79100"),
                "as_of": positions[0]["as_of"],
            }
        ]

        events = await repo.get_audit_log(subject="it-1")
        assert [e["event_type"] for e in events] == [
            "order.created",
            "order.status_changed",
            "fill.recorded",
        ]
        position_events = await repo.get_audit_log(subject="005930")
        assert len(position_events) == 2
    finally:
        await repo.close()


@pytest.mark.asyncio
async def test_quote_insertion() -> None:
    if not _pg_available():
        pytest.skip("PostgreSQL not reachable")
    repo = await Repository.connect(DSN)
    try:
        await repo.migrate()
        async with repo._pool.acquire() as connection:  # noqa: SLF001
            await connection.execute("DELETE FROM quotes")
        await repo.insert_quote(
            Quote(
                symbol="035420",
                last_price=Decimal("41000"),
                timestamp=datetime(2026, 9, 14, 0, 0, tzinfo=UTC),
            )
        )
        async with repo._pool.acquire() as connection:  # noqa: SLF001
            row = await connection.fetchrow(
                "SELECT symbol, price FROM quotes ORDER BY id DESC LIMIT 1"
            )
        assert row["symbol"] == "035420"
        assert row["price"] == Decimal("41000")
    finally:
        await repo.close()
