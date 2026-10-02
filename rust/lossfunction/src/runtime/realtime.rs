//! Realtime quotes simulation — paper trading on the live market.
//!
//! The KIS realtime execution-price stream (H0STCNT0, read-only market data)
//! drives the real pipeline (quote -> aggregator/history -> strategy -> risk
//! -> gateway -> mock fill -> portfolio). History is seeded from backfilled
//! daily candles so indicator strategies can decide from the first tick.
//! No order can leave: the only broker is the in-process MockBroker — this is
//! the fake-fill simulation, the venue is never asked to trade.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::broker::mock::MockBroker;
use crate::broker::Broker as _;
use crate::domain::order::OrderStatus;
use crate::risk::RiskManager;
use crate::runtime::TradingRuntime;
use crate::storage::Repository;
use crate::strategy_registry;
use crate::types::{Quote, Symbol};

/// Quote-driven runtime. Every tick updates in-memory state; the decision
/// cycle and persistence run at most once per `cycle_interval` so a burst of
/// ticks cannot spin SQLite (the latest quote per symbol is what persists).
pub struct RealtimeLoop {
    runtime: TradingRuntime,
    broker: Arc<MockBroker>,
    repository: Repository,
    cycle_interval: Duration,
    next_cycle: Instant,
    pending: HashMap<Symbol, Quote>,
    persisted_status: HashMap<String, OrderStatus>,
    persisted_fills: Vec<String>,
}

