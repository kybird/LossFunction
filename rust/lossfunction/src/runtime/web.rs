//! Single-file HTML status page — the lightweight frontend.
//!
//! Server-side rendered from SQLite, framework-free, zero external assets.
//! Every DB-sourced string is HTML-escaped; the page is read-only except
//! for the kill-switch control served by the same listener.

use crate::storage::{AuditRow, RecentFill, RecentOrder, StoredPosition};

/// Escape DB values for safe HTML interpolation.
pub fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn money(value: &Option<rust_decimal::Decimal>) -> String {
    match value {
        None => "—".to_string(),
        Some(decimal) => {
            let text = decimal.to_string();
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    }
}

fn status_pill(status: &str) -> String {
    let known = ["filled", "cancelled", "rejected", "unknown"];
    let class = if known.contains(&status) { status } else { "" };
    format!(r#"<span class="pill {class}">{}</span>"#, esc(status))
}

const STYLE: &str = r#"
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
.kill { font-size: 12px; padding: 2px 8px; border-radius: 4px;
        background: #5a1f1f; color: #ff9c9c; }
.kill button { margin-left: 8px; font-size: 12px; padding: 2px 10px;
               cursor: pointer; }
footer { margin-top: 28px; color: #566070; font-size: 12px; }
"#;

/// Everything the status page needs, pre-fetched by the handler.
#[derive(Debug, Default)]
pub struct StatusPageData {
    pub trading_mode: String,
    pub broker: String,
    pub database_path: String,
    pub kill_switch: bool,
    pub kill_reason: Option<String>,
    pub now_utc: String,
    pub positions: Vec<StoredPosition>,
    pub latest_prices: Vec<(String, rust_decimal::Decimal)>,
    pub orders: Vec<RecentOrder>,
    pub fills: Vec<RecentFill>,
    pub audit: Vec<AuditRow>,
}

fn table(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    if rows.is_empty() {
        return r#"<p class="empty">(no rows)</p>"#.to_string();
    }
    let head = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let class = if i >= 1 { " class=\"num\"" } else { "" };
            format!("<th{class}>{}</th>", esc(h))
        })
        .collect::<Vec<_>>()
        .join("");
    let body = rows
        .into_iter()
        .map(|row| {
            "<tr>".to_string()
                + &row
                    .iter()
                    .enumerate()
                    .map(|(i, cell)| {
                        let class = if i >= 1 { " class=\"num\"" } else { "" };
                        format!("<td{class}>{cell}</td>")
                    })
                    .collect::<Vec<_>>()
                    .join("")
                + "</tr>"
        })
        .collect::<Vec<_>>()
        .join("");
    format!("<table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>")
}

/// Render the full HTML status page from pre-fetched data.
pub fn render_status_page(data: &StatusPageData) -> String {
    let prices: std::collections::HashMap<&str, &rust_decimal::Decimal> = data
        .latest_prices
        .iter()
        .map(|(symbol, price)| (symbol.as_str(), price))
        .collect();

    let position_rows = data
        .positions
        .iter()
        .map(|position| {
            let last = prices.get(position.symbol.as_str());
            let value = last
                .map(|price| {
                    money(&Some(
                        rust_decimal::Decimal::from(position.quantity) * *price,
                    ))
                })
                .unwrap_or_else(|| "—".to_string());
            vec![
                esc(&position.symbol),
                position.quantity.to_string(),
                money(&Some(position.average_price)),
                last.map(|p| money(&Some(**p)))
                    .unwrap_or_else(|| "—".to_string()),
                value,
            ]
        })
        .collect();

    let order_rows = data
        .orders
        .iter()
        .map(|order| {
            vec![
                esc(&order.client_order_id),
                esc(&order.symbol),
                esc(&order.side),
                order.quantity.to_string(),
                money(&order.limit_price),
                status_pill(&order.status),
                order.created_at.format("%m-%d %H:%M:%S").to_string(),
            ]
        })
        .collect();

    let fill_rows = data
        .fills
        .iter()
        .map(|fill| {
            vec![
                esc(&fill.client_order_id),
                money(&Some(fill.price)),
                fill.quantity.to_string(),
                fill.executed_at.format("%m-%d %H:%M:%S").to_string(),
            ]
        })
        .collect();

    let audit_rows = data
        .audit
        .iter()
        .map(|event| {
            let payload = event.payload.to_string();
            vec![
                event.occurred_at.format("%m-%d %H:%M:%S").to_string(),
                esc(&event.event_type),
                esc(event.subject.as_deref().unwrap_or("")),
                esc(&payload.chars().take(120).collect::<String>()),
            ]
        })
        .collect();

    let mode_class = if data.trading_mode == "live" {
        " live"
    } else {
        ""
    };
    let kill_badge = if data.kill_switch {
        let reason = data.kill_reason.as_deref().unwrap_or("");
        format!(
            r#"<span class="kill">KILL SWITCH ON · {} <button onclick="toggleKill(false)">해제</button></span>"#,
            esc(reason)
        )
    } else {
        r#"<span class="kill"><button onclick="toggleKill(true)">KILL</button></span>"#.to_string()
    };

    format!(
        r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8">
<meta http-equiv="refresh" content="5">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>LossFunction · {mode}</title>
<style>{STYLE}</style></head>
<body>
<h1>LossFunction <span class="mode{mode_class}">{mode}</span> {kill_badge}</h1>
<p class="meta">broker {broker} · db {db} · {now} · 새로고침 5초</p>

<h2>Positions</h2>
{positions}

<h2>Recent orders</h2>
{orders}

<h2>Recent fills</h2>
{fills}

<h2>Recent audit events</h2>
{audit}

<footer>JSON: <code>GET /healthz</code> · 제어: <code>POST /control/kill-switch</code>
(loopback 전용)</footer>
<script>
function toggleKill(on) {{
  var msg = on ? 'kill switch를 켭니다. 새 주문이 차단됩니다.'
               : 'kill switch를 해제합니다.';
  if (!confirm(msg)) return;
  fetch('/control/kill-switch', {{
    method: 'POST',
    headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify({{activate: on, reason: 'manual (web)'}})
  }}).then(function () {{ location.reload(); }});
}}
</script>
</body></html>"#,
        mode = esc(&data.trading_mode),
        broker = esc(&data.broker),
        db = esc(&data.database_path),
        now = esc(&data.now_utc),
        positions = table(
            &["symbol", "qty", "avg price", "last", "value"],
            position_rows
        ),
        orders = table(
            &["order id", "symbol", "side", "qty", "limit", "status", "created"],
            order_rows
        ),
        fills = table(&["order id", "price", "qty", "executed"], fill_rows),
        audit = table(&["at", "event", "subject", "payload"], audit_rows),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;

    fn data() -> StatusPageData {
        StatusPageData {
            trading_mode: "paper".into(),
            broker: "MockBroker".into(),
            database_path: "/data/lossfunction.db".into(),
            now_utc: "2026-09-15 00:00:00 UTC".into(),
            ..Default::default()
        }
    }

    #[test]
    fn skeleton_and_sections() {
        let page = render_status_page(&data());
        for marker in [
            "<!doctype html>",
            "LossFunction",
            "MockBroker",
            "/data/lossfunction.db",
            "Positions",
            "Recent orders",
            "Recent fills",
            "Recent audit events",
            "(no rows)",
            "http-equiv=\"refresh\" content=\"5\"",
            "/healthz",
        ] {
            assert!(page.contains(marker), "missing: {marker}");
        }
    }

    #[test]
    fn db_values_are_escaped() {
        let page = render_status_page(&StatusPageData {
            positions: vec![StoredPosition {
                symbol: "<script>alert(1)</script>".into(),
                quantity: 1,
                average_price: Decimal::from(1),
            }],
            audit: vec![AuditRow {
                event_type: "<b>evil</b>".into(),
                subject: Some("x&y".into()),
                payload: serde_json::json!({}),
                occurred_at: Utc::now(),
            }],
            ..data()
        });
        assert!(!page.contains("<script>alert(1)"));
        assert!(page.contains("&lt;script&gt;"));
        assert!(!page.contains("<b>evil</b>"));
        assert!(page.contains("x&amp;y"));
    }

    #[test]
    fn kill_badge_reflects_state() {
        let mut payload = data();
        payload.kill_switch = true;
        payload.kill_reason = Some("manual halt".into());
        let page = render_status_page(&payload);
        assert!(page.contains("KILL SWITCH ON"));
        assert!(page.contains("manual halt"));
    }
}
