"""Status page rendering tests — content, empty state, escaping."""

from datetime import UTC, datetime
from decimal import Decimal

from lossfunction.runtime.web import render_status_page

NOW = datetime(2026, 9, 15, 3, 30, tzinfo=UTC)


def _page(**overrides: object) -> str:
    defaults: dict[str, object] = {
        "trading_mode": "paper",
        "broker": "MockBroker",
        "database_path": "/data/lossfunction.db",
        "positions": [],
        "orders": [],
        "fills": [],
        "audit": [],
        "latest_prices": {},
        "now": NOW,
    }
    defaults.update(overrides)
    return render_status_page(**defaults)  # type: ignore[arg-type]


def test_page_skeleton_and_sections() -> None:
    page = _page()
    for marker in (
        "<!doctype html>",
        "LossFunction",
        "paper",
        "MockBroker",
        "/data/lossfunction.db",
        "Positions",
        "Recent orders",
        "Recent fills",
        "Recent audit events",
        "(no rows)",
        'http-equiv="refresh" content="5"',
        "/healthz",
    ):
        assert marker in page, f"missing: {marker}"


def test_live_mode_marked() -> None:
    assert 'class="mode live"' in _page(trading_mode="live")


class _FakeOrder:
    client_order_id = "ord-0001"
    symbol = "005930"
    side = "buy"
    quantity = 10
    limit_price = Decimal("79000")
    status = "filled"
    created_at = NOW


def test_page_renders_data_rows() -> None:
    page = _page(
        positions=[
            {
                "symbol": "005930",
                "quantity": 10,
                "average_price": Decimal("79000"),
                "as_of": NOW,
            }
        ],
        orders=[_FakeOrder()],
        fills=[
            {
                "id": 1,
                "client_order_id": "ord-0001",
                "quantity": 10,
                "price": Decimal("79000"),
                "executed_at": NOW,
            }
        ],
        audit=[
            {
                "id": 1,
                "event_type": "order.created",
                "subject": "ord-0001",
                "payload": {"status": "pending"},
                "occurred_at": NOW,
            }
        ],
        latest_prices={"005930": Decimal("80500")},
    )
    assert "005930" in page
    assert "79,000" in page  # avg price formatted
    assert "80,500" in page  # latest price
    assert "805,000" in page  # position value 10 * 80500
    assert "ord-0001" in page
    assert "order.created" in page
    assert 'class="pill filled"' in page


def test_db_values_are_escaped() -> None:
    page = _page(
        positions=[
            {
                "symbol": "<script>alert(1)</script>",
                "quantity": 1,
                "average_price": Decimal("1"),
                "as_of": NOW,
            }
        ],
        audit=[
            {
                "id": 1,
                "event_type": "<b>evil</b>",
                "subject": "x&y",
                "payload": {},
                "occurred_at": NOW,
            }
        ],
    )
    assert "<script>alert(1)</script>" not in page
    assert "&lt;script&gt;" in page
    assert "<b>evil</b>" not in page
    assert "x&amp;y" in page


def test_naive_timestamps_coerced_to_utc() -> None:
    from datetime import timedelta

    kst = NOW + timedelta(hours=9)  # same instant, +09:00 offset
    page = _page(
        audit=[
            {
                "id": 1,
                "event_type": "e",
                "subject": None,
                "payload": {},
                "occurred_at": kst,
            }
        ]
    )
    assert "03:30:00" in page  # rendered in UTC regardless of source tz
