//! Background parameter walk-forward runner.

use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::backtest::BacktestConfig;
use crate::backtest_optimize::walk_forward;
use crate::runtime::backfill::BackfillStatus;
use crate::storage::Repository;
use crate::types::Symbol;

/// One optimize pass over stored candles; returns the human summary and
/// persists the winning combo as the strategy's suggested default.
pub async fn run_optimize(
    repository: &Repository,
    strategy_key: &str,
    symbols: &[Symbol],
    years: i64,
) -> Result<String, String> {
    let bars = crate::backtest::load_bars_from_candles(repository, symbols, 120)
        .await
        .map_err(|error| format!("봉 데이터 부족: {error}"))?;
    let years_bars = bars.len();
    let _ = years;

    let config = BacktestConfig::default();
    let outcomes = walk_forward(&bars, strategy_key, symbols, config, 0.7, 3).await?;

    let default = outcomes
        .iter()
        .find(|o| o.is_default)
        .ok_or("기본 파라미터 기준행 없음")?;
    let best = outcomes.first().ok_or("결과 없음")?;

    let improvement = if best.is_default {
        "기본이 최적".to_string()
    } else {
        format!(
            "{:+}%p (기본 {}% → {}%)",
            best.validation.return_pct.round_dp(2) - default.validation.return_pct.round_dp(2),
            default.validation.return_pct.round_dp(2),
            best.validation.return_pct.round_dp(2)
        )
    };
    repository
        .upsert_strategy_suggestion(
            strategy_key,
            &best.label,
            &improvement,
            &serde_json::json!({
                "validation_return_pct": best.validation.return_pct.to_string(),
                "mdd_pct": best.validation.mdd_pct.to_string(),
                "trades": best.validation.trades,
                "win_rate_pct": best.validation.win_rate_pct.to_string(),
            }),
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut lines = format!(
        "최적 {} — 검증 수익률 {}% · MDD {}% · 승률 {}% (기본 {}%, {improvement}) · 봉 {years_bars}개",
        best.label,
        best.validation.return_pct.round_dp(2),
        best.validation.mdd_pct.round_dp(2),
        best.validation.win_rate_pct.round_dp(1),
        default.validation.return_pct.round_dp(2),
    );
    for outcome in outcomes.iter().skip(1).take(2) {
        lines.push_str(&format!(
            "\n{} {}%{}",
            outcome.label,
            outcome.validation.return_pct.round_dp(2),
            if outcome.is_default { " (기본)" } else { "" }
        ));
    }
    Ok(lines)
}

/// Status machine + audit around one background run.
pub fn spawn_optimize(
    repository: Repository,
    strategy_key: String,
    symbols: Vec<Symbol>,
    years: i64,
    status: Arc<Mutex<BackfillStatus>>,
) {
    *status.lock().expect("optimize status lock") = BackfillStatus::Running;
    tokio::spawn(async move {
        let at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        let result = run_optimize(&repository, &strategy_key, &symbols, years).await;
        let (next, payload) = match result {
            Ok(summary) => (
                BackfillStatus::Done {
                    at,
                    summary: format!("[{strategy_key}] {summary}"),
                },
                serde_json::json!({"strategy": strategy_key, "outcome": "done", "summary": summary}),
            ),
            Err(reason) => (
                BackfillStatus::Failed {
                    at,
                    reason: format!("[{strategy_key}] {reason}"),
                },
                serde_json::json!({"strategy": strategy_key, "outcome": "failed", "reason": reason}),
            ),
        };
        let _ = repository
            .record_event("control.optimize", "operator", &payload)
            .await;
        *status.lock().expect("optimize status lock") = next;
    });
}
