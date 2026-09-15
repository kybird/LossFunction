"""Single-file HTML status page — the lightweight frontend.

Server-side rendered from SQLite, framework-free, zero external assets
(no CDN, no JS build). Auto-refreshes via <meta http-equiv="refresh">.
Every DB-sourced string is HTML-escaped; the page is read-only.
"""

import html
from datetime import UTC, datetime
from decimal import Decimal
from typing import Any

_STYLE = """
    body { font-family: -apple-system, 'Segoe UI', Roboto, sans-serif;
           background: #0f1115; color: #d7dce3; margin: 0; padding: 24px;
           max-width: 1080px; }
    h1 { font-size: 22px; margin: 0 0 4px; }
    h1 .mode { font-size: 13px; padding: 2px 8px; border-radius: 4px;
               background: #234; color: #9fc3ff; vertical-align: middle; }
    h1 .mode.live { background: #5a1f1f; color: #ff9c9c; }
    .meta { color: #7b8494; font-size: 13px; margin-bottom: 20px; }
    h2 { font-size: 14px; color: #9fb0c3; text-transform: uppercase;
         letter-spacing: .06em; margin: 24px 0 8px; }
    table { border-collapse: collapse; width: 100%; font-size: 13px; }
    th, td { text-align: left; padding: 6px 10px; border-bottom: 1px solid #232833; }
    th { color: #7b8494; font-weight: 500; }
    td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
    .empty { color: #566070; font-style: italic; padding: 8px 2px; }
    .pill { padding: 1px 7px; border-radius: 9px; font-size: 12px; background: #2a2f3a; }
    .pill.filled { color: #8fd694; } .pill.cancelled { color: #c9a86a; }
    .pill.rejected { color: #ff9c9c; } .pill.unknown { color: #e2b93b; }
    footer { margin-top: 28px; color: #566070; font-size: 12px; }
    .kill { font-size: 12px; padding: 2px 8px; border-radius: 4px;
            background: #5a1f1f; color: #ff9c9c; }
    .kill button { margin-left: 8px; font-size: 12px; padding: 2px 10px;
                   cursor: pointer; }
"""

_STATUS_PILL = {"filled", "cancelled", "rejected", "unknown"}


def _esc(value: Any) -> str:
    return html.escape(str(value), quote=True)


def _money(value: Decimal | None) -> str:
    if value is None:
        return "—"
    return f"{value:,.4f}".rstrip("0").rstrip(".") if value else "0"


def _table(headers: list[str], rows: list[list[str]], *, numeric_from: int = 1) -> str:
    if not rows:
        return '<p class="empty">(no rows)</p>'
    head = "".join(
        f'<th class="{"num" if i >= numeric_from else ""}">{_esc(h)}</th>'
        for i, h in enumerate(headers)
    )
    body = "".join(
        "<tr>"
        + "".join(
            f'<td class="{"num" if i >= numeric_from else ""}">{cell}</td>'
            for i, cell in enumerate(row)
        )
        + "</tr>"
        for row in rows
    )
    return f"<table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>"


def _status_cell(status: str) -> str:
    cls = status if status in _STATUS_PILL else ""
    return f'<span class="pill {cls}">{_esc(status)}</span>'


def render_status_page(
    *,
    trading_mode: str,
    broker: str,
    database_path: str,
    kill_switch: bool = False,
    kill_reason: str | None = None,
    positions: list[dict[str, Any]],
    orders: list[Any],
    fills: list[dict[str, Any]],
    audit: list[dict[str, Any]],
    latest_prices: dict[str, Decimal] | None = None,
    now: datetime | None = None,
) -> str:
    """Render the full HTML status page from repository rows."""
    latest_prices = latest_prices or {}
    now = now or datetime.now(tz=UTC)

    position_rows = []
    for position in positions:
        symbol = position["symbol"]
        price = latest_prices.get(symbol)
        value = f"{_money(Decimal(position['quantity']) * price)}" if price else "—"
        position_rows.append(
            [
                _esc(symbol),
                str(position["quantity"]),
                _money(position["average_price"]),
                _money(price) if price else "—",
                value,
            ]
        )

    order_rows = [
        [
            _esc(order.client_order_id),
            _esc(order.symbol),
            _esc(order.side),
            _esc(order.quantity),
            _money(order.limit_price),
            _status_cell(order.status),
            _esc(order.created_at.astimezone(UTC).strftime("%m-%d %H:%M:%S")),
        ]
        for order in orders
    ]

    fill_rows = [
        [
            _esc(fill["client_order_id"]),
            _money(fill["price"]),
            str(fill["quantity"]),
            _esc(fill["executed_at"].astimezone(UTC).strftime("%m-%d %H:%M:%S")),
        ]
        for fill in fills
    ]

    audit_rows = [
        [
            _esc(event["occurred_at"].astimezone(UTC).strftime("%m-%d %H:%M:%S")),
            _esc(event["event_type"]),
            _esc(event["subject"] or ""),
            _esc(str(event["payload"])[:120]),
        ]
        for event in audit
    ]

    mode_class = " live" if trading_mode == "live" else ""
    return f"""<!doctype html>
<html lang="ko"><head><meta charset="utf-8">
<meta http-equiv="refresh" content="5">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>LossFunction · {_esc(trading_mode)}</title>
<style>{_STYLE}</style></head>
<body>
<h1>LossFunction <span class="mode{_esc(mode_class)}">{_esc(trading_mode)}</span>
<span class="kill">{
        "KILL SWITCH ON" + (" · " + _esc(kill_reason) if kill_reason else "") if kill_switch else ""
    }
<button onclick="toggleKill({str(not kill_switch).lower()})">{
        "해제" if kill_switch else "KILL"
    }</button></span></h1>
<p class="meta">broker {_esc(broker)} · db {_esc(database_path)} ·
{_esc(now.astimezone(UTC).strftime("%Y-%m-%d %H:%M:%S UTC"))} · 새로고침 5초</p>

<h2>Positions</h2>
{_table(["symbol", "qty", "avg price", "last", "value"], position_rows, numeric_from=1)}

<h2>Recent orders</h2>
{
        _table(
            ["order id", "symbol", "side", "qty", "limit", "status", "created"],
            order_rows,
            numeric_from=3,
        )
    }

<h2>Recent fills</h2>
{_table(["order id", "price", "qty", "executed"], fill_rows, numeric_from=1)}

<h2>Recent audit events</h2>
{_table(["at", "event", "subject", "payload"], audit_rows, numeric_from=99)}

<footer>JSON: <code>GET /healthz</code> · 제어: <code>POST /control/kill-switch</code>
(loopback 전용)</footer>
<script>
function toggleKill(on) {{
  var msg = on ? 'kill switch를 켭니다. 새 주문이 차단됩니다.' : 'kill switch를 해제합니다.';
  if (!confirm(msg)) return;
  fetch('/control/kill-switch', {{
    method: 'POST',
    headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify({{activate: on, reason: 'manual (web)'}})
  }}).then(function () {{ location.reload(); }});
}}
</script>
</body></html>"""
