"""Storage integration tests — real SQLite file, no server required."""

from datetime import UTC, datetime
from decimal import Decimal

import pytest

from lossfunction.broker.base import Quote
from lossfunction.storage import Repository, int_to_money, money_to_int


@pytest.fixture
async def repo(tmp_path):
    repository = await Repository.connect(tmp_path / "test.db")
    await repository.migrate()
    yield repository
    await repository.close()


async def test_migrations_are_idempotent(tmp_path) -> None:
    repository = await Repository.connect(tmp_path / "idem.db")
    try:
        first = await repository.migrate()
        second = await repository.migrate()
        assert first == [1]
        assert second == []

        cursor = await repository._db.execute(  # noqa: SLF001
            "SELECT version FROM schema_migrations ORDER BY version"
        )
        versions = [row[0] for row in await cursor.fetchall()]
        assert versions == [1]

        cursor = await repository._db.execute(  # noqa: SLF001
            "SELECT name FROM sqlite_master WHERE type = 'table'"
        )
        tables = {row[0] for row in await cursor.fetchall()}
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
        await repository.close()


async def test_money_roundtrip_is_exact() -> None:
    for value in ("80000", "80500.1234", "0.0001", "12345678.9999"):
        assert int_to_money(money_to_int(Decimal(value))) == Decimal(value)
    # Sub-1e-4 residues round half-even: 10000.5 -> 10000 (to even).
    assert money_to_int(Decimal("1.00005")) == 10000
    assert money_to_int(Decimal("1.00015")) == 10002


async def test_order_lifecycle_audited(repo: Repository) -> None:
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
    assert order.limit_price == Decimal("79000")  # exact round-trip
    assert order.created_at.tzinfo is not None

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
    fills = await repo.list_fills("it-1")
    assert len(fills) == 1
    assert fills[0]["price"] == Decimal("79000")

    await repo.upsert_position("005930", 10, Decimal("79000"))
    await repo.upsert_position("005930", 6, Decimal("79100"))
    positions = await repo.get_positions()
    assert len(positions) == 1
    assert positions[0]["symbol"] == "005930"
    assert positions[0]["quantity"] == 6
    assert positions[0]["average_price"] == Decimal("79100")

    events = await repo.get_audit_log(subject="it-1")
    assert [e["event_type"] for e in events] == [
        "order.created",
        "order.status_changed",
        "fill.recorded",
    ]
    position_events = await repo.get_audit_log(subject="005930")
    assert len(position_events) == 2


async def test_quote_insertion(repo: Repository) -> None:
    await repo.insert_quote(
        Quote(
            symbol="035420",
            last_price=Decimal("41000"),
            timestamp=datetime(2026, 9, 14, 0, 0, tzinfo=UTC),
        )
    )
    cursor = await repo._db.execute(  # noqa: SLF001
        "SELECT symbol, price FROM quotes ORDER BY id DESC LIMIT 1"
    )
    row = await cursor.fetchone()
    assert row["symbol"] == "035420"
    assert row["price"] == 410000000  # 41000.0000 in 1e-4 KRW units


async def test_analysis_recorded_as_audit(repo: Repository) -> None:
    await repo.record_analysis("regime", {"regime": "volatile", "used_fallback": False})
    events = await repo.get_audit_log(subject="analysis:regime")
    assert events[-1]["event_type"] == "analysis.regime"
    assert events[-1]["payload"]["regime"] == "volatile"


async def test_concurrent_writes_serialize(tmp_path) -> None:
    """Two repositories on one file must not corrupt under WAL + timeout."""
    import asyncio

    repo_a = await Repository.connect(tmp_path / "conc.db")
    repo_b = await Repository.connect(tmp_path / "conc.db")
    try:
        await repo_a.migrate()
        await asyncio.gather(
            *[
                repo.create_order(
                    client_order_id=f"c-{i}",
                    symbol="005930",
                    side="buy",
                    order_type="market",
                    quantity=1,
                    limit_price=None,
                    status="pending",
                )
                for i, repo in enumerate([repo_a, repo_b] * 10)
            ]
        )
        orders = await repo_a.list_orders()
        assert len(orders) == 20
        events = await repo_a.get_audit_log()
        assert len(events) == 20
    finally:
        await repo_a.close()
        await repo_b.close()
