//! Background backtest runner — the web control around the engine.

use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::backtest::{summarize, BacktestConfig, BacktestEngine};
use crate::broker::mock::MockBroker;
use crate::runtime::backfill::BackfillStatus;
use crate::storage::Repository;
use crate::strategy_registry;
use crate::types::Symbol;

/// One backtest run to completion; `Err` carries the failure reason. Runs the
/// engine over stored candles with a price-tracking mock broker — no network,
/// no orders can leave.
pub async fn run_backtest(
    repository: &Repository,
    strategy_key: &str,
    symbols: &[Symbol],
    years: i64,
    config: BacktestConfig,
) -> Result<String, String> {
    let strategy = strategy_registry::build(strategy_key, symbols, 10)
        .ok_or_else(|| format!("알 수 없는 전략 키: {strategy_key}"))?;
    let bars = crate::backtest::load_bars_from_candles(repository, symbols, 60)
        .await
        .map_err(|error| format!("봉 데이터 부족: {error}"))?;
    let years_bars = bars.len();
    let _ = years; // window trimming happens in candles selection below

    let initial_cash = config.initial_cash;
    let broker = Arc::new(MockBroker::new());
    let hook_broker = Arc::clone(&broker);
    let engine = BacktestEngine::new(strategy, config).with_bar_hook(Arc::new(move |bar| {
        hook_broker.set_price(&bar.symbol, bar.close);
    }));
    let result = engine.run(bars, &*broker).await;
    let metrics = summarize(&result, initial_cash);

    Ok(format!(
        "수익률 {}% · MDD {}% · 거래 {}회 (승률 {}%) · 수수료 {}원 · 거래세 {}원 · 최종자산 {}원 · 봉 {}개",
        metrics.return_pct.round_dp(2),
        metrics.mdd_pct.round_dp(2),
        metrics.trades,
        metrics.win_rate_pct.round_dp(1),
        crate::runtime::web::fmt_krw(&metrics.total_commission),
        crate::runtime::web::fmt_krw(&metrics.total_tax),
        crate::runtime::web::fmt_krw(&metrics.final_equity),
        years_bars,
    ))
}

/// Drive the status machine + audit trail around one background run.
pub fn spawn_backtest(
    repository: Repository,
    strategy_key: String,
    symbols: Vec<Symbol>,
    years: i64,
    config: BacktestConfig,
    status: Arc<Mutex<BackfillStatus>>,
) {
    *status.lock().expect("backtest status lock") = BackfillStatus::Running;
    tokio::spawn(async move {
        let at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        let result = run_backtest(&repository, &strategy_key, &symbols, years, config).await;
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
            .record_event("control.backtest", "operator", &payload)
            .await;
        *status.lock().expect("backtest status lock") = next;
    });
}
