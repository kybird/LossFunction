//! Single-file HTML status page — the lightweight frontend.
//!
//! Server-side rendered from SQLite, framework-free, zero external assets.
//! Every DB-sourced string is HTML-escaped; the page is read-only except
//! for the kill-switch control served by the same listener.

use crate::runtime::backfill::BackfillStatus;
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

/// Thousands-grouped integer KRW ("12,345,678").
pub(crate) fn fmt_krw(value: &rust_decimal::Decimal) -> String {
    krw_int(value)
}

fn krw_int(value: &rust_decimal::Decimal) -> String {
    let rounded = value.round_dp(0);
    let text = rounded.abs().to_string();
    let grouped = text
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default()
        .join(",");
    if rounded.is_sign_negative() {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// Signed pnl span: green up / red down.
fn pnl_span(amount: &rust_decimal::Decimal) -> String {
    let (class, sign) = if amount.is_sign_negative() {
        ("down", "")
    } else {
        ("up", "+")
    };
    format!(
        r#"<span class="pnl {class}">{sign}{}원</span>"#,
        krw_int(&amount.abs())
    )
}

/// Inline-SVG sparkline from 1e-4 KRW ints — numeric-only, no escaping
/// surface. Flat series renders as a centered line.
fn sparkline(points: &[i64], up: bool) -> String {
    if points.len() < 2 {
        return r#"<span class="empty">—</span>"#.to_string();
    }
    let (min, max) = (*points.iter().min().unwrap(), *points.iter().max().unwrap());
    let (w, h, pad) = (120u32, 28u32, 2u32);
    let span = (max - min).max(1) as f64;
    let step = (w - 2 * pad) as f64 / (points.len() - 1) as f64;
    let coords = points
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let x = pad as f64 + i as f64 * step;
            let y = (h - pad) as f64 - ((value - min) as f64 / span) * (h - 2 * pad) as f64;
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let stroke = if up { "#8fd694" } else { "#ff9c9c" };
    format!(
        r#"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}"><polyline points="{coords}" fill="none" stroke="{stroke}" stroke-width="1.5"/></svg>"#
    )
}

/// Freshness class for a candle timestamp: green <=7d, amber <=30d, red
/// older. Day-field arithmetic — display guidance, not billing.
fn freshness_class(ts: &str, now: &str) -> &'static str {
    fn day_tuple(text: &str) -> Option<(i64, i64, i64)> {
        Some((
            text.get(0..4)?.parse().ok()?,
            text.get(5..7)?.parse().ok()?,
            text.get(8..10)?.parse().ok()?,
        ))
    }
    let (Some((ty, tm, td)), Some((ny, nm, nd))) = (day_tuple(ts), day_tuple(now)) else {
        return "old";
    };
    let approx_days = (ny - ty) * 365 + (nm - tm) * 30 + (nd - td);
    if approx_days <= 7 {
        "fresh"
    } else if approx_days <= 30 {
        "stale"
    } else {
        "old"
    }
}

/// Korean display names for common codes — a convenience label, never a
/// trading key. Unknown codes render as the bare code.
const SYMBOL_NAMES: &[(&str, &str)] = &[
    ("005930", "삼성전자"),
    ("000660", "SK하이닉스"),
    ("035420", "NAVER"),
    ("005380", "현대차"),
    ("068270", "셀트리온"),
    ("207940", "삼성바이오로직스"),
    ("000270", "기아"),
    ("051910", "LG화학"),
    ("006400", "삼성SDI"),
    ("012330", "현대모비스"),
    ("105560", "KB금융"),
    ("055550", "신한지주"),
    ("086790", "하나금융지주"),
    ("011200", "HMM"),
    ("028260", "삼성물산"),
    ("069500", "KODEX 200"),
    ("102110", "TIGER 미국S&P500"),
    ("379800", "KODEX 미국채10년"),
    ("091170", "KODEX 인버스"),
    ("293940", "KODEX 반도체"),
];

/// "삼성전자 <small>005930</small>" or the bare code — name passes esc().
fn symbol_label(code: &str) -> String {
    match SYMBOL_NAMES.iter().find(|(known, _)| *known == code) {
        Some((_, name)) => format!(r#"{} <small>{}</small>"#, esc(name), esc(code)),
        None => esc(code),
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
.cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
         gap: 10px; margin: 14px 0 4px; }
.card { background: #161a22; border: 1px solid #232833; border-radius: 8px;
        padding: 10px 14px; }
.card .k { color: #7b8494; font-size: 11px; text-transform: uppercase;
           letter-spacing: .05em; margin-bottom: 4px; }
.card .v { font-size: 17px; font-weight: 600; font-variant-numeric: tabular-nums; }
.card .v small { color: #7b8494; font-size: 12px; font-weight: 400; }
.pnl.up { color: #8fd694; } .pnl.down { color: #ff9c9c; }
.side-buy { color: #8fd694; } .side-sell { color: #ff9c9c; }
.fresh-pill { padding: 1px 7px; border-radius: 9px; font-size: 11px; }
.fresh-pill.fresh { background: #1d3325; color: #8fd694; }
.fresh-pill.stale { background: #3a311d; color: #e2b93b; }
.fresh-pill.old { background: #3a1d1d; color: #ff9c9c; }
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
    /// (symbol, newest candle ts) — data freshness.
    pub candle_dates: Vec<(String, String)>,
    pub backfill: BackfillStatus,
    /// (symbol, newest-last 1e-4 KRW ints) for inline-SVG sparklines.
    pub sparklines: Vec<(String, Vec<i64>)>,
    pub strategy_label: String,
    pub uptime_seconds: u64,
    /// (key, name, description, params) from the registry.
    pub strategies: Vec<(String, String, String, String)>,
    pub backtest: BackfillStatus,
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

    let spark_map: std::collections::HashMap<&str, &[i64]> = data
        .sparklines
        .iter()
        .map(|(symbol, series)| (symbol.as_str(), series.as_slice()))
        .collect();
    let position_rows = data
        .positions
        .iter()
        .map(|position| {
            let last = prices.get(position.symbol.as_str());
            let qty = rust_decimal::Decimal::from(position.quantity);
            let cost = qty * position.average_price;
            let (value, pnl) = match last {
                Some(price) => {
                    let value = qty * **price;
                    (value.to_string(), value - cost)
                }
                None => (cost.to_string(), rust_decimal::Decimal::ZERO),
            };
            let series = spark_map
                .get(position.symbol.as_str())
                .copied()
                .unwrap_or(&[]);
            let rising = last
                .map(|price| **price >= position.average_price)
                .unwrap_or(false);
            let pnl_pct = if cost.is_zero() {
                "0".to_string()
            } else {
                (pnl * rust_decimal::Decimal::from(100) / cost)
                    .round_dp(2)
                    .to_string()
            };
            vec![
                symbol_label(&position.symbol),
                sparkline(series, rising),
                position.quantity.to_string(),
                money(&Some(position.average_price)),
                last.map(|p| money(&Some(**p)))
                    .unwrap_or_else(|| "—".to_string()),
                value,
                format!("{} <small>({pnl_pct}%)</small>", pnl_span(&pnl)),
            ]
        })
        .collect();

    let order_rows = data
        .orders
        .iter()
        .map(|order| {
            let (side_label, side_class) = if order.side == "sell" {
                ("매도", "side-sell")
            } else {
                ("매수", "side-buy")
            };
            vec![
                order.created_at.format("%m-%d %H:%M:%S").to_string(),
                symbol_label(&order.symbol),
                format!(r#"<span class="{side_class}">{side_label}</span>"#),
                order.quantity.to_string(),
                money(&order.limit_price),
                status_pill(&order.status),
            ]
        })
        .collect();

    let fill_rows = data
        .fills
        .iter()
        .map(|fill| {
            let (side_label, side_class) = if fill.side == "sell" {
                ("매도", "side-sell")
            } else {
                ("매수", "side-buy")
            };
            vec![
                fill.executed_at.format("%m-%d %H:%M:%S").to_string(),
                symbol_label(&fill.symbol),
                format!(r#"<span class="{side_class}">{side_label}</span>"#),
                fill.quantity.to_string(),
                money(&Some(fill.price)),
                krw_int(&(fill.price * rust_decimal::Decimal::from(fill.quantity))),
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

    let strategy_rows = data
        .strategies
        .iter()
        .map(|(key, name, description, params)| {
            vec![esc(key), esc(name), esc(description), esc(params)]
        })
        .collect();

    let candle_rows = data
        .candle_dates
        .iter()
        .map(|(symbol, ts)| {
            let class = freshness_class(ts, &data.now_utc);
            let label = match class {
                "fresh" => "최신",
                "stale" => "갱신 필요",
                _ => "오래됨",
            };
            vec![
                symbol_label(symbol),
                format!(r#"{ts} <span class="fresh-pill {class}">{label}</span>"#),
            ]
        })
        .collect();

    let (backfill_line, backfill_disabled) = match &data.backfill {
        BackfillStatus::Idle => ("대기 — 아직 갱신 없음".to_string(), ""),
        BackfillStatus::Running => ("실행 중…".to_string(), " disabled"),
        BackfillStatus::Done { at, summary } => (format!("완료 {at} — {summary}"), ""),
        BackfillStatus::Failed { at, reason } => (format!("실패 {at} — {reason}"), ""),
    };

    // Summary: what is happening, in one row of cards.
    let (cost_total, value_total) = data.positions.iter().fold(
        (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO),
        |(cost, value), position| {
            let qty = rust_decimal::Decimal::from(position.quantity);
            let cost = cost + qty * position.average_price;
            let value = value
                + qty
                    * prices
                        .get(position.symbol.as_str())
                        .copied()
                        .unwrap_or(&position.average_price);
            (cost, value)
        },
    );
    let unrealized = value_total - cost_total;
    let unrealized_pct = if cost_total.is_zero() {
        rust_decimal::Decimal::ZERO
    } else {
        unrealized * rust_decimal::Decimal::from(100) / cost_total
    };
    let uptime = {
        let secs = data.uptime_seconds;
        if secs >= 3600 {
            format!("{}시간 {}분", secs / 3600, (secs % 3600) / 60)
        } else {
            format!("{}분", secs / 60)
        }
    };
    let cards = format!(
        r#"<div class="cards">
<div class="card"><div class="k">전략</div><div class="v">{strategy}</div></div>
<div class="card"><div class="k">모드 · 브로커</div><div class="v">{mode} <small>{broker}</small></div></div>
<div class="card"><div class="k">총 평가금액</div><div class="v">{value}원</div></div>
<div class="card"><div class="k">미실현 손익</div><div class="v">{pnl} <small>{pct}%</small></div></div>
<div class="card"><div class="k">보유 종목</div><div class="v">{count}</div></div>
<div class="card"><div class="k">가동</div><div class="v">{uptime}</div></div>
</div>"#,
        strategy = esc(&data.strategy_label),
        broker = esc(&data.broker),
        mode = esc(&data.trading_mode),
        value = krw_int(&value_total),
        pnl = pnl_span(&unrealized),
        pct = unrealized_pct.round_dp(2),
        count = data.positions.len(),
        uptime = esc(&uptime),
    );

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
<p class="meta">{db} · {now} · 새로고침 5초</p>
{cards}

<h2>보유 포지션</h2>
{positions}

<h2>주문 내역</h2>
{orders}

<h2>체결 내역</h2>
{fills}

<h2>전략 목록</h2>
{strategies}

<h2>백테스트</h2>
<p class="meta">상태: {backtest_line}</p>
<p class="meta">
<select id="bt-strategy">{strategy_options}</select>
<input id="bt-symbols" placeholder="종목코드 (쉼표 구분, 비우면 watchlist)" size="34">
<input id="bt-years" type="number" value="5" min="1" max="30" size="3">년
<button onclick="runBacktest()">실행</button></p>

<h2>데이터 (일봉)</h2>
{candles}
<p class="meta">일봉 백필: {backfill_line}
<button onclick="runBackfill()"{backfill_disabled}>일봉 갱신</button></p>

<h2>감사 로그</h2>
{audit}

<footer>JSON: <code>GET /healthz</code> · 제어: <code>POST /control/kill-switch</code>
· <code>POST /control/backfill</code> (loopback 전용)</footer>
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
function runBacktest() {{
  var symbols = document.getElementById('bt-symbols').value.trim();
  var body = {{
    strategy: document.getElementById('bt-strategy').value,
    years: parseInt(document.getElementById('bt-years').value, 10) || 5
  }};
  if (symbols) body.symbols = symbols.split(',').map(function (s) {{ return s.trim(); }});
  fetch('/control/backtest', {{
    method: 'POST',
    headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify(body)
  }}).then(function () {{ location.reload(); }});
}}
function runBackfill() {{
  if (!confirm('일봉 백필을 시작합니다 (읽기 전용·수십 초).')) return;
  fetch('/control/backfill', {{
    method: 'POST',
    headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify({{years: 5}})
  }}).then(function () {{ location.reload(); }});
}}
</script>
</body></html>"#,
        strategies = table(&["키", "이름", "설명", "기본 파라미터"], strategy_rows),
        strategy_options = data
            .strategies
            .iter()
            .map(|(key, name, _description, _params)| {
                format!(r#"<option value="{}">{}</option>"#, esc(key), esc(name))
            })
            .collect::<Vec<_>>()
            .join(""),
        backtest_line = esc(&match &data.backtest {
            BackfillStatus::Idle => "대기".to_string(),
            BackfillStatus::Running => "실행 중…".to_string(),
            BackfillStatus::Done { at, summary } => format!("완료 {at} — {summary}"),
            BackfillStatus::Failed { at, reason } => format!("실패 {at} — {reason}"),
        }),
        candles = table(&["종목", "마지막 봉"], candle_rows),
        backfill_line = esc(&backfill_line),
        backfill_disabled = backfill_disabled,
        mode = esc(&data.trading_mode),
        db = esc(&data.database_path),
        now = esc(&data.now_utc),
        cards = cards,
        positions = table(
            &[
                "종목",
                "추이",
                "수량",
                "평단가",
                "현재가",
                "평가금액",
                "손익"
            ],
            position_rows
        ),
        orders = table(
            &["시각", "종목", "구분", "수량", "주문가", "상태"],
            order_rows
        ),
        fills = table(
            &["시각", "종목", "구분", "수량", "가격", "체결금액"],
            fill_rows
        ),
        audit = table(&["시각", "이벤트", "주체", "내용"], audit_rows),
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
            "미실현 손익",
            "보유 포지션",
            "주문 내역",
            "체결 내역",
            "데이터 (일봉)",
            "감사 로그",
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

    #[test]
    fn data_section_renders_freshness_and_backfill_state() {
        let payload = StatusPageData {
            candle_dates: vec![("005930".into(), "2026-09-19T06:30:00+00:00".into())],
            backfill: BackfillStatus::Done {
                at: "2026-09-20 10:00:00 UTC".into(),
                summary: "3711 bars stored, 0/3 symbols failed".into(),
            },
            ..data()
        };
        let page = render_status_page(&payload);
        assert!(page.contains("데이터 (일봉)"));
        assert!(page.contains("005930"));
        assert!(page.contains("2026-09-19T06:30:00+00:00"));
        assert!(page.contains("3711 bars stored"));
        assert!(page.contains("runBackfill"));
    }

    /// Backfill summaries travel through esc() — failure reasons can embed
    /// venue messages, which are foreign input.
    #[test]
    fn backfill_reason_is_escaped() {
        let payload = StatusPageData {
            backfill: BackfillStatus::Failed {
                at: "2026-09-20 10:00:00 UTC".into(),
                reason: "<script>x</script>".into(),
            },
            ..data()
        };
        let page = render_status_page(&payload);
        assert!(!page.contains("<script>x"), "unescaped reason leaked");
        assert!(page.contains("&lt;script&gt;"));
    }

    /// AC: the page states what is happening — strategy, money, pnl, and a
    /// per-symbol sparkline from the quote series.
    #[test]
    fn summary_cards_state_the_story() {
        use rust_decimal::Decimal;
        let payload = StatusPageData {
            strategy_label: "EntryPriceStrategy v1".into(),
            uptime_seconds: 7_200,
            positions: vec![StoredPosition {
                symbol: "005930".into(),
                quantity: 10,
                average_price: Decimal::from(80_000),
            }],
            latest_prices: vec![("005930".into(), Decimal::from(90_000))],
            sparklines: vec![(
                "005930".into(),
                (80_000i64..80_060).map(|v| v * 10_000).collect(),
            )],
            ..data()
        };
        let page = render_status_page(&payload);
        assert!(page.contains("EntryPriceStrategy v1"), "strategy shown");
        assert!(page.contains("900,000원"), "total mark value 10x90,000");
        assert!(page.contains("+100,000원"), "unrealized pnl");
        assert!(page.contains("12.50%"), "pnl percentage");
        assert!(page.contains("<svg"), "sparkline rendered");
        assert!(page.contains("2시간 0분"), "uptime");
    }

    /// AC: fills read back with symbol and side for the 매수/매도 display.
    #[tokio::test]
    async fn fills_read_back_with_symbol_and_side() {
        let dir = tempfile::tempdir().unwrap();
        let repository =
            crate::storage::Repository::open(dir.path().join("fills.db").to_str().unwrap())
                .await
                .unwrap();
        repository.migrate().await.unwrap();

        use crate::types::{OrderSide, OrderType, Symbol};
        let order = crate::domain::order::Order::new(
            "ord-0001",
            Symbol::parse("005930").unwrap(),
            OrderSide::Buy,
            OrderType::Market,
            10,
            None,
        )
        .unwrap();
        repository.create_order(&order, "paper").await.unwrap();
        repository
            .record_fill(
                "ord-0001",
                &Symbol::parse("005930").unwrap(),
                OrderSide::Buy,
                10,
                rust_decimal::Decimal::from(80_000),
                chrono::Utc::now(),
            )
            .await
            .unwrap();

        let fills = repository.recent_fills(10).await.unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].symbol, "005930");
        assert_eq!(fills[0].side, "buy");
    }

    /// Known codes render "이름 + 코드"; unknown codes fall back bare.
    #[test]
    fn symbol_names_render_with_fallback() {
        let payload = StatusPageData {
            positions: vec![StoredPosition {
                symbol: "005930".into(),
                quantity: 1,
                average_price: Decimal::from(1),
            }],
            candle_dates: vec![("999999".into(), "2026-09-20T06:30:00+00:00".into())],
            ..data()
        };
        let page = render_status_page(&payload);
        assert!(
            page.contains("삼성전자 <small>005930</small>"),
            "known name shown"
        );
        assert!(page.contains(">999999<"), "unknown code falls back bare");
    }
}
