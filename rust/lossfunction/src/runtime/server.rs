//! axum HTTP server: health JSON, status page, and the kill-switch control.
//!
//! Controls share the page listener — loopback-only by deployment default;
//! do not expose the port publicly.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;

use crate::risk::RiskManager;
use crate::runtime::backfill::{spawn_backfill, BackfillCredentials, BackfillStatus};
use crate::storage::Repository;
use crate::types::Symbol;

/// Shared application state for the HTTP handlers.
pub struct AppState {
    pub trading_mode: String,
    pub broker: String,
    pub database_path: String,
    pub risk: Arc<RiskManager>,
    pub repository: Repository,
    pub started: Instant,
    /// Live backfill state shown on the status page.
    pub backfill: Arc<std::sync::Mutex<BackfillStatus>>,
    /// Present only when credentials reached this process (env/vault
    /// upstream); the web layer never fetches secrets itself.
    pub backfill_creds: Option<BackfillCredentials>,
    pub watchlist: Vec<Symbol>,
    /// "name vN" — what is deciding right now.
    pub strategy_label: String,
    /// Live backtest-run state shown on the status page.
    pub backtest: Arc<std::sync::Mutex<BackfillStatus>>,
    /// Screener run state shown on the watchlist page.
    pub screen: Arc<std::sync::Mutex<BackfillStatus>>,
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(overview_page))
        .route("/positions", get(positions_page))
        .route("/lab", get(lab_page))
        .route("/data", get(data_page))
        .route("/audit", get(audit_page))
        .route("/healthz", get(health))
        .route("/control/kill-switch", post(control_kill_switch))
        .route("/control/backfill", post(control_backfill))
        .route("/strategies", get(strategies))
        .route("/symbol/{code}", get(symbol_page))
        .route("/control/backtest", post(control_backtest))
        .route("/watchlist", get(watchlist_page))
        .route("/control/watchlist", post(control_watchlist))
        .route("/control/screener", post(control_screener))
        .route(
            "/control/generate-strategy",
            post(control_generate_strategy),
        )
        .with_state(state)
}

async fn health(State(state): State<SharedState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "lossfunction-runtime",
        "trading_mode": state.trading_mode,
        "broker": state.broker,
        "kill_switch": state.risk.kill_switch_active(),
        "kill_reason": state.risk.kill_reason(),
        "uptime_seconds": state.started.elapsed().as_secs(),
    }))
}

async fn page_data(state: &SharedState) -> crate::runtime::web::StatusPageData {
    crate::runtime::web::StatusPageData {
        trading_mode: state.trading_mode.clone(),
        broker: state.broker.clone(),
        database_path: state.database_path.clone(),
        kill_switch: state.risk.kill_switch_active(),
        kill_reason: state.risk.kill_reason(),
        now_utc: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        positions: state.repository.positions().await.unwrap_or_default(),
        latest_prices: state
            .repository
            .latest_quotes()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(symbol, price)| (symbol.to_string(), price))
            .collect(),
        orders: state.repository.recent_orders(50).await.unwrap_or_default(),
        fills: state.repository.recent_fills(50).await.unwrap_or_default(),
        audit: state.repository.recent_audit(50).await.unwrap_or_default(),
        candle_dates: state
            .repository
            .latest_candle_dates()
            .await
            .unwrap_or_default(),
        sparklines: {
            let watch = state
                .repository
                .watchlist()
                .await
                .unwrap_or_else(|_| state.watchlist.clone());
            let mut series = Vec::with_capacity(watch.len());
            for symbol in &watch {
                let prices = state
                    .repository
                    .quote_history(symbol, 60)
                    .await
                    .unwrap_or_default();
                series.push((
                    symbol.as_str().to_string(),
                    prices
                        .iter()
                        .map(|price| price.mantissa() as i64)
                        .collect::<Vec<_>>(),
                ));
            }
            series
        },
        strategy_label: state.strategy_label.clone(),
        uptime_seconds: state.started.elapsed().as_secs(),
        strategies: crate::strategy_registry::registry()
            .into_iter()
            .map(|spec| {
                (
                    spec.key.to_string(),
                    spec.name.to_string(),
                    spec.description.to_string(),
                    spec.params.to_string(),
                    spec.generated,
                )
            })
            .collect(),
        backfill: state.backfill.lock().expect("backfill status lock").clone(),
        backtest: state.backtest.lock().expect("backtest status lock").clone(),
        screen: state.screen.lock().expect("screen status lock").clone(),
    }
}

