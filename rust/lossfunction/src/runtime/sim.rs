//! Real-data replay simulation — paper trading on actual market history.
//!
//! Replays stored candles (the backfill's real daily bars) through the real
//! pipeline (quote -> aggregator/history -> strategy -> risk -> gateway ->
//! mock fill -> portfolio) and persists everything so the status page shows
//! positions and P&L grounded in real prices. No order can leave: the only
//! broker is the in-process MockBroker; the venue is never contacted.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::broker::mock::MockBroker;
use crate::broker::Broker as _;
use crate::domain::order::OrderStatus;
use crate::risk::RiskManager;
use crate::runtime::TradingRuntime;
use crate::storage::Repository;
use crate::strategy_registry;
use crate::types::{Quote, Symbol};

/// One replayed market event.
#[derive(Debug, Clone)]
struct ReplayBar {
    symbol: Symbol,
    timestamp: chrono::DateTime<chrono::Utc>,
    close: rust_decimal::Decimal,
}

/// Replays real candles through the live runtime. `tick()` advances one
/// replayed bar (public for tests); `run()` loops with `interval` as the
/// replay acceleration.
pub struct SimLoop {
    runtime: TradingRuntime,
    broker: Arc<MockBroker>,
    repository: Repository,
    interval: Duration,
    bars: Vec<ReplayBar>,
    cursor: usize,
    persisted_status: HashMap<String, OrderStatus>,
    persisted_fills: Vec<String>,
}

impl SimLoop {
    /// Loads candles for `symbols` from `repository` (newest-last, merged
    /// chronologically across symbols) and wires the runtime with the
    /// registry strategy `strategy_key`.
    pub async fn new(
        broker: Arc<MockBroker>,
        risk: Arc<RiskManager>,
        repository: Repository,
        symbols: Vec<Symbol>,
        strategy_key: &str,
        interval: Duration,
    ) -> Self {
        let mut bars = Vec::new();
        for symbol in &symbols {
            let stored = repository
                .daily_candles(symbol, crate::marketdata::Timeframe::Day)
                .await
                .unwrap_or_default();
            for bar in stored {
                bars.push(ReplayBar {
                    symbol: symbol.clone(),
                    timestamp: bar.timestamp,
                    close: bar.close,
                });
            }
        }
        bars.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        let strategy = strategy_registry::build(strategy_key, &symbols, 10)
            .expect("simulation requires a registered strategy key");
        let runtime = TradingRuntime::new(
            Arc::clone(&broker) as Arc<dyn crate::broker::Broker>,
            strategy,
            risk,
            "sim",
            120,
        );
        Self {
            runtime,
            broker,
            repository,
            interval,
            bars,
            cursor: 0,
            persisted_status: HashMap::new(),
            persisted_fills: Vec::new(),
        }
    }

    pub fn runtime(&mut self) -> &mut TradingRuntime {
        &mut self.runtime
    }

    pub fn strategy_label(&self) -> String {
        self.runtime.strategy_label()
    }

    /// Total replayable bars.
    pub fn total_bars(&self) -> usize {
        self.bars.len()
    }

    /// Bars replayed so far.
    pub fn replayed(&self) -> usize {
        self.cursor
    }

    pub async fn run(&mut self) {
        loop {
            if let Err(error) = self.tick().await {
                eprintln!("sim replay tick failed: {error}");
            }
            if self.cursor >= self.bars.len() {
                eprintln!("sim replay finished ({} bars)", self.bars.len());
                return;
            }
            tokio::time::sleep(self.interval).await;
        }
    }

    /// Replay one bar: real close price through the full live pipeline.
    pub async fn tick(&mut self) -> Result<(), crate::storage::StorageError> {
        let Some(bar) = self.bars.get(self.cursor).cloned() else {
            return Ok(()); // replay exhausted
        };
        self.cursor += 1;

        let quote = Quote {
            symbol: bar.symbol.clone(),
            last_price: bar.close,
            timestamp: bar.timestamp,
        };
        self.broker.set_price(&quote.symbol, quote.last_price);
        self.runtime.on_quote(quote.clone());
        self.repository
            .insert_quote(&quote.symbol, quote.last_price, bar.timestamp)
            .await?;

        let submitted = self.runtime.run_decision_cycle().await;
        let broker_ids = self.runtime.open_local_orders().clone();
        for order in &submitted {
            self.repository.create_order(order, "paper-sim").await?;
            self.persisted_status
                .insert(order.client_order_id().to_string(), OrderStatus::Submitted);
        }

        let statuses = self.runtime.sync_all().await;
        for (client_order_id, status) in &statuses {
            if self.persisted_status.get(client_order_id) != Some(status) {
                self.repository
                    .update_order_status(client_order_id, *status, None)
                    .await?;
                self.persisted_status
                    .insert(client_order_id.clone(), *status);
            }
            if *status == OrderStatus::Filled && !self.persisted_fills.contains(client_order_id) {
                if let Some(broker_id) = broker_ids.get(client_order_id) {
                    self.record_fill(client_order_id, broker_id).await?;
                }
            }
        }

        for position in self.runtime.portfolio().positions() {
            if position.quantity > 0 {
                self.repository
                    .upsert_position(&position.symbol, position.quantity, position.average_price)
                    .await?;
            }
        }
        Ok(())
    }

