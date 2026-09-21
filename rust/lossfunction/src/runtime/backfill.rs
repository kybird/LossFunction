//! Background daily-bar backfill — the status page's data-refresh control.
//!
//! Read-only on the venue (the chart quotation endpoint only — no order can
//! leave through this path) and idempotent on storage (candle upserts), so a
//! stray button click is safe by construction. The only thing to guard is
//! concurrent runs, which the HTTP layer enforces before spawning.

use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::kis::auth::{kis_base_url, KisAuth};
use crate::kis::chart::KisChartSource;
use crate::kis::rest::KisRestClient;
use crate::marketdata::backfill_daily;
use crate::storage::Repository;
use crate::types::Symbol;

/// Credential snapshot injected at startup (settings -> env -> vault upstream;
/// the web layer never reads secrets itself).
#[derive(Clone, Debug)]
pub struct BackfillCredentials {
    /// "real" | "mock" — quotation endpoints are read-only, so real is safe.
    pub environment: String,
    pub app_key: String,
    pub app_secret: String,
    pub account: String,
    /// Tests point this at a dead local port to exercise the failure path
    /// offline; production leaves it None and the environment decides.
    pub base_url_override: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum BackfillStatus {
    #[default]
    Idle,
    Running,
    Done {
        at: String,
        summary: String,
    },
    Failed {
        at: String,
        reason: String,
    },
}

impl BackfillStatus {
    /// Short label for the status page (values are page-safe words).
    pub fn label(&self) -> &'static str {
        match self {
            BackfillStatus::Idle => "idle",
            BackfillStatus::Running => "running",
            BackfillStatus::Done { .. } => "done",
            BackfillStatus::Failed { .. } => "failed",
        }
    }
}

/// One backfill pass to completion. Returns `Err(reason)` when any symbol
/// failed (the reason carries the full per-symbol detail), `Ok(summary)`
/// otherwise. Blocking — callers run it on a spawned task.
pub async fn run_backfill(
    credentials: &BackfillCredentials,
    repository: &Repository,
    watchlist: &[Symbol],
    years: i64,
) -> Result<String, String> {
    let base_url = credentials
        .base_url_override
        .clone()
        .unwrap_or_else(|| kis_base_url(&credentials.environment).to_string());
    let http = reqwest::Client::new();
    let auth = KisAuth::new(
        base_url.clone(),
        credentials.app_key.clone(),
        credentials.app_secret.clone(),
        http.clone(),
    );
    let rest = KisRestClient::new(
        auth,
        &credentials.account,
        &credentials.environment,
        base_url,
        http,
        "backfill",
    )
    .map_err(|error| format!("KIS 클라이언트 구성 실패: {error}"))?;
    let source = KisChartSource::new(rest);

    let to = Utc::now();
    let from = to - chrono::Duration::days(365 * years);
    let outcomes = backfill_daily(&source, repository, watchlist, from, to).await;
    let total: u64 = outcomes
        .iter()
        .filter_map(|(_, outcome)| outcome.as_ref().ok().copied())
        .sum();
    let failures = outcomes
        .iter()
        .filter(|(_, outcome)| outcome.is_err())
        .count();
    let mut report = format!(
        "{total} bars stored, {failures}/{} symbols failed",
        outcomes.len()
    );
    for (symbol, outcome) in &outcomes {
        if let Err(error) = outcome {
            report.push_str(&format!("\n{symbol}: {error}"));
        }
    }
    if failures == 0 {
        Ok(report)
    } else {
        Err(report)
    }
}

/// Drive the status machine around one background run: Running now, terminal
/// state plus an audit event when it settles.
pub fn spawn_backfill(
    credentials: BackfillCredentials,
    repository: Repository,
    watchlist: Vec<Symbol>,
    years: i64,
    status: Arc<Mutex<BackfillStatus>>,
) {
    *status.lock().expect("backfill status lock") = BackfillStatus::Running;
    tokio::spawn(async move {
        let at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        let result = run_backfill(&credentials, &repository, &watchlist, years).await;
        let (next, outcome) = match result {
            Ok(summary) => (
                BackfillStatus::Done {
                    at,
                    summary: summary.clone(),
                },
                serde_json::json!({ "outcome": "done", "summary": summary }),
            ),
            Err(reason) => (
                BackfillStatus::Failed {
                    at,
                    reason: reason.clone(),
                },
                serde_json::json!({ "outcome": "failed", "reason": reason }),
            ),
        };
        let _ = repository
            .record_event("control.backfill", "operator", &outcome)
            .await;
        *status.lock().expect("backfill status lock") = next;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dead_port_credentials() -> BackfillCredentials {
        BackfillCredentials {
            environment: "real".into(),
            app_key: "test-key".into(),
            app_secret: "test-secret".into(),
            account: "12345678-01".into(),
            base_url_override: Some("http://127.0.0.1:9".into()), // nothing listens
        }
    }

    /// Offline failure path: an unreachable venue settles the machine in
    /// Failed (never stuck in Running) and leaves an audit trail.
    #[tokio::test]
    async fn unreachable_venue_settles_failed_with_audit() {
        let dir = tempfile::tempdir().unwrap();
        let repository = Repository::open(dir.path().join("bf.db").to_str().unwrap())
            .await
            .unwrap();
        repository.migrate().await.unwrap();
        let status = Arc::new(Mutex::new(BackfillStatus::Idle));

        spawn_backfill(
            dead_port_credentials(),
            repository.clone(),
            vec![Symbol::parse("005930").unwrap()],
            1,
            Arc::clone(&status),
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let settled = !matches!(
                *status.lock().unwrap(),
                BackfillStatus::Running | BackfillStatus::Idle
            );
            if settled {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "backfill never settled"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        match &*status.lock().unwrap() {
            BackfillStatus::Failed { reason, .. } => {
                assert!(reason.contains("symbols failed"), "got: {reason}")
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        let events = repository.recent_audit(10).await.unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "control.backfill"));
    }
}