async fn strategies() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "strategies": crate::strategy_registry::registry()
            .into_iter()
            .map(|spec| serde_json::json!({
                "key": spec.key,
                "name": spec.name,
                "description": spec.description,
                "params": spec.params,
            }))
            .collect::<Vec<_>>(),
    }))
}

/// Per-symbol view: name, latest price, a daily-close chart from candles,
/// and recent bars. Unknown (unparseable) codes 404.
async fn symbol_page(
    State(state): State<SharedState>,
    axum::extract::Path(code): axum::extract::Path<String>,
) -> Result<Html<String>, StatusCode> {
    let symbol = Symbol::parse(code).map_err(|_| StatusCode::NOT_FOUND)?;
    let candles = state
        .repository
        .daily_candles(&symbol, crate::marketdata::Timeframe::Day)
        .await
        .unwrap_or_default();
    let latest = state
        .repository
        .latest_quotes()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|(quote_symbol, _)| quote_symbol == &symbol)
        .map(|(_, price)| price);
    Ok(Html(crate::runtime::web::render_symbol_page(
        &symbol, &candles, latest,
    )))
}

async fn overview_page(State(state): State<SharedState>) -> Html<String> {
    let data = page_data(&state).await;
    Html(crate::runtime::web::render_overview(&data))
}

async fn positions_page(State(state): State<SharedState>) -> Html<String> {
    let data = page_data(&state).await;
    Html(crate::runtime::web::render_positions_page(&data))
}

async fn lab_page(State(state): State<SharedState>) -> Html<String> {
    let data = page_data(&state).await;
    Html(crate::runtime::web::render_lab_page(&data))
}

async fn data_page(State(state): State<SharedState>) -> Html<String> {
    let data = page_data(&state).await;
    Html(crate::runtime::web::render_data_page(&data))
}

async fn audit_page(State(state): State<SharedState>) -> Html<String> {
    let data = page_data(&state).await;
    Html(crate::runtime::web::render_audit_page(&data))
}

#[derive(Debug, Deserialize)]
pub struct BacktestCommand {
    pub strategy: String,
    pub symbols: Option<Vec<String>>,
    pub years: Option<i64>,
    /// Optional BacktestConfig overrides (백테스트 설정 UI).
    pub config: Option<BacktestConfigOverrides>,
}

#[derive(Debug, Deserialize)]
pub struct BacktestConfigOverrides {
    pub initial_cash: Option<i64>,
    /// Percent, e.g. 0.015 means 0.015% — converted to a rate.
    pub commission_pct: Option<rust_decimal::Decimal>,
    pub tax_pct: Option<rust_decimal::Decimal>,
    pub history_capacity: Option<usize>,
}

