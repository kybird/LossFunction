//! Trading runtime orchestrator.
//!
//! Wires the full event flow (wiki: architecture §3):
//! quote -> strategy decision -> risk check -> gateway submit ->
//! execution sync -> portfolio update — and owns restart recovery:
//! pending orders re-registered at Unknown and reconciled against the
//! broker, then the portfolio rebuilt from broker-side positions.

pub mod assembly;
pub mod demo;
pub mod server;
pub mod web;

use std::collections::HashMap;
use std::sync::Arc;

use crate::broker::Broker;
use crate::domain::order::{Order, OrderStatus};
use crate::domain::portfolio::{Fill, Portfolio};
use crate::execution::gateway::{target_status, OrderGateway, Reconciler, SubmitError};
use crate::execution::state_machine::StateMachine;
use crate::risk::RiskManager;
use crate::strategy::{DecisionLayer, MarketSnapshot, OrderIntent, Strategy};
use crate::types::{OrderSide, Quote, Symbol};

/// Coordinates strategy, risk, execution, and recovery.
pub struct TradingRuntime {
    broker: Arc<dyn Broker>,
    reconciler: Reconciler,
    decisions: DecisionLayer,
    risk: Arc<RiskManager>,
    machine: StateMachine,
    gateway: OrderGateway,
    history: crate::history::HistoryWindow,
    /// Live ticks close into daily bars here; completed bars feed `history`.
    aggregator: crate::marketdata::BarAggregator,
    quotes: HashMap<Symbol, Quote>,
    portfolio: Portfolio,
    order_prefix: String,
    order_seq: u64,
    /// client_order_id -> broker_order_id of submitted-but-unsynced orders.
    open_local: HashMap<String, String>,
}

impl TradingRuntime {
    pub fn new(
        broker: Arc<dyn Broker>,
        strategy: Box<dyn Strategy>,
        risk: Arc<RiskManager>,
        order_prefix: impl Into<String>,
        history_capacity: usize,
    ) -> Self {
        Self {
            reconciler: Reconciler::new(Arc::clone(&broker)),
            gateway: OrderGateway::new(Arc::clone(&broker)),
            broker,
            decisions: DecisionLayer::new(strategy),
            risk,
            machine: StateMachine::new(),
            history: crate::history::HistoryWindow::new(history_capacity),
            aggregator: crate::marketdata::BarAggregator::new(),
            quotes: HashMap::new(),
            portfolio: Portfolio::new(),
            order_prefix: order_prefix.into(),
            order_seq: 0,
            open_local: HashMap::new(),
        }
    }

    pub fn portfolio(&self) -> &Portfolio {
        &self.portfolio
    }

    pub fn machine(&self) -> &StateMachine {
        &self.machine
    }

    pub fn status_of(&self, client_order_id: &str) -> Option<OrderStatus> {
        self.machine.status(client_order_id)
    }

    /// client_order_id -> broker_order_id for recovery after a crash.
    pub fn open_local_orders(&self) -> &HashMap<String, String> {
        &self.open_local
    }

    /// Record a quote; returns the snapshot it produced. A tick from a new
    /// trading date first closes the previous day's bar into `history` —
    /// snapshots only ever see completed bars.
    pub fn on_quote(&mut self, quote: Quote) -> MarketSnapshot {
        if let Some(bar) = self.aggregator.push(&quote) {
            self.history.push(&bar.symbol, bar.close);
        }
        self.quotes.insert(quote.symbol.clone(), quote);
        self.snapshot()
    }

    pub fn snapshot(&self) -> MarketSnapshot {
        MarketSnapshot {
            quotes: self.quotes.clone(),
            positions: self
                .portfolio
                .positions()
                .map(|position| (position.symbol.clone(), position.clone()))
                .collect(),
            as_of: format!("{}-session", self.order_prefix),
            history: self.history.snapshot(),
        }
    }

    /// One strategy -> risk -> submit pass; returns submitted orders.
    pub async fn run_decision_cycle(&mut self) -> Vec<Order> {
        let snapshot = self.snapshot();
        let decision = self.decisions.decide(&snapshot);
        let mut submitted = Vec::new();
        for intent in &decision.intents {
            // A malformed strategy intent is rejected like a risk rejection —
            // one bad intent must never kill the runtime. Recording is the
            // ops layer's job (audit trail), same as risk rejections.
            let Ok(order) = self.new_order(intent) else {
                continue;
            };
            if self
                .risk
                .check_order(&order, &self.portfolio, &self.quotes)
                .is_err()
            {
                continue; // risk rejection is recorded by the ops layer
            }
            match self.gateway.submit(&mut self.machine, &order).await {
                Ok(ack) => {
                    self.open_local
                        .insert(order.client_order_id().to_string(), ack.broker_order_id);
                    submitted.push(order);
                }
                // Timeout: the order sits in Unknown; reconciliation is the
                // only way out — never resubmit here.
                Err(SubmitError::Timeout(_)) | Err(SubmitError::Duplicate(_)) => {}
                Err(SubmitError::Broker(_)) | Err(SubmitError::MachineError(_)) => {}
            }
        }
        submitted
    }

    fn new_order(&mut self, intent: &OrderIntent) -> Result<Order, crate::domain::DomainError> {
        self.order_seq += 1;
        let client_order_id = format!("{}-{:04}", self.order_prefix, self.order_seq);
        Order::new(
            client_order_id,
            intent.symbol.clone(),
            intent.side,
            intent.order_type,
            intent.quantity,
            intent.limit_price,
        )
    }