impl RealtimeLoop {
    /// Wires the runtime with the registry strategy `strategy_key` and seeds
    /// its lookback window from the SOURCE database's backfilled candles
    /// (newest-last per symbol, oldest-first seeding).
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        broker: Arc<MockBroker>,
        risk: Arc<RiskManager>,
        repository: Repository,
        source: Repository,
        symbols: Vec<Symbol>,
        strategy_key: &str,
        history_window: usize,
        cycle_interval: Duration,
    ) -> Self {
        let strategy = strategy_registry::build(strategy_key, &symbols, 10)
            .expect("realtime loop requires a registered strategy key");
        let mut runtime = TradingRuntime::new(
            Arc::clone(&broker) as Arc<dyn crate::broker::Broker>,
            strategy,
            risk,
            "rt",
            history_window,
        );
        let mut seeded = 0usize;
        for symbol in &symbols {
            let candles = source
                .daily_candles(symbol, crate::marketdata::Timeframe::Day)
                .await
                .unwrap_or_default();
            for bar in candles {
                runtime.seed_daily_close(symbol, bar.close);
                seeded += 1;
            }
        }
        println!(
            "realtime loop: strategy {} seeded with {seeded} daily closes x {} symbols",
            strategy_key,
            symbols.len()
        );
        Self {
            runtime,
            broker,
            repository,
            cycle_interval,
            next_cycle: Instant::now(),
            pending: HashMap::new(),
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

    /// Process one live tick. Cheap paths always run (price/history update);
    /// the decision cycle + persistence run on the throttled cadence.
    pub async fn on_quote(&mut self, quote: Quote) {
        self.broker.set_price(&quote.symbol, quote.last_price);
        self.runtime.on_quote(quote.clone());
        self.pending.insert(quote.symbol.clone(), quote);

        let now = Instant::now();
        if now >= self.next_cycle {
            self.next_cycle = now + self.cycle_interval;
            if let Err(error) = self.cycle().await {
                eprintln!("realtime cycle failed: {error}");
            }
        }
    }

    /// Consume quotes from the market-data client until the channel closes.
    pub async fn run(&mut self, mut quotes: tokio::sync::mpsc::UnboundedReceiver<Quote>) {
        while let Some(quote) = quotes.recv().await {
            self.on_quote(quote).await;
        }
        eprintln!("realtime loop: quote stream ended");
    }

    /// Persist the latest quote per symbol, run one decision cycle and
    /// reconcile orders/fills/positions (same persistence contract as the
    /// demo and sim loops).
    async fn cycle(&mut self) -> Result<(), crate::storage::StorageError> {
        let mut symbols: Vec<&Symbol> = self.pending.keys().collect();
        symbols.sort();
        let now = chrono::Utc::now();
        for symbol in symbols {
            let quote = &self.pending[symbol];
            self.repository
                .insert_quote(symbol, quote.last_price, now)
                .await?;
        }

        let submitted = self.runtime.run_decision_cycle().await;
        let broker_ids = self.runtime.open_local_orders().clone();
        for order in &submitted {
            self.repository
                .create_order(order, "paper-realtime")
                .await?;
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

    fn risk() -> Arc<CoreRiskManager> {
        Arc::new(CoreRiskManager::with_clock(
            RiskLimits {
                max_order_notional: rust_decimal::Decimal::from(10_000_000),
                max_position_quantity: 50,
                max_gross_exposure: rust_decimal::Decimal::from(30_000_000),
                daily_loss_limit: rust_decimal::Decimal::from(500_000),
                stale_quote_max_age: chrono::TimeDelta::weeks(100),
            },
            chrono::Utc::now,
        ))
    }

    /// Seed the SOURCE database so the golden cross lands on the LAST seeded
    /// close (flat 1000 -> dip 960 -> mild 970 recovery -> 1200 breakout):
    /// fast(5) 1016 > slow(20) 994 at the final bar, below it the bar before.
    /// The live same-date tick never enters history (look-ahead guard), so
    /// the decision cycle fires on the seeded cross alone.
    async fn seed_source(source: &Repository) {
        let symbol = Symbol::parse("005930").unwrap();
        let mut closes = Vec::new();
        closes.extend(std::iter::repeat_n(rust_decimal::Decimal::from(1_000), 20));
        closes.extend(std::iter::repeat_n(rust_decimal::Decimal::from(960), 5));
        closes.extend(std::iter::repeat_n(rust_decimal::Decimal::from(970), 4));
        closes.push(rust_decimal::Decimal::from(1_200));
        let mut bars = Vec::new();
        for (day, close) in closes.into_iter().enumerate() {
            bars.push(crate::marketdata::Bar {
                symbol: symbol.clone(),
                timeframe: crate::marketdata::Timeframe::Day,
                timestamp: chrono::Utc.with_ymd_and_hms(2026, 9, 1, 6, 30, 0).unwrap()
                    + chrono::Duration::days(day as i64),
                open: close,
                high: close,
                low: close,
                close,
                volume: 0,
            });
        }
        source.upsert_candles(&bars).await.unwrap();
    }

    async fn loop_with(path: &std::path::Path, cycle_interval: Duration) -> RealtimeLoop {
        let repository = Repository::open(path.to_str().unwrap()).await.unwrap();
        repository.migrate().await.unwrap();
        let source = Repository::open(path.to_str().unwrap()).await.unwrap();
        source.migrate().await.unwrap();
        seed_source(&source).await;
        let symbols = vec![Symbol::parse("005930").unwrap()];
        RealtimeLoop::new(
            Arc::new(MockBroker::new()),
            risk(),
            repository,
            source,
            symbols,
            "sma-cross",
            120,
            cycle_interval,
        )
        .await
    }

    fn tick(minute: u32, price: i64) -> Quote {
        Quote {
            symbol: Symbol::parse("005930").unwrap(),
            last_price: rust_decimal::Decimal::from(price),
            timestamp: chrono::Utc
                .with_ymd_and_hms(2026, 10, 2, 5, minute, 0)
                .unwrap(),
        }
    }

    /// Seeded history: the strategy sees 30 backfilled closes before any
    /// live tick arrives — the lookback is not empty at boot.
    #[tokio::test]
    async fn seeds_history_from_backfilled_candles() {
        let dir = tempfile::tempdir().unwrap();
        let mut realtime = loop_with(&dir.path().join("a.db"), Duration::from_millis(0)).await;
        let symbol = Symbol::parse("005930").unwrap();
        let snapshot = realtime.runtime().snapshot();
        let history = snapshot
            .history
            .get(&symbol)
            .map(|closes| closes.len())
            .unwrap_or(0);
        assert!(history >= 20, "expected seeded lookback, got {history}");
    }

    /// A live tick on top of a crossed seed produces a decision, a mock fill
    /// and persistence — the full fake-purchase pipeline.
    #[tokio::test]
    async fn live_tick_crosses_entry_and_fills() {
        let dir = tempfile::tempdir().unwrap();
        let mut realtime = loop_with(&dir.path().join("b.db"), Duration::from_millis(0)).await;
        realtime.on_quote(tick(1, 1_100)).await;

        let fills = realtime.repository.recent_fills(10).await.unwrap();
        assert!(
            !fills.is_empty(),
            "expected a fake fill from the mock broker"
        );
        let positions = realtime.repository.positions().await.unwrap();
        assert!(!positions.is_empty(), "expected a persisted position");
        let orders = realtime.repository.recent_orders(10).await.unwrap();
        assert!(!orders.is_empty());
        assert!(orders.iter().any(|o| o.status == "filled"));
    }

    /// Throttle: two ticks inside one interval persist one quote row for the
    /// symbol (latest), not two — bursts cannot spin SQLite.
    #[tokio::test]
    async fn burst_persists_latest_quote_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut realtime = loop_with(&dir.path().join("c.db"), Duration::from_secs(300)).await;
        realtime.on_quote(tick(1, 1_500)).await;
        realtime.on_quote(tick(2, 1_600)).await;

        let symbol = Symbol::parse("005930").unwrap();
        let quotes = realtime
            .repository
            .quote_history(&symbol, 10)
            .await
            .unwrap();
        assert_eq!(
            quotes.len(),
            1,
            "expected exactly the first cycle's quote row"
        );
    }
}