impl BacktestConfigOverrides {
    /// Apply with range validation; a violation refuses the request.
    fn apply(
        &self,
        mut config: crate::backtest::BacktestConfig,
    ) -> Result<crate::backtest::BacktestConfig, &'static str> {
        use rust_decimal::Decimal;
        let hundred = Decimal::from(100);
        if let Some(cash) = self.initial_cash {
            if !(1_000_000..=100_000_000_000).contains(&cash) {
                return Err("initial_cash out of range (1,000,000 .. 100,000,000,000)");
            }
            config.initial_cash = Decimal::from(cash);
        }
        if let Some(pct) = self.commission_pct {
            if !(Decimal::ZERO..=Decimal::from(1)).contains(&pct) {
                return Err("commission_pct out of range (0 .. 1)");
            }
            config.commission_rate = pct / hundred;
        }
        if let Some(pct) = self.tax_pct {
            if !(Decimal::ZERO..=Decimal::from(1)).contains(&pct) {
                return Err("tax_pct out of range (0 .. 1)");
            }
            config.tax_rate = pct / hundred;
        }
        if let Some(capacity) = self.history_capacity {
            if !(2..=1000).contains(&capacity) {
                return Err("history_capacity out of range (2 .. 1000)");
            }
            config.history_capacity = capacity;
        }
        Ok(config)
    }
}

/// Kick off a background backtest over stored candles. Offline by
/// construction (mock broker + candles); concurrent runs refused (409).
async fn control_backtest(
    State(state): State<SharedState>,
    Json(command): Json<BacktestCommand>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let years = command.years.filter(|y| (1..=30).contains(y)).unwrap_or(5);
    if crate::strategy_registry::find(&command.strategy).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let symbols: Vec<Symbol> = match command.symbols {
        Some(list) if !list.is_empty() => list
            .into_iter()
            .map(Symbol::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| StatusCode::BAD_REQUEST)?,
        _ => state
            .repository
            .watchlist()
            .await
            .unwrap_or_else(|_| state.watchlist.clone()),
    };
    {
        let status = state.backtest.lock().expect("backtest status lock");
        if matches!(&*status, BackfillStatus::Running) {
            return Err(StatusCode::CONFLICT);
        }
    }
    let config = match &command.config {
        Some(overrides) => overrides
            .apply(crate::backtest::BacktestConfig::default())
            .map_err(|reason| {
                let at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
                *state.backtest.lock().expect("backtest status lock") = BackfillStatus::Failed {
                    at,
                    reason: reason.to_string(),
                };
                StatusCode::BAD_REQUEST
            })?,
        None => crate::backtest::BacktestConfig::default(),
    };
    state
        .repository
        .record_event(
            "control.backtest",
            "operator",
            &json!({ "action": "start", "strategy": command.strategy, "years": years, "source": "web" }),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::runtime::backtest_runner::spawn_backtest(
        state.repository.clone(),
        command.strategy,
        symbols,
        years,
        config,
        Arc::clone(&state.backtest),
    );
    Ok(Json(json!({ "ok": true, "status": "running" })))
}

async fn watchlist_page(State(state): State<SharedState>) -> Html<String> {
    let mut data = page_data(&state).await;
    data.strategies = Vec::new(); // not shown on this screen
    Html(crate::runtime::web::render_watchlist_page(&data))
}

/// Liquidity screener: KIS 거래금액순 상위로 워치리스트를 갱신한다.
/// 위험 종목(투자위험/관리/정리매매/불성실/거래정지/SPAC)은 요청 자체가 제외.
async fn control_screener(
    State(state): State<SharedState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let Some(credentials) = state.backfill_creds.clone() else {
        return Err(StatusCode::PRECONDITION_FAILED);
    };
    {
        let status = state.screen.lock().expect("screen status lock");
        if matches!(&*status, BackfillStatus::Running) {
            return Err(StatusCode::CONFLICT);
        }
    }
    let repository = state.repository.clone();
    let status = Arc::clone(&state.screen);
    *status.lock().expect("screen status lock") = BackfillStatus::Running;
    tokio::spawn(async move {
        let at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        let result = run_screen(&credentials, &repository, 20).await;
        let (next, payload) = match result {
            Ok(summary) => (
                BackfillStatus::Done {
                    at,
                    summary: summary.clone(),
                },
                serde_json::json!({"outcome": "done", "summary": summary}),
            ),
            Err(reason) => (
                BackfillStatus::Failed {
                    at,
                    reason: reason.clone(),
                },
                serde_json::json!({"outcome": "failed", "reason": reason}),
            ),
        };
        let _ = repository
            .record_event("control.screener", "operator", &payload)
            .await;
        *status.lock().expect("screen status lock") = next;
    });
    Ok(Json(json!({ "ok": true, "status": "running" })))
}

/// One screening pass — read-only on the venue (ranking quotation only).
async fn run_screen(
    credentials: &crate::runtime::backfill::BackfillCredentials,
    repository: &Repository,
    top: usize,
) -> Result<String, String> {
    let base_url = credentials
        .base_url_override
        .clone()
        .unwrap_or_else(|| crate::kis::auth::kis_base_url(&credentials.environment).to_string());
    let http = reqwest::Client::new();
    let auth = crate::kis::auth::KisAuth::new(
        base_url.clone(),
        credentials.app_key.clone(),
        credentials.app_secret.clone(),
        http.clone(),
    );
    let rest = crate::kis::rest::KisRestClient::new(
        auth,
        &credentials.account,
        &credentials.environment,
        base_url,
        http,
        "screener",
    )
    .map_err(|error| format!("클라이언트 구성 실패: {error}"))?;
    let ranked = crate::kis::screener::volume_rank_top(&rest, top).await?;

    let current = repository.watchlist().await.map_err(|e| e.to_string())?;
    let mut dropped = 0usize;
    for symbol in &current {
        if !ranked.iter().any(|r| r.code == symbol.as_str())
            && repository
                .remove_from_watchlist(symbol)
                .await
                .unwrap_or(false)
        {
            dropped += 1;
        }
    }
    let mut added = 0usize;
    for entry in &ranked {
        if let Ok(symbol) = Symbol::parse(entry.code.clone()) {
            if repository
                .add_to_watchlist(&symbol, "screener")
                .await
                .unwrap_or(false)
            {
                added += 1;
            }
        }
    }
    let total = repository
        .watchlist()
        .await
        .map_err(|e| e.to_string())?
        .len();
    Ok(format!(
        "거래금액순 상위 {top}종목 — 신규 {added}, 제외 {dropped}, 총 {total}종목 (위험종목은 조회 단계에서 제외)"
    ))
}

#[derive(Debug, Deserialize)]
pub struct WatchlistCommand {
    /// "add" | "remove"
    pub action: String,
    pub symbol: String,
}

/// Edit the persistent watchlist. Same interface for humans and the
/// screener: every change is audited with its source.
async fn control_watchlist(
    State(state): State<SharedState>,
    Json(command): Json<WatchlistCommand>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let symbol = Symbol::parse(command.symbol.trim()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let source = "web";
    let changed = match command.action.as_str() {
        "add" => state.repository.add_to_watchlist(&symbol, source).await,
        "remove" => state.repository.remove_from_watchlist(&symbol).await,
        _ => return Err(StatusCode::BAD_REQUEST),
    }
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .repository
        .record_event(
            "control.watchlist",
            "operator",
            &json!({ "action": command.action, "symbol": symbol.as_str(),
                     "changed": changed, "source": source }),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "ok": true, "changed": changed })))
}

