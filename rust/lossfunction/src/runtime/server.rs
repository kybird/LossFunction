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
use crate::runtime::web::{render_status_page, StatusPageData};
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
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(status_page))
        .route("/healthz", get(health))
        .route("/control/kill-switch", post(control_kill_switch))
        .route("/control/backfill", post(control_backfill))
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

async fn status_page(State(state): State<SharedState>) -> Html<String> {
    let data = StatusPageData {
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
        audit: state.repository.recent_audit(20).await.unwrap_or_default(),
        candle_dates: state
            .repository
            .latest_candle_dates()
            .await
            .unwrap_or_default(),
        backfill: state.backfill.lock().expect("backfill status lock").clone(),
    };
    Html(render_status_page(&data))
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
        assert!(page.contains("KILL SWITCH ON"));
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

        // The status page shows the Data section and the failure.
        let response = viewer
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let page = body_text(response.into_body()).await;
        assert!(page.contains("Data"), "data section missing");
        assert!(page.contains("실패"), "failure state not visible");

        let events = shared.repository.recent_audit(10).await.unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "control.backfill"));
    }
}