    /// Pull the broker-side execution state for one open local order.
    /// Terminal statuses apply discovered fills and clear the open set.
    pub async fn sync_execution(&mut self, client_order_id: &str) -> Option<OrderStatus> {
        let Some(broker_order_id) = self.open_local.get(client_order_id).cloned() else {
            return self.machine.status(client_order_id);
        };

        let mut status = self.machine.status(client_order_id)?;
        if matches!(
            status,
            OrderStatus::Submitted | OrderStatus::PartiallyFilled
        ) {
            if let Ok(report) = self.broker.execution_report(&broker_order_id).await {
                let target = target_status(&report);
                if target != status {
                    status = self
                        .machine
                        .transition(
                            client_order_id,
                            target,
                            format!(
                                "execution sync: filled {}/{} open={}",
                                report.filled_quantity, report.order_quantity, report.open
                            ),
                        )
                        .ok()?;
                }
            }
        } else if status == OrderStatus::Unknown {
            status = self
                .reconciler
                .reconcile(&mut self.machine, client_order_id, &broker_order_id)
                .await
                .ok()?;
        }

        if !matches!(
            status,
            OrderStatus::Submitted | OrderStatus::PartiallyFilled
        ) {
            self.apply_fills(client_order_id, &broker_order_id).await;
            self.open_local.remove(client_order_id);
        }
        Some(status)
    }

    pub async fn sync_all(&mut self) -> HashMap<String, OrderStatus> {
        let ids: Vec<String> = self.open_local.keys().cloned().collect();
        let mut results = HashMap::new();
        for id in ids {
            if let Some(status) = self.sync_execution(&id).await {
                results.insert(id, status);
            }
        }
        results
    }

    async fn apply_fills(&mut self, client_order_id: &str, broker_order_id: &str) {
        let Ok(report) = self.broker.execution_report(broker_order_id).await else {
            return;
        };
        if self.machine.status(client_order_id) != Some(OrderStatus::Filled)
            || report.filled_quantity <= 0
        {
            return;
        }
        let Some(price) = report.average_fill_price else {
            return;
        };
        let fill = Fill {
            client_order_id: client_order_id.to_string(),
            symbol: report.symbol.clone(),
            side: report.side,
            quantity: report.filled_quantity,
            price,
        };
        if let Ok(next) = self.portfolio.apply_fill(fill) {
            self.portfolio = next;
        }
    }

    /// Restart recovery: reconcile pending orders, rebuild portfolio.
    pub async fn recover(
        &mut self,
        pending: &HashMap<String, String>,
    ) -> HashMap<String, OrderStatus> {
        let results = self
            .reconciler
            .reconcile_pending(&mut self.machine, pending)
            .await
            .unwrap_or_default();
        self.rebuild_portfolio().await;
        for (client_order_id, status) in &results {
            if matches!(
                status,
                OrderStatus::Submitted | OrderStatus::PartiallyFilled
            ) {
                if let Some(broker_id) = pending.get(client_order_id) {
                    self.open_local
                        .insert(client_order_id.clone(), broker_id.clone());
                }
            }
        }
        results
    }

    /// Replace local portfolio state with broker-side positions.
    pub async fn rebuild_portfolio(&mut self) {
        let Ok(positions) = self.broker.positions().await else {
            return;
        };
        let mut next = Portfolio::new();
        for position in positions {
            if position.quantity == 0 {
                continue;
            }
            let fill = Fill {
                client_order_id: format!("restore-{}", position.symbol),
                symbol: position.symbol.clone(),
                side: OrderSide::Buy,
                quantity: position.quantity,
                price: position.average_price,
            };
            if let Ok(with_position) = next.apply_fill(fill) {
                next = with_position;
            }
        }
        self.portfolio = next;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::mock::MockBroker;
    use crate::risk::RiskLimits;
    use crate::strategy::{MarketSnapshot, StrategyDecision};
    use crate::types::OrderType;
    use chrono::TimeDelta;
    use rust_decimal::Decimal;

    fn risk() -> Arc<RiskManager> {
        Arc::new(RiskManager::with_clock(
            RiskLimits {
                max_order_notional: Decimal::from(10_000_000),
                max_position_quantity: 50,
                max_gross_exposure: Decimal::from(30_000_000),
                daily_loss_limit: Decimal::from(500_000),
                stale_quote_max_age: TimeDelta::seconds(60),
            },
            chrono::Utc::now,
        ))
    }

    /// Emits a shape-invalid intent (zero quantity) every cycle.
    struct BadIntentStrategy;

    impl Strategy for BadIntentStrategy {
        fn name(&self) -> &str {
            "bad-intent"
        }
        fn version(&self) -> &str {
            "1"
        }
        fn decide(&self, _snapshot: &MarketSnapshot) -> StrategyDecision {
            StrategyDecision {
                strategy_name: "bad-intent".into(),
                strategy_version: "1".into(),
                intents: vec![OrderIntent {
                    symbol: Symbol::parse("005930").unwrap(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: 0, // shape-invalid: Order::new refuses this
                    limit_price: None,
                }],
                features: Default::default(),
                rationale: String::new(),
            }
        }
    }

    /// A malformed strategy intent must be skipped, never a panic — the
    /// runtime outlives any single bad strategy output (was: `.expect`).
    #[tokio::test]
    async fn malformed_intent_is_rejected_not_fatal() {
        let mut runtime = TradingRuntime::new(
            Arc::new(MockBroker::new()) as Arc<dyn Broker>,
            Box::new(BadIntentStrategy),
            risk(),
            "ord",
            10,
        );

        let submitted = runtime.run_decision_cycle().await;
        assert!(submitted.is_empty());

        // Still alive and deciding on the next cycle.
        let again = runtime.run_decision_cycle().await;
        assert!(again.is_empty());
    }
}