#[derive(Debug, Deserialize)]
pub struct GenerateStrategyCommand {
    pub description: String,
}

/// Natural-language strategy generation: GLM writes the Rust file, the
/// compile gate decides whether it lands. Restarts activate successes.
async fn control_generate_strategy(
    State(state): State<SharedState>,
    Json(command): Json<GenerateStrategyCommand>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let description: String = command.description.trim().chars().take(2000).collect();
    if description.chars().count() < 6 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let Ok(api_key) = std::env::var("GLM_API_KEY") else {
        return Err(StatusCode::PRECONDITION_FAILED); // 412: key not injected
    };
    let base_url = std::env::var("GLM_BASE_URL")
        .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".to_string());
    let model = std::env::var("GLM_MODEL").unwrap_or_else(|_| "glm-4-flash".to_string());
    let client = crate::analysis::GlmClient::new(api_key, base_url, model, reqwest::Client::new());

    let outcome = crate::runtime::strategy_codegen::generate_code(&client, &description).await;
    let result = match outcome {
        Ok(code) => match crate::runtime::strategy_codegen::write_and_compile_gate(&code) {
            Ok(()) => serde_json::json!({
                "ok": true,
                "code": code,
                "note": "컴파일 게이트 통과 — 프로세스 재시작 후 실험실에서 활성화됩니다 (미검증·생성 표시)"
            }),
            Err(compile_error) => {
                serde_json::json!({ "ok": false, "code": code, "error": compile_error })
            }
        },
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    };
    let _ = state
        .repository
        .record_event(
            "control.generate_strategy",
            "operator",
            &serde_json::json!({
                "ok": result["ok"],
                "description": description,
            }),
        )
        .await;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct KillSwitchCommand {
    pub activate: bool,
    pub reason: Option<String>,
}

async fn control_kill_switch(
    State(state): State<SharedState>,
    Json(command): Json<KillSwitchCommand>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let reason: String = command
        .reason
        .unwrap_or_else(|| "manual (web)".to_string())
        .chars()
        .take(200)
        .collect();
    if command.activate {
        state.risk.activate_kill_switch(reason.clone());
    } else {
        state.risk.deactivate_kill_switch();
    }
    state
        .repository
        .record_event(
            "control.kill_switch",
            "operator",
            &json!({
                "activate": command.activate,
                "reason": reason,
                "source": "web",
            }),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": true,
        "kill_switch": state.risk.kill_switch_active(),
        "reason": state.risk.kill_reason(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct BackfillCommand {
    pub years: Option<i64>,
}

/// Kick off a background daily-bar backfill. Read-only on the venue and
/// idempotent on storage; the only hazard is concurrent runs, so a request
/// while Running is refused (409). Shares the kill-switch control posture:
/// loopback-only by deployment, every action audited.
async fn control_backfill(
    State(state): State<SharedState>,
    Json(command): Json<BackfillCommand>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let years = command
        .years
        .filter(|years| (1..=50).contains(years))
        .unwrap_or(5);
    let Some(credentials) = state.backfill_creds.clone() else {
        let at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        *state.backfill.lock().expect("backfill status lock") = BackfillStatus::Failed {
            at,
            reason: "KIS 자격증명이 이 프로세스에 없음 (KIS_APP_KEY/KIS_APP_SECRET)".to_string(),
        };
        return Err(StatusCode::BAD_REQUEST);
    };
    {
        let status = state.backfill.lock().expect("backfill status lock");
        if matches!(&*status, BackfillStatus::Running) {
            return Err(StatusCode::CONFLICT);
        }
    }
    state
        .repository
        .record_event(
            "control.backfill",
            "operator",
            &json!({ "action": "start", "years": years, "source": "web" }),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    spawn_backfill(
        credentials,
        state.repository.clone(),
        state.watchlist.clone(),
        years,
        Arc::clone(&state.backfill),
    );
    Ok(Json(
        json!({ "ok": true, "status": "running", "years": years }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use chrono::TimeZone;
    use tower::ServiceExt;

    async fn state(tmp: &std::path::Path) -> AppState {
        let repository = Repository::open(tmp.join("server.db").to_str().unwrap())
            .await
            .unwrap();
        AppState {
            trading_mode: "paper".into(),
            broker: "MockBroker".into(),
            database_path: tmp.join("server.db").display().to_string(),
            risk: Arc::new(RiskManager::new(crate::risk::RiskLimits {
                max_order_notional: rust_decimal::Decimal::from(1_000_000),
                max_position_quantity: 10,
                max_gross_exposure: rust_decimal::Decimal::from(10_000_000),
                daily_loss_limit: rust_decimal::Decimal::from(100_000),
                stale_quote_max_age: chrono::TimeDelta::seconds(30),
            })),
            repository,
            started: Instant::now(),
            backfill: Arc::new(std::sync::Mutex::new(BackfillStatus::Idle)),
            backfill_creds: None,
            watchlist: vec![Symbol::parse("005930").unwrap()],
            strategy_label: "EntryPriceStrategy v1".into(),
            backtest: Arc::new(std::sync::Mutex::new(BackfillStatus::Idle)),
            screen: Arc::new(std::sync::Mutex::new(BackfillStatus::Idle)),
        }
    }

    async fn body_text(body: Body) -> String {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn kill_switch_toggle_health_page_and_audit() {
        let tmp = tempfile::tempdir().unwrap();
        let app = router(Arc::new(state(tmp.path()).await));

        // Initial state: off.
        let response = app
            .clone()
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let health: serde_json::Value =
            serde_json::from_str(&body_text(response.into_body()).await).unwrap();
        assert_eq!(health["kill_switch"], serde_json::json!(false));

        // Activate.
        let request = Request::post("/control/kill-switch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"activate": true, "reason": "integration test"}).to_string(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&body_text(response.into_body()).await).unwrap();
        assert_eq!(result["kill_switch"], serde_json::json!(true));
        assert_eq!(result["reason"], serde_json::json!("integration test"));

        // Health reflects it.
        let response = app
            .clone()
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let health: serde_json::Value =
            serde_json::from_str(&body_text(response.into_body()).await).unwrap();
        assert_eq!(health["kill_switch"], serde_json::json!(true));

        // Page shows the badge with the reason.
        let response = app
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let page = body_text(response.into_body()).await;
        assert!(page.contains("KILL ·"));
        assert!(page.contains("integration test"));

        // The action is audited.
        let shared = state(tmp.path()).await; // same db file
        let events = shared.repository.recent_audit(10).await.unwrap();
        assert_eq!(events.last().unwrap().event_type, "control.kill_switch");
        assert_eq!(
            events.last().unwrap().payload["activate"],
            serde_json::json!(true)
        );

        // Release.
        let request = Request::post("/control/kill-switch")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"activate": false}).to_string(),
            ))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&body_text(response.into_body()).await).unwrap();
        assert_eq!(result["kill_switch"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn unknown_routes_404() {
        let tmp = tempfile::tempdir().unwrap();
        let app = router(Arc::new(state(tmp.path()).await));
        let response = app
            .oneshot(Request::get("/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    async fn backfill_post(app: axum::Router, years: i64) -> (axum::http::StatusCode, String) {
        let response = app
            .oneshot(
                Request::post("/control/backfill")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "years": years }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let code = response.status();
        (code, body_text(response.into_body()).await)
    }

    async fn wait_settled(status: &Arc<std::sync::Mutex<BackfillStatus>>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let settled = !matches!(
                *status.lock().unwrap(),
                BackfillStatus::Running | BackfillStatus::Idle
            );
            if settled {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "backfill never settled"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// No credentials in the process -> 400 and a page-visible failure state.
    #[tokio::test]
    async fn backfill_without_credentials_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let app = router(Arc::new(state(tmp.path()).await));
        let (code, _) = backfill_post(app, 5).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    /// A run in progress refuses concurrent runs; a failing venue settles
    /// into a page-visible Failed state with an audit trail.
    #[tokio::test]
    async fn backfill_duplicate_refused_and_failure_settles() {
        let tmp = tempfile::tempdir().unwrap();
        let mut raw = state(tmp.path()).await;
        // Dead local port: fails offline, fast.
        raw.backfill_creds = Some(BackfillCredentials {
            environment: "real".into(),
            app_key: "k".into(),
            app_secret: "s".into(),
            account: "12345678-01".into(),
            base_url_override: Some("http://127.0.0.1:9".into()),
        });
        let shared: SharedState = Arc::new(raw);
        let app = router(Arc::clone(&shared));
        let viewer = app.clone();

        let (first, _) = backfill_post(app.clone(), 1).await;
        assert_eq!(first, StatusCode::OK); // accepted, running
        let (second, _) = backfill_post(app, 1).await;
        assert_eq!(second, StatusCode::CONFLICT); // duplicate refused

        wait_settled(&shared.backfill).await;
        assert!(matches!(
            *shared.backfill.lock().unwrap(),
            BackfillStatus::Failed { .. }
        ));

        // The data screen shows the backfill failure.
        let response = viewer
            .oneshot(Request::get("/data").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let page = body_text(response.into_body()).await;
        assert!(page.contains("일봉"), "data screen missing: {page}");
        assert!(page.contains("실패"), "failure state not visible: {page}");

        let events = shared.repository.recent_audit(10).await.unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "control.backfill"));
    }

    #[tokio::test]
    async fn strategies_endpoint_and_page_section() {
        let tmp = tempfile::tempdir().unwrap();
        let app = router(Arc::new(state(tmp.path()).await));

        let response = app
            .clone()
            .oneshot(Request::get("/strategies").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response.into_body()).await;
        assert!(body.contains("sma-cross"));
        assert!(body.contains("momentum-rotation"));

        let response = app
            .oneshot(Request::get("/lab").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let page = body_text(response.into_body()).await;
        assert!(page.contains("전략 목록"));
        assert!(page.contains("MACD 교차"));
    }

    /// Backtest control: seeded candles -> background run settles Done with
    /// the metrics summary; bad keys/symbols are refused.
    #[tokio::test]
    async fn backtest_control_runs_over_seeded_candles() {
        let tmp = tempfile::tempdir().unwrap();
        let shared: SharedState = Arc::new(state(tmp.path()).await);
        // Seed 80 rising bars — enough history (min 60) for sma-cross.
        let mut seeded = Vec::new();
        for i in 0..80i64 {
            let day =
                chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + chrono::Duration::days(i);
            let ts = chrono::Utc.from_utc_datetime(&day.and_hms_opt(6, 30, 0).unwrap());
            let price = rust_decimal::Decimal::from(80_000 + i * 50);
            seeded.push(crate::marketdata::bar(
                &Symbol::parse("005930").unwrap(),
                ts,
                price,
                price,
                price,
                price,
                1_000,
            ));
        }
        let repository = shared.repository.clone();
        repository.migrate().await.unwrap();
        repository.upsert_candles(&seeded).await.unwrap();

        let app = router(Arc::clone(&shared));
        let post = |body: String| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::post("/control/backtest")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        // Unknown strategy -> 404; bad symbol -> 400.
        let response = post(serde_json::json!({"strategy": "ghost"}).to_string()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response =
            post(serde_json::json!({"strategy": "sma-cross", "symbols": ["nope"]}).to_string())
                .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Valid run accepted.
        let response =
            post(serde_json::json!({"strategy": "sma-cross", "years": 1}).to_string()).await;
        assert_eq!(response.status(), StatusCode::OK);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let settled = !matches!(
                *shared.backtest.lock().unwrap(),
                BackfillStatus::Running | BackfillStatus::Idle
            );
            if settled {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "backtest never settled"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        match &*shared.backtest.lock().unwrap() {
            BackfillStatus::Done { summary, .. } => {
                assert!(summary.contains("수익률"), "got: {summary}");
                assert!(summary.contains("MDD"), "got: {summary}");
            }
            other => panic!("expected Done, got {other:?}"),
        }
        let events = shared.repository.recent_audit(10).await.unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "control.backtest"));
    }

    /// BacktestConfig overrides: valid values change the run config,
    /// out-of-range values are refused with 400.
    #[tokio::test]
    async fn backtest_config_overrides_apply_and_validate() {
        use crate::backtest::BacktestConfig;
        use rust_decimal::Decimal;

        let defaults = BacktestConfig::default();
        let applied = BacktestConfigOverrides {
            initial_cash: Some(50_000_000),
            commission_pct: Some(Decimal::from_str_exact("0.02").unwrap()),
            tax_pct: Some(Decimal::ZERO), // ETF-style: no tax
            history_capacity: Some(30),
        }
        .apply(defaults)
        .unwrap();
        assert_eq!(applied.initial_cash, Decimal::from(50_000_000));
        assert_eq!(
            applied.commission_rate,
            Decimal::from_str_exact("0.0002").unwrap()
        );
        assert_eq!(applied.tax_rate, Decimal::ZERO);
        assert_eq!(applied.history_capacity, 30);

        assert!(
            BacktestConfigOverrides {
                initial_cash: Some(-1),
                commission_pct: None,
                tax_pct: None,
                history_capacity: None,
            }
            .apply(BacktestConfig::default())
            .is_err(),
            "negative cash must be refused"
        );
    }

    #[tokio::test]
    async fn symbol_page_renders_and_404s() {
        let tmp = tempfile::tempdir().unwrap();
        let shared: SharedState = Arc::new(state(tmp.path()).await);
        let repository = shared.repository.clone();
        repository.migrate().await.unwrap();
        let symbol = Symbol::parse("005930").unwrap();
        let mut seeded = Vec::new();
        for i in 0..30i64 {
            let day =
                chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Duration::days(i);
            let ts = chrono::Utc.from_utc_datetime(&day.and_hms_opt(6, 30, 0).unwrap());
            let price = crate::storage::int_to_money((80_000 + i * 100) * 10_000);
            seeded.push(crate::marketdata::bar(
                &symbol, ts, price, price, price, price, 1,
            ));
        }
        repository.upsert_candles(&seeded).await.unwrap();

        let app = router(shared);
        let response = app
            .clone()
            .oneshot(Request::get("/symbol/005930").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response.into_body()).await;
        assert!(body.contains("삼성전자"), "symbol name shown");
        assert!(body.contains("<svg"), "chart rendered");
        assert!(body.contains("80000"), "price rendered: {body}");

        let response = app
            .oneshot(Request::get("/symbol/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Generation control: 400 for too-short descriptions, 412 when the
    /// GLM key is not in the process environment.
    #[tokio::test]
    async fn generate_strategy_validates_input_and_key() {
        let tmp = tempfile::tempdir().unwrap();
        let app = router(Arc::new(state(tmp.path()).await));
        let post = |body: String| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::post("/control/generate-strategy")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        std::env::remove_var("GLM_API_KEY");
        let response = post(serde_json::json!({"description": "너무 짧다"}).to_string()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = post(
            serde_json::json!({"description": "RSI 30 이하 매수, 60 매도하는 전략"}).to_string(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    }

    /// Watchlist control: add/remove persist and appear; bad codes 400.
    #[tokio::test]
    async fn watchlist_control_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let shared: SharedState = Arc::new(state(tmp.path()).await);
        let repository = shared.repository.clone();
        repository.migrate().await.unwrap();
        repository
            .seed_watchlist(&[Symbol::parse("005930").unwrap()])
            .await
            .unwrap();
        let app = router(shared);
        let post = |body: String| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::post("/control/watchlist")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        let response =
            post(serde_json::json!({"action": "add", "symbol": "035720"}).to_string()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let response =
            post(serde_json::json!({"action": "add", "symbol": "nope"}).to_string()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response =
            post(serde_json::json!({"action": "remove", "symbol": "005930"}).to_string()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(repository.watchlist().await.unwrap().len(), 1);

        // The page renders the remaining symbol with remove buttons.
        let response = app
            .oneshot(Request::get("/watchlist").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let page = body_text(response.into_body()).await;
        assert!(page.contains("워치리스트"));
        assert!(page.contains("wlRemove"));
        assert!(page.contains("/symbol/035720"));
        let events = repository.recent_audit(10).await.unwrap();
        assert!(events.iter().any(|e| e.event_type == "control.watchlist"));
    }
}
