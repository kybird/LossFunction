"""Demo loop tests — pipeline liveness, kill switch, determinism."""

from datetime import UTC, datetime, timedelta
from decimal import Decimal

from lossfunction.broker.mock import MockBroker
from lossfunction.risk import RiskLimits, RiskManager
from lossfunction.runtime.demo import DemoLoop
from lossfunction.storage import Repository


def _risk() -> RiskManager:
    return RiskManager(
        RiskLimits(
            max_order_notional=Decimal("10000000"),
            max_position_quantity=50,
            max_gross_exposure=Decimal("30000000"),
            daily_loss_limit=Decimal("500000"),
            stale_quote_max_age=timedelta(seconds=60),
        ),
        clock=lambda: datetime.now(tz=UTC),
    )


async def _loop(tmp_path, seed: int = 7) -> tuple[DemoLoop, Repository]:
    repository = await Repository.connect(tmp_path / "demo.db")
    await repository.migrate()
    loop = DemoLoop(broker=MockBroker(), risk=_risk(), repository=repository, seed=seed)
    return loop, repository


async def test_ticks_populate_database(tmp_path) -> None:
    loop, repository = await _loop(tmp_path)
    try:
        for _ in range(30):
            await loop.tick()

        quotes = await repository._db.execute("SELECT COUNT(*) FROM quotes")  # noqa: SLF001
        assert (await quotes.fetchone())[0] >= 90  # 3 symbols × 30 ticks

        orders = await repository.list_orders()
        assert orders, "demo should have produced orders"
        assert any(order.status in ("filled", "submitted") for order in orders)

        fills = await repository.list_recent_fills()
        positions = await repository.get_positions()
        assert fills, "crossing entries should have filled"
        assert positions

        portfolio_positions = loop._runtime.portfolio.positions  # noqa: SLF001
        stored = {p["symbol"]: (p["quantity"], p["average_price"]) for p in positions}
        for symbol, position in portfolio_positions.items():
            if position.quantity > 0:
                assert stored[symbol][0] == position.quantity
    finally:
        await repository.close()


async def test_kill_switch_blocks_demo_orders(tmp_path) -> None:
    loop, repository = await _loop(tmp_path)
    try:
        loop._risk.activate_kill_switch("test halt")  # noqa: SLF001

        for _ in range(5):
            await loop.tick()
        assert await repository.list_orders() == []  # nothing passed risk
        assert loop._runtime.portfolio.positions == {}
    finally:
        await repository.close()


async def test_demo_is_deterministic(tmp_path) -> None:
    loop_a, repo_a = await _loop(tmp_path / "a", seed=42)
    loop_b, repo_b = await _loop(tmp_path / "b", seed=42)
    try:
        for _ in range(40):
            await loop_a.tick()
            await loop_b.tick()

        assert loop_a._prices == loop_b._prices  # noqa: SLF001
        positions_a = sorted(
            (p["symbol"], p["quantity"], str(p["average_price"]))
            for p in await repo_a.get_positions()
        )
        positions_b = sorted(
            (p["symbol"], p["quantity"], str(p["average_price"]))
            for p in await repo_b.get_positions()
        )
        assert positions_a == positions_b
    finally:
        await repo_a.close()
        await repo_b.close()
