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
            let mut text = decimal.to_string();
            // Trim only fractional zeros ("80.0000" -> "80"); an integer
            // like 80000 must keep its zeros (was: "8").
            if text.contains('.') {
                while text.ends_with('0') {
                    text.pop();
                }
                if text.ends_with('.') {
                    text.pop();
                }
            }
            text
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
    // Every symbol display links to its per-symbol page.
    let inner = match SYMBOL_NAMES.iter().find(|(known, _)| *known == code) {
        Some((_, name)) => format!(r#"{} <small>{}</small>"#, esc(name), esc(code)),
        None => esc(code),
    };
    format!(r#"<a href="/symbol/{}">{inner}</a>"#, esc(code))
}

fn status_pill(status: &str) -> String {
    let known = ["filled", "cancelled", "rejected", "unknown"];
    let class = if known.contains(&status) { status } else { "" };
    format!(r#"<span class="pill {class}">{}</span>"#, esc(status))
}

const STYLE: &str = r#"
:root { --bg:#0d1017; --panel:#141924; --panel2:#181f2c; --line:#242c3b;
        --text:#dbe2ec; --dim:#7b8494; --accent:#6ea8ff; --up:#8fd694;
        --down:#ff9c9c; --warn:#e2b93b; }
* { box-sizing: border-box; }
body { font-family:-apple-system,'Segoe UI',Roboto,sans-serif; background:var(--bg);
       color:var(--text); margin:0; padding:0 0 48px; font-size:14px;
       -webkit-font-smoothing:antialiased; }
a { color:var(--accent); text-decoration:none; } a:hover { text-decoration:underline; }
.topbar { position:sticky; top:0; z-index:10; background:rgba(13,16,23,.92);
          backdrop-filter:blur(6px); border-bottom:1px solid var(--line);
          padding:0 28px; display:flex; align-items:center; gap:22px; height:52px; }
.brand { font-weight:700; font-size:15px; letter-spacing:.02em; }
.brand .mode { font-size:11px; padding:2px 8px; border-radius:4px; background:#234;
               color:#9fc3ff; vertical-align:middle; margin-left:8px; }
.brand .mode.live { background:#5a1f1f; color:#ff9c9c; }
.nav { display:flex; gap:4px; }
.nav a { color:var(--dim); padding:6px 12px; border-radius:6px; font-size:13px; }
.nav a.active { color:var(--text); background:var(--panel2); }
.nav a:hover { color:var(--text); text-decoration:none; }
.topbar .kill { margin-left:auto; font-size:12px; }
.kill button, .cardbtn { font-size:12px; padding:3px 12px; cursor:pointer;
  border-radius:6px; border:1px solid var(--line); background:var(--panel2);
  color:var(--text); }
.kill { font-size:12px; padding:2px 8px; border-radius:4px;
        background:#5a1f1f; color:#ff9c9c; }
main { max-width:1120px; margin:0 auto; padding:26px 28px 0; }
.meta { color:var(--dim); font-size:12.5px; margin:6px 0 0; }
h2 { font-size:13px; color:var(--dim); text-transform:uppercase; letter-spacing:.07em;
     margin:30px 0 10px; display:flex; align-items:center; gap:10px; }
h2::after { content:""; flex:1; height:1px; background:var(--line); }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(170px,1fr));
         gap:12px; margin:20px 0 6px; }
.card { background:var(--panel); border:1px solid var(--line); border-radius:10px;
        padding:14px 16px; transition:border-color .15s; }
.card:hover { border-color:#32405a; }
.card .k { color:var(--dim); font-size:11px; text-transform:uppercase;
           letter-spacing:.06em; margin-bottom:6px; }
.card .v { font-size:19px; font-weight:600; font-variant-numeric:tabular-nums; }
.card .v small { color:var(--dim); font-size:12px; font-weight:400; }
table { border-collapse:collapse; width:100%; font-size:13px; }
th, td { text-align:left; padding:8px 12px; border-bottom:1px solid var(--line); }
th { color:var(--dim); font-weight:500; font-size:12px; text-transform:uppercase;
     letter-spacing:.04em; }
tbody tr:hover { background:var(--panel2); }
td.num, th.num { text-align:right; font-variant-numeric:tabular-nums; }
.empty { color:#566070; font-style:italic; padding:8px 2px; }
.pill { padding:1px 8px; border-radius:10px; font-size:12px; background:var(--panel2); }
.pill.filled { color:var(--up); } .pill.cancelled { color:#c9a86a; }
.pill.rejected { color:var(--down); } .pill.unknown { color:var(--warn); }
.pnl.up { color:var(--up); } .pnl.down { color:var(--down); }
.side-buy { color:var(--up); } .side-sell { color:var(--down); }
.fresh-pill { padding:1px 8px; border-radius:10px; font-size:11px; }
.fresh-pill.fresh { background:#1d3325; color:var(--up); }
.fresh-pill.stale { background:#3a311d; color:var(--warn); }
.fresh-pill.old { background:#3a1d1d; color:var(--down); }
.controls { display:flex; flex-wrap:wrap; gap:8px; align-items:center;
            background:var(--panel); border:1px solid var(--line); border-radius:10px;
            padding:12px 14px; }
.controls label { color:var(--dim); font-size:12px; }
.controls input, .controls select { background:var(--bg); color:var(--text);
  border:1px solid var(--line); border-radius:6px; padding:6px 10px; font-size:13px; }
.controls input:focus, .controls select:focus { outline:1px solid var(--accent); }
.controls button { background:var(--accent); color:#0d1017; border:none; font-weight:600;
  border-radius:6px; padding:7px 18px; cursor:pointer; font-size:13px; }
.controls button:disabled { opacity:.45; cursor:default; }
.statusline { background:var(--panel); border:1px solid var(--line); border-radius:10px;
              padding:12px 14px; margin-top:10px; font-size:13px; }
footer { max-width:1120px; margin:34px auto 0; padding:0 28px; color:#566070;
         font-size:12px; }
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
    /// (key, name, description, params, generated) from the registry.
    pub strategies: Vec<(String, String, String, String, bool)>,
    pub backtest: BackfillStatus,
    pub screen: BackfillStatus,
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

/// Shared chrome: top bar with nav, brand, kill control. `active` picks
/// the highlighted tab.
fn layout(title: &str, active: &str, kill_badge: &str, content: &str, meta: &str) -> String {
    let tab = |key: &str, href: &str, label: &str| {
        let class = if key == active {
            " class=\"active\""
        } else {
            ""
        };
        format!(r#"<a href="{href}"{class}>{label}</a>"#)
    };
    format!(
        r##"<!doctype html>
<html lang="ko"><head><meta charset="utf-8">
<meta http-equiv="refresh" content="5">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · LossFunction</title>
<style>{STYLE}</style></head>
<body>
<header class="topbar">
  <span class="brand">LossFunction<span class="mode{mode_class}">{mode}</span></span>
  <nav class="nav">{nav_home}{nav_positions}{nav_lab}{nav_data}{nav_watchlist}{nav_audit}</nav>
  {kill_badge}
</header>
<main>
{meta}
{content}
</main>
<footer>JSON: <code>GET /healthz</code> · 제어: <code>POST /control/kill-switch</code> · <code>POST /control/backfill</code> · <code>POST /control/backtest</code> (loopback 전용)</footer>
<script>{SCRIPT}</script>
</body></html>"##,
        title = esc(title),
        mode = esc(&data_mode()),
        mode_class = if data_mode() == "live" { " live" } else { "" },
        nav_home = tab("overview", "/", "개요"),
        nav_positions = tab("positions", "/positions", "포지션·내역"),
        nav_lab = tab("lab", "/lab", "실험실"),
        nav_data = tab("data", "/data", "데이터"),
        nav_watchlist = tab("watchlist", "/watchlist", "워치리스트"),
        nav_audit = tab("audit", "/audit", "로그"),
        kill_badge = kill_badge,
        meta = meta,
        content = content,
    )
}

thread_local! {
    static PAGE_MODE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn set_page_mode(mode: &str) {
    PAGE_MODE.with(|cell| *cell.borrow_mut() = mode.to_string());
}

fn data_mode() -> String {
    PAGE_MODE.with(|cell| cell.borrow().clone())
}

const SCRIPT: &str = r#"
function toggleKill(on) {
  var msg = on ? 'kill switch를 켭니다. 새 주문이 차단됩니다.'
               : 'kill switch를 해제합니다.';
  if (!confirm(msg)) return;
  fetch('/control/kill-switch', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({activate: on, reason: 'manual (web)'})
  }).then(function () { location.reload(); });
}
function runBacktest() {
  var symbols = document.getElementById('bt-symbols').value.trim();
  var body = {
    strategy: document.getElementById('bt-strategy').value,
    years: parseInt(document.getElementById('bt-years').value, 10) || 5,
    config: {
      initial_cash: parseInt(document.getElementById('bt-cash').value, 10) || 100000000,
      commission_pct: parseFloat(document.getElementById('bt-commission').value),
      tax_pct: parseFloat(document.getElementById('bt-tax').value)
    }
  };
  if (symbols) body.symbols = symbols.split(',').map(function (s) { return s.trim(); });
  post('/control/backtest', body);
}
function runScreener() {
  if (!confirm('KIS 거래금액순 상위로 워치리스트를 갱신합니다 (위험 종목은 조회에서 제외).')) return;
  post('/control/screener', {});
}
function wlAdd() {
  var code = document.getElementById('wl-add').value.trim();
  if (!code) return;
  post('/control/watchlist', {action: 'add', symbol: code});
}
function wlRemove(code) {
  if (!confirm(code + '을(를) 워치리스트에서 제거합니다.')) return;
  post('/control/watchlist', {action: 'remove', symbol: code});
}
function generateStrategy() {{
  var desc = document.getElementById('nl-desc').value.trim();
  if (!desc) return;
  var out = document.getElementById('nl-result');
  out.textContent = '생성 중… (GLM 호출+컴파일 게이트, 수십 초)';
  document.querySelectorAll('button').forEach(function (b) {{ b.disabled = true; }});
  fetch('/control/generate-strategy', {{
    method: 'POST',
    headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify({{description: desc}})
  }}).then(function (r) {{ return r.json(); }}).then(function (res) {{
    out.textContent = res.ok
      ? res.note + '

' + res.code
      : '실패 — ' + (res.error || '') + (res.code ? '

생성 코드:
' + res.code : '');
    document.querySelectorAll('button').forEach(function (b) {{ b.disabled = false; }});
  }}).catch(function (e) {{
    out.textContent = '요청 실패: ' + e;
    document.querySelectorAll('button').forEach(function (b) {{ b.disabled = false; }});
  }});
}}
function runBackfill() {
  if (!confirm('일봉 백필을 시작합니다 (읽기 전용·수십 초).')) return;
  post('/control/backfill', {years: 5});
}
function post(url, body) {
  document.querySelectorAll('button').forEach(function (b) { b.disabled = true; });
  fetch(url, {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify(body)
  }).then(function () { setTimeout(function () { location.reload(); }, 400); });
}
"#;

/// Overview screen: the story in one glance.
pub fn render_overview(data: &StatusPageData) -> String {
    set_page_mode(&data.trading_mode);
    let prices: std::collections::HashMap<&str, &rust_decimal::Decimal> = data
        .latest_prices
        .iter()
        .map(|(symbol, price)| (symbol.as_str(), price))
        .collect();

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

    let position_rows = position_rows(data, &prices);
    let recent_fills: Vec<crate::storage::RecentFill> =
        data.fills.iter().take(5).cloned().collect();
    let fill_rows_short = fill_rows(&recent_fills);

    let content = format!(
        "{cards}<h2>보유 포지션</h2>{positions}<h2>최근 체결 (<a href=\"/positions\">전체 내역</a>)</h2>{fills}",
        cards = cards,
        positions = table(&["종목", "추이", "수량", "평단가", "현재가", "평가금액", "손익"], position_rows),
        fills = table(&["시각", "종목", "구분", "수량", "가격", "체결금액"], fill_rows_short),
    );
    layout(
        "개요",
        "overview",
        &kill_badge(data),
        &content,
        &format!(
            r#"<p class="meta">{db} · {now} · 새로고침 5초</p>"#,
            db = esc(&data.database_path),
            now = esc(&data.now_utc)
        ),
    )
}

/// 포지션·내역 화면.
pub fn render_positions_page(data: &StatusPageData) -> String {
    set_page_mode(&data.trading_mode);
    let prices: std::collections::HashMap<&str, &rust_decimal::Decimal> = data
        .latest_prices
        .iter()
        .map(|(symbol, price)| (symbol.as_str(), price))
        .collect();
    let content = format!(
        "<h2>보유 포지션</h2>{positions}<h2>주문 내역</h2>{orders}<h2>체결 내역</h2>{fills}",
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
            position_rows(data, &prices)
        ),
        orders = table(
            &["시각", "종목", "구분", "수량", "주문가", "상태"],
            order_rows(data)
        ),
        fills = table(
            &["시각", "종목", "구분", "수량", "가격", "체결금액"],
            fill_rows(&data.fills)
        ),
    );
    layout(
        "포지션·내역",
        "positions",
        &kill_badge(data),
        &content,
        &format!(r#"<p class="meta">{now}</p>"#, now = esc(&data.now_utc)),
    )
}

/// 실험실 화면: 백테스트와 전략 목록.
pub fn render_lab_page(data: &StatusPageData) -> String {
    set_page_mode(&data.trading_mode);
    let strategy_rows = data
        .strategies
        .iter()
        .map(|(key, name, description, params, generated)| {
            let marked = if *generated {
                format!(
                    r#"{} <span class="fresh-pill stale">미검증·생성</span>"#,
                    esc(name)
                )
            } else {
                esc(name)
            };
            vec![esc(key), marked, esc(description), esc(params)]
        })
        .collect();
    let strategy_options = data
        .strategies
        .iter()
        .map(|(key, name, _description, _params, _generated)| {
            format!(r#"<option value="{}">{}</option>"#, esc(key), esc(name))
        })
        .collect::<Vec<_>>()
        .join("");
    let bt_line = backtest_line(data);
    let content = format!(
        r#"<h2>백테스트</h2>
<div class="controls">
  <label>전략</label><select id="bt-strategy">{strategy_options}</select>
  <label>종목</label><input id="bt-symbols" placeholder="쉼표 구분 · 비우면 watchlist" size="30">
  <label>기간</label><input id="bt-years" type="number" value="5" min="1" max="30" size="3">년
  <label>초기자금</label><input id="bt-cash" type="number" value="100000000" min="1000000" size="10">원
  <label>수수료</label><input id="bt-commission" type="number" value="0.015" step="0.001" min="0" max="1" size="5">%
  <label>거래세</label><input id="bt-tax" type="number" value="0.15" step="0.01" min="0" max="1" size="5">%
  <button onclick="runBacktest()">실행</button>
</div>
<div class="statusline">{bt_line}</div>
<h2>전략 생성 (자연어 → 코드)</h2>
<div class="controls">
  <input id="nl-desc" placeholder="예: RSI가 30 이하로 떨어지면 매수, 60에 도달하면 매도" size="52">
  <button onclick="generateStrategy()">생성</button>
</div>
<div class="statusline" id="nl-result">설명을 쓰고 생성을 누르면 GLM이 Rust 코드를 만들고 컴파일 게이트를 통과한 것만 등록합니다.</div>
<h2>전략 목록</h2>
{strategies}"#,
        strategy_options = strategy_options,
        bt_line = esc(&bt_line),
        strategies = table(&["키", "이름", "설명", "기본 파라미터"], strategy_rows),
    );
    layout(
        "실험실",
        "lab",
        &kill_badge(data),
        &content,
        &format!(r#"<p class="meta">{now}</p>"#, now = esc(&data.now_utc)),
    )
}

/// 데이터 화면: 일봉 신선도와 백필.
pub fn render_data_page(data: &StatusPageData) -> String {
    set_page_mode(&data.trading_mode);
    let backfill_disabled = if matches!(data.backfill, BackfillStatus::Running) {
        " disabled"
    } else {
        ""
    };
    let content = format!(
        r#"<h2>일봉 (candles)</h2>
{candles}
<div class="controls" style="margin-top:12px">
  <button onclick="runBackfill()"{backfill_disabled}>일봉 갱신 (백필)</button>
  <span class="meta">{bf_line}</span>
</div>"#,
        candles = table(&["종목", "마지막 봉"], candle_rows(data)),
        backfill_disabled = backfill_disabled,
        bf_line = esc(&backfill_line(data)),
    );
    layout(
        "데이터",
        "data",
        &kill_badge(data),
        &content,
        &format!(
            r#"<p class="meta">{db} · {now}</p>"#,
            db = esc(&data.database_path),
            now = esc(&data.now_utc)
        ),
    )
}

/// 워치리스트 화면: 종목 선정의 중심 상태 — 이름·최신가·신선도·스파크라인,
/// 추가/제거 컨트롤. 스파크라인/최신가는 page_data가 watchlist 기준으로 채움.
pub fn render_watchlist_page(data: &StatusPageData) -> String {
    set_page_mode(&data.trading_mode);
    let prices: std::collections::HashMap<&str, &rust_decimal::Decimal> = data
        .latest_prices
        .iter()
        .map(|(symbol, price)| (symbol.as_str(), price))
        .collect();
    let fresh: std::collections::HashMap<&str, &str> = data
        .candle_dates
        .iter()
        .map(|(symbol, ts)| {
            let class = freshness_class(ts, &data.now_utc);
            let label = match class {
                "fresh" => "최신",
                "stale" => "갱신 필요",
                _ => "오래됨",
            };
            (symbol.as_str(), label)
        })
        .collect();

    let rows = data
        .sparklines
        .iter()
        .map(|(symbol, series)| {
            let code = symbol.as_str();
            let last = prices.get(code).copied();
            let rising = last
                .zip(series.last())
                .map(|(price, last_tick)| *price >= crate::storage::int_to_money(*last_tick))
                .unwrap_or(true);
            let freshness = fresh
                .get(code)
                .map(|label| {
                    let class = if *label == "최신" {
                        "fresh"
                    } else if *label == "갱신 필요" {
                        "stale"
                    } else {
                        "old"
                    };
                    format!(r#"<span class="fresh-pill {class}">{label}</span>"#)
                })
                .unwrap_or_else(|| r#"<span class="fresh-pill old">봉 없음</span>"#.to_string());
            vec![
                symbol_label(code),
                last.map(|p| money(&Some(*p)))
                    .unwrap_or_else(|| "—".to_string()),
                freshness,
                sparkline(series, rising),
                format!(r#"<button onclick="wlRemove('{code}')">제거</button>"#),
            ]
        })
        .collect();
    let content = format!(
        r#"<h2>워치리스트</h2>
{table}
<div class="controls" style="margin-top:12px">
  <input id="wl-add" placeholder="종목코드 6자리" size="12">
  <button onclick="wlAdd()">추가</button>
  <button onclick="runScreener()">스크리닝 갱신 (거래금액순)</button>
  <span class="meta">{screen_line}</span>
</div>"#,
        table = table(&["종목", "최신가", "일봉", "추이", ""], rows),
        screen_line = esc(&match &data.screen {
            BackfillStatus::Idle => "스크리너: 대기".to_string(),
            BackfillStatus::Running => "스크리너: 실행 중…".to_string(),
            BackfillStatus::Done { at, summary } => format!("스크리너 완료 {at} — {summary}"),
            BackfillStatus::Failed { at, reason } => format!("스크리너 실패 {at} — {reason}"),
        }),
    );
    layout(
        "워치리스트",
        "watchlist",
        &kill_badge(data),
        &content,
        &format!(
            r#"<p class="meta">{now} · 새로고침 5초</p>"#,
            now = esc(&data.now_utc)
        ),
    )
}

/// 감사 로그 화면.
pub fn render_audit_page(data: &StatusPageData) -> String {
    set_page_mode(&data.trading_mode);
    let audit_rows = data
        .audit
        .iter()
        .map(|event| {
            let payload = event.payload.to_string();
            vec![
                event.occurred_at.format("%m-%d %H:%M:%S").to_string(),
                esc(&event.event_type),
                esc(event.subject.as_deref().unwrap_or("")),
                esc(&payload.chars().take(160).collect::<String>()),
            ]
        })
        .collect();
    layout(
        "로그",
        "audit",
        &kill_badge(data),
        &table(&["시각", "이벤트", "주체", "내용"], audit_rows),
        &format!(r#"<p class="meta">{now}</p>"#, now = esc(&data.now_utc)),
    )
}

fn kill_badge(data: &StatusPageData) -> String {
    if data.kill_switch {
        let reason = data.kill_reason.as_deref().unwrap_or("");
        format!(
            r#"<span class="kill">KILL · {} <button onclick="toggleKill(false)">해제</button></span>"#,
            esc(reason)
        )
    } else {
        r#"<span class="kill"><button onclick="toggleKill(true)">KILL</button></span>"#.to_string()
    }
}

fn backtest_line(data: &StatusPageData) -> String {
    match &data.backtest {
        BackfillStatus::Idle => "백테스트: 대기".to_string(),
        BackfillStatus::Running => "백테스트: 실행 중…".to_string(),
        BackfillStatus::Done { at, summary } => format!("백테스트 완료 {at} — {summary}"),
        BackfillStatus::Failed { at, reason } => format!("백테스트 실패 {at} — {reason}"),
    }
}

fn backfill_line(data: &StatusPageData) -> String {
    match &data.backfill {
        BackfillStatus::Idle => "백필: 대기".to_string(),
        BackfillStatus::Running => "백필: 실행 중…".to_string(),
        BackfillStatus::Done { at, summary } => format!("백필 완료 {at} — {summary}"),
        BackfillStatus::Failed { at, reason } => format!("백필 실패 {at} — {reason}"),
    }
}

fn position_rows(
    data: &StatusPageData,
    prices: &std::collections::HashMap<&str, &rust_decimal::Decimal>,
) -> Vec<Vec<String>> {
    let spark_map: std::collections::HashMap<&str, &[i64]> = data
        .sparklines
        .iter()
        .map(|(symbol, series)| (symbol.as_str(), series.as_slice()))
        .collect();
    data.positions
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
        .collect()
}

fn order_rows(data: &StatusPageData) -> Vec<Vec<String>> {
    data.orders
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
        .collect()
}

fn fill_rows(fills: &[crate::storage::RecentFill]) -> Vec<Vec<String>> {
    fills
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
        .collect()
}

fn candle_rows(data: &StatusPageData) -> Vec<Vec<String>> {
    data.candle_dates
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
        .collect()
}

/// Per-symbol page: identity, latest price, a closes chart (inline SVG),
/// and the newest bars. Pure function of pre-fetched data.
pub fn render_symbol_page(
    symbol: &crate::types::Symbol,
    candles: &[crate::marketdata::Bar],
    latest: Option<rust_decimal::Decimal>,
) -> String {
    use crate::marketdata::Timeframe;
    let _ = Timeframe::Day;
    let name = SYMBOL_NAMES
        .iter()
        .find(|(known, _)| *known == symbol.as_str())
        .map(|(_, name)| *name)
        .unwrap_or("");
    let closes: Vec<i64> = candles
        .iter()
        .map(|bar| bar.close.mantissa() as i64)
        .collect();
    let chart = chart_svg(&closes, 640, 160);
    let stats = if candles.is_empty() {
        r#"<p class="empty">저장된 일봉이 없습니다 — 백필을 실행하세요.</p>"#.to_string()
    } else {
        let first = &candles[0];
        let last = candles.last().unwrap();
        let min = candles
            .iter()
            .map(|b| b.close)
            .fold(first.close, rust_decimal::Decimal::min);
        let max = candles
            .iter()
            .map(|b| b.close)
            .fold(first.close, rust_decimal::Decimal::max);
        format!(
            r#"<table><thead><tr><th>기간</th><th>시작가</th><th>최고</th><th>최저</th><th>마지막 종가</th><th>봉 수</th></tr></thead>
<tbody><tr><td>{} ~ {}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr></tbody></table>"#,
            first.timestamp.format("%Y-%m-%d"),
            last.timestamp.format("%Y-%m-%d"),
            money(&Some(first.close)),
            money(&Some(max)),
            money(&Some(min)),
            money(&Some(last.close)),
            candles.len(),
        )
    };
    let bars: String = candles
        .iter()
        .rev()
        .take(10)
        .map(|bar| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                bar.timestamp.format("%Y-%m-%d"),
                money(&Some(bar.open)),
                money(&Some(bar.high)),
                money(&Some(bar.low)),
                money(&Some(bar.close)),
                bar.volume
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let price_line = match latest {
        Some(price) => format!("현재가 {}원", money(&Some(price))),
        None => "현재가 —".to_string(),
    };
    format!(
        r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{code} · {name}</title>
<style>{STYLE}</style></head>
<body>
<h1>{name} <small>{code}</small></h1>
<p class="meta">{price_line} · <a href="/">← 전체 상태로</a></p>
{chart}
<h2>통계</h2>
{stats}
<h2>최근 봉</h2>
<table><thead><tr><th>날짜</th><th>시가</th><th>고가</th><th>저가</th><th>종가</th><th>거래량</th></tr></thead><tbody>{bars}</tbody></table>
</body></html>"#,
        code = esc(symbol.as_str()),
        name = esc(name),
        price_line = esc(&price_line),
        chart = chart,
        stats = stats,
        bars = bars,
    )
}

/// Larger line chart (min/max labeled) from 1e-4 ints.
fn chart_svg(points: &[i64], w: u32, h: u32) -> String {
    if points.len() < 2 {
        return r#"<p class="empty">(차트를 그릴 데이터가 없습니다)</p>"#.to_string();
    }
    let (min, max) = (*points.iter().min().unwrap(), *points.iter().max().unwrap());
    let span = (max - min).max(1) as f64;
    let pad = 8.0;
    let step = (w as f64 - 2.0 * pad) / (points.len() - 1) as f64;
    let coords = points
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let x = pad + i as f64 * step;
            let y = (h as f64 - pad) - ((value - min) as f64 / span) * (h as f64 - 2.0 * pad);
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let min_money = crate::storage::int_to_money(min);
    let max_money = crate::storage::int_to_money(max);
    format!(
        r##"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}"><polyline points="{coords}" fill="none" stroke="#6ea8ff" stroke-width="1.5"/><text x="4" y="12" fill="#7b8494" font-size="11">{}</text><text x="4" y="{}" fill="#7b8494" font-size="11">{}</text></svg>"##,
        money(&Some(max_money)),
        h - 4,
        money(&Some(min_money))
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
        set_page_mode("paper");
        let page = render_overview(&data());
        for marker in [
            "<!doctype html>",
            "LossFunction",
            "MockBroker",
            "/data/lossfunction.db",
            "미실현 손익",
            "보유 포지션",
            "최근 체결",
            "실험실",
            "데이터",
            "로그",
            "http-equiv=\"refresh\" content=\"5\"",
            "/healthz",
            "class=\"nav\"",
        ] {
            assert!(page.contains(marker), "missing: {marker}");
        }
    }

    #[test]
    fn db_values_are_escaped() {
        set_page_mode("paper");
        let page = render_overview(&StatusPageData {
            positions: vec![StoredPosition {
                symbol: "<script>alert(1)</script>".into(),
                quantity: 1,
                average_price: Decimal::from(1),
            }],
            ..data()
        });
        let audit_page = render_audit_page(&StatusPageData {
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
        assert!(!audit_page.contains("<b>evil</b>"));
        assert!(audit_page.contains("x&amp;y"));
    }

    #[test]
    fn kill_badge_reflects_state() {
        let mut payload = data();
        payload.kill_switch = true;
        payload.kill_reason = Some("manual halt".into());
        set_page_mode(&payload.trading_mode);
        let page = render_overview(&payload);
        assert!(page.contains("KILL ·"));
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
        set_page_mode(&payload.trading_mode);
        let page = render_data_page(&payload);
        assert!(page.contains("일봉 (candles)"));
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
        set_page_mode(&payload.trading_mode);
        let page = render_data_page(&payload);
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
        set_page_mode(&payload.trading_mode);
        let page = render_overview(&payload);
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
        set_page_mode(&payload.trading_mode);
        let overview = render_overview(&payload);
        assert!(
            overview.contains(r#"<a href="/symbol/005930">삼성전자 <small>005930</small></a>"#),
            "known name links to its page"
        );
        let data_page = render_data_page(&payload);
        assert!(
            data_page.contains(">999999<"),
            "unknown code falls back bare"
        );
    }

    /// Generated strategies carry the 미검증·생성 badge in the lab list.
    #[test]
    fn generated_strategies_are_badged() {
        set_page_mode("paper");
        let payload = StatusPageData {
            strategies: vec![(
                "rsi-dip-buy".into(),
                "RSI 저점 매수".into(),
                "설명".into(),
                "RSI 14·30/60".into(),
                true,
            )],
            ..data()
        };
        let page = render_lab_page(&payload);
        assert!(page.contains("미검증·생성"), "badge missing");
        assert!(page.contains("rsi-dip-buy"));
    }
}