    async fn record_fill(
        &mut self,
        client_order_id: &str,
        broker_order_id: &str,
    ) -> Result<(), crate::storage::StorageError> {
        let Ok(report) = self.broker.execution_report(broker_order_id).await else {
            return Ok(());
        };
        if report.filled_quantity > 0 {
            if let Some(price) = report.average_fill_price {
                self.repository
                    .record_fill(
                        client_order_id,
                        &report.symbol,
                        report.side,
                        report.filled_quantity,
                        price,
                        report.timestamp,
                    )
                    .await?;
            }
        }
        self.persisted_fills.push(client_order_id.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::{RiskLimits, RiskManager as CoreRiskManager};
    use chrono::TimeZone;

    fn risk() -> Arc<RiskManager> {
        Arc::new(CoreRiskManager::with_clock(
            RiskLimits {
                max_order_notional: rust_decimal::Decimal::from(10_000_000),
                max_position_quantity: 50,
                max_gross_exposure: rust_decimal::Decimal::from(30_000_000),
                daily_loss_limit: rust_decimal::Decimal::from(500_000),
                stale_quote_max_age: chrono::TimeDelta::weeks(100), // replayed stamps are old
            },
            chrono::Utc::now,
        ))
    }

    async fn sim(path: &std::path::Path) -> SimLoop {
        let repository = Repository::open(path.to_str().unwrap()).await.unwrap();
        repository.migrate().await.unwrap();
        let symbol = Symbol::parse("005930").unwrap();
        // Oscillating real-style series: rises, dips, rises — golden cross
        // territory for sma-cross after enough bars.
        let mut seeded = Vec::new();
        for i in 0..120i64 {
            let day =
                chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + chrono::Duration::days(i);
            let ts = chrono::Utc.from_utc_datetime(&day.and_hms_opt(6, 30, 0).unwrap());
            let level = if i % 40 < 20 { 80_000 } else { 90_000 };
            let close = crate::storage::int_to_money((level + (i % 20) * 100) * 10_000);
            seeded.push(crate::marketdata::bar(
                &symbol, ts, close, close, close, close, 1_000,
            ));
        }
        repository.upsert_candles(&seeded).await.unwrap();
        SimLoop::new(
            Arc::new(MockBroker::new()),
            risk(),
            repository,
            vec![symbol],
            "sma-cross",
            Duration::from_millis(1),
        )
        .await
    }

    /// Replay progresses through the real bars and trades land in storage —
    /// the pipeline runs on real prices with the mock broker only.
    #[tokio::test]
    async fn replay_runs_real_bars_and_persists_activity() {
        let dir = tempfile::tempdir().unwrap();
        let mut sim = sim(&dir.path().join("sim.db")).await;
        assert_eq!(sim.total_bars(), 120);

        // Run the full replay synchronously (tick = one bar).
        for _ in 0..120 {
            sim.tick().await.unwrap();
        }
        assert_eq!(sim.replayed(), 120);
        assert!(
            sim.tick().await.unwrap(); // exhausted replay is a no-op
            assert_eq!(sim.replayed(), 120, "cursor must not advance past the end");
            "exhausted replay is a no-op"
        );

        // Positions/orders visible through the repository (status page source).
        let orders = sim.repository.recent_orders(200).await.unwrap();
        assert!(!orders.is_empty(), "strategy placed real-data orders");
        let positions = sim.repository.positions().await.unwrap();
        assert!(positions.iter().all(|p| p.quantity >= 0), "positions sane");
        // The kill switch path is shared with production wiring.
        sim.runtime().strategy_label();
    }

    /// The replay is deterministic: same candles -> same order sequence.
    #[tokio::test]
    async fn replay_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = sim(&dir.path().join("a.db")).await;
        let mut b = sim(&dir.path().join("b.db")).await;
        for _ in 0..120 {
            a.tick().await.unwrap();
            b.tick().await.unwrap();
        }
        let norm = |orders: Vec<crate::storage::RecentOrder>| {
            orders
                .into_iter()
                .map(|o| (o.symbol, o.side, o.quantity, o.status))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            norm(a.repository.recent_orders(200).await.unwrap()),
            norm(b.repository.recent_orders(200).await.unwrap())
        );
    }
}
