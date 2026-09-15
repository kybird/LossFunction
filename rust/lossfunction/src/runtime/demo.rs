//! Synthetic paper market — the demo loop.
//!
//! Feeds deterministic random-walk quotes through the real pipeline
//! (quote -> decision -> risk -> gateway -> mock fill -> portfolio) and
//! persists everything to SQLite so a status page shows a living system.
//! Only meaningful on the in-memory paper broker; a kill switch blocks new
//! orders exactly as it would in production wiring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rust_decimal::Decimal;

use crate::broker::mock::MockBroker;
use crate::broker::Broker as _;
use crate::domain::order::OrderStatus;
use crate::risk::RiskManager;
use crate::runtime::TradingRuntime;
use crate::storage::Repository;
use crate::strategy::{EntryPriceStrategy, Strategy};
use crate::types::{Quote, Symbol};

/// Deterministic uniform [-1, 1) generator (no external rand dependency;
/// same seed -> same sequence, asserted by tests).
pub struct DemoRng {
    state: u64,
}

impl DemoRng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_mul(0x9E3779B97F4A7C15) | 1,
        }
    }

    pub fn next_uniform(&mut self) -> f64 {
        // xorshift64* — deterministic, adequate for a demo random walk.
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        let value = x.wrapping_mul(0x2545F4914F6CDD1D);
        // Map to [0, 1) with 53 bits of mantissa.
        (value >> 11) as f64 / (1u64 << 53) as f64
    }
}

const SYMBOLS: [(&str, i64); 3] = [("005930", 80_000), ("035420", 41_000), ("069500", 100_000)];

fn entry_prices() -> std::collections::BTreeMap<Symbol, Decimal> {
    SYMBOLS
        .iter()
        .map(|(code, price)| {
            (
                Symbol::parse(*code).unwrap(),
                Decimal::from(*price) * Decimal::from(995) / Decimal::from(1000), // -0.5%
            )
        })
        .collect()
}

/// One `tick()` per interval; each tick is a full decision cycle.
pub struct DemoLoop {
    runtime: TradingRuntime,
    broker: Arc<MockBroker>,
    repository: Repository,
    interval: Duration,
    rng: DemoRng,
    prices: HashMap<Symbol, i64>,
    persisted_status: HashMap<String, OrderStatus>,
    persisted_fills: Vec<String>,
}

impl DemoLoop {
    pub fn new(
        broker: Arc<MockBroker>,
        risk: Arc<RiskManager>,
        repository: Repository,
        seed: u64,
    ) -> Self {
        let strategy = EntryPriceStrategy::new(entry_prices(), 5);
        let runtime = TradingRuntime::new(
            Arc::clone(&broker) as Arc<dyn crate::broker::Broker>,
            Box::new(strategy) as Box<dyn Strategy>,
            risk,
            "ord",
            120,
        );
        Self {
            runtime,
            broker,
            repository,
            interval: Duration::from_secs(3),
            rng: DemoRng::new(seed),
            prices: SYMBOLS
                .iter()
                .map(|(code, price)| (Symbol::parse(*code).unwrap(), *price))
                .collect(),
            persisted_status: HashMap::new(),
            persisted_fills: Vec::new(),
        }
    }

    pub fn runtime(&mut self) -> &mut TradingRuntime {
        &mut self.runtime
    }

    pub async fn run(&mut self) {
        loop {
            if let Err(error) = self.tick().await {
                eprintln!("demo tick failed: {error}");
            }
            tokio::time::sleep(self.interval).await;
        }
    }

    /// Advance the synthetic market by one step (public for tests).
    pub async fn tick(&mut self) -> Result<(), crate::storage::StorageError> {
        let now = Utc::now();
        // Deterministic iteration: HashMap order varies per instance and
        // would change the rng draw sequence between same-seed loops.
        let mut symbols: Vec<Symbol> = self.prices.keys().cloned().collect();
        symbols.sort();
        for symbol in &symbols {
            let drift = self.rng.next_uniform() * 0.008 - 0.004; // ±0.4%
            let price = self.prices.get_mut(symbol).unwrap();
            *price = (*price as f64 * (1.0 + drift)).round() as i64;
        }

        let quotes: Vec<Quote> = symbols
            .iter()
            .map(|symbol| Quote {
                symbol: symbol.clone(),
                last_price: Decimal::from(self.prices[symbol]),
                timestamp: now,
            })
            .collect();
        for quote in &quotes {
            self.broker.set_price(&quote.symbol, quote.last_price);
            self.runtime.on_quote(quote.clone());
            self.repository
                .insert_quote(&quote.symbol, quote.last_price, now)
                .await?;
        }

        let submitted = self.runtime.run_decision_cycle().await;
        let broker_ids = self.runtime.open_local_orders().clone();
        for order in &submitted {
            self.repository.create_order(order, "paper").await?;
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
    use chrono::TimeDelta;

    fn risk() -> Arc<CoreRiskManager> {
        Arc::new(CoreRiskManager::with_clock(
            RiskLimits {
                max_order_notional: Decimal::from(10_000_000),
                max_position_quantity: 50,
                max_gross_exposure: Decimal::from(30_000_000),
                daily_loss_limit: Decimal::from(500_000),
                stale_quote_max_age: TimeDelta::seconds(60),
            },
            Utc::now,
        ))
    }

    async fn demo(path: &std::path::Path, seed: u64) -> DemoLoop {
        let repository = Repository::open(path.to_str().unwrap()).await.unwrap();
        DemoLoop::new(Arc::new(MockBroker::new()), risk(), repository, seed)
    }

    #[tokio::test]
    async fn ticks_populate_database() {
        let dir = tempfile::tempdir().unwrap();
        let mut demo = demo(&dir.path().join("a.db"), 7).await;
        for _ in 0..30 {
            demo.tick().await.unwrap();
        }

        let positions = demo.repository.positions().await.unwrap();
        assert!(!positions.is_empty(), "demo should hold positions");
        let fills = demo.repository.recent_fills(50).await.unwrap();
        assert!(!fills.is_empty(), "crossing entries should have filled");
        let orders = demo.repository.recent_orders(50).await.unwrap();
        assert!(!orders.is_empty());
        assert!(orders.iter().any(|o| o.status == "filled"));
    }

    #[tokio::test]
    async fn kill_switch_blocks_demo_orders() {
        let dir = tempfile::tempdir().unwrap();
        // Build the loop with an already-activated shared risk manager to
        // prove the block end to end.
        let risk = risk();
        risk.activate_kill_switch("test halt");
        let repository = Repository::open(dir.path().join("c.db").to_str().unwrap())
            .await
            .unwrap();
        let mut halted = DemoLoop::new(Arc::new(MockBroker::new()), risk, repository, 7);
        for _ in 0..5 {
            halted.tick().await.unwrap();
        }
        assert!(halted
            .repository
            .recent_orders(10)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn demo_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = demo(&dir.path().join("d1.db"), 42).await;
        let mut b = demo(&dir.path().join("d2.db"), 42).await;
        for _ in 0..40 {
            a.tick().await.unwrap();
            b.tick().await.unwrap();
        }
        let positions_a = a.repository.positions().await.unwrap();
        let positions_b = b.repository.positions().await.unwrap();
        let norm = |positions: Vec<crate::storage::StoredPosition>| {
            positions
                .into_iter()
                .map(|p| (p.symbol, p.quantity, p.average_price.to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(norm(positions_a), norm(positions_b));
    }
}
