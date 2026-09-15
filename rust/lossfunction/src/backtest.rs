//! Backtesting engine — event simulation over historical bars.
//!
//! The feed is strictly sequential: a bar becomes visible only when the
//! cursor reaches it, and strategies receive completed closes through the
//! same history-window contract as live wiring — look-ahead access is a
//! raised error, not a discipline. Costs (commission both sides, securities
//! tax on sells) enter exact-Decimal cash accounting per bar.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::broker::{limit_crosses, Broker, OrderRequest};
use crate::history::HistoryWindow;
use crate::risk::RiskManager;
use crate::strategy::{MarketSnapshot, OrderIntent, Strategy};
use crate::types::{OrderSide, Price, Quantity, Quote, Symbol};

/// One historical bar (daily or intraday — the engine is agnostic).
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub symbol: Symbol,
    pub timestamp: DateTime<Utc>,
    pub close: Price,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BacktestConfig {
    pub initial_cash: Decimal,
    pub commission_rate: Decimal,
    pub tax_rate: Decimal,
    pub history_capacity: usize,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            initial_cash: Decimal::from(100_000_000),
            commission_rate: Decimal::from_str_exact("0.00015").unwrap(), // 0.015%
            tax_rate: Decimal::from_str_exact("0.0015").unwrap(),         // 0.15%, sells
            history_capacity: 120,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FillRecord {
    pub client_order_id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub price: Price,
    pub commission: Decimal,
    pub tax: Decimal,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BacktestResult {
    pub cash: Decimal,
    pub total_commission: Decimal,
    pub total_tax: Decimal,
    pub equity: Decimal,
    pub equity_curve: Vec<(DateTime<Utc>, Decimal)>,
    pub fills: Vec<FillRecord>,
}

#[derive(Debug, thiserror::Error)]
#[error("index {index} is at or beyond cursor {cursor}")]
pub struct LookaheadError {
    pub index: usize,
    pub cursor: usize,
}

/// Sequential bar cursor; future bars are unreachable by construction.
pub struct BacktestFeed {
    bars: Vec<Bar>,
    cursor: usize,
}

impl BacktestFeed {
    pub fn new(mut bars: Vec<Bar>) -> Self {
        bars.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        Self { bars, cursor: 0 }
    }

    pub fn has_next(&self) -> bool {
        self.cursor < self.bars.len()
    }

    pub fn advance(&mut self) -> Option<Bar> {
        let bar = self.bars.get(self.cursor).cloned()?;
        self.cursor += 1;
        Some(bar)
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    #[cfg(test)]
    fn bars_first(&self) -> Price {
        self.bars[0].close
    }

    /// Only past/current bars are addressable (look-ahead guard).
    pub fn bar_at(&self, index: usize) -> Result<Bar, LookaheadError> {
        if index >= self.cursor {
            return Err(LookaheadError {
                index,
                cursor: self.cursor,
            });
        }
        Ok(self.bars[index].clone())
    }
}

/// Runs a strategy over bars with fees, tax, and risk checks.
type BarHook = std::sync::Arc<dyn Fn(&Bar) + Send + Sync>;

pub struct BacktestEngine {
    strategy: Box<dyn Strategy>,
    config: BacktestConfig,
    risk: Option<std::sync::Arc<RiskManager>>,
    /// Invoked with each consumed bar — callers wire venue prices (the
    /// mock broker fills against configured prices).
    bar_hook: Option<BarHook>,
}

impl BacktestEngine {
    pub fn new(strategy: Box<dyn Strategy>, config: BacktestConfig) -> Self {
        Self {
            strategy,
            config,
            risk: None,
            bar_hook: None,
        }
    }

    pub fn with_bar_hook(mut self, hook: BarHook) -> Self {
        self.bar_hook = Some(hook);
        self
    }

    pub fn with_risk(
        strategy: Box<dyn Strategy>,
        config: BacktestConfig,
        risk: std::sync::Arc<RiskManager>,
    ) -> Self {
        Self {
            strategy,
            config,
            risk: Some(risk),
            bar_hook: None,
        }
    }

    pub async fn run(&self, bars: Vec<Bar>, broker: &dyn Broker) -> BacktestResult {
        let mut feed = BacktestFeed::new(bars);
        let mut history = HistoryWindow::new(self.config.history_capacity);
        let mut cash = self.config.initial_cash;
        let mut fills: Vec<FillRecord> = Vec::new();
        let mut equity_curve = Vec::new();
        let mut live_quotes: HashMap<Symbol, Quote> = HashMap::new();
        let mut order_seq = 0u64;
        let mut portfolio = crate::domain::portfolio::Portfolio::new();

        while let Some(bar) = feed.advance() {
            if let Some(hook) = &self.bar_hook {
                hook(&bar);
            }
            live_quotes.insert(
                bar.symbol.clone(),
                Quote {
                    symbol: bar.symbol.clone(),
                    last_price: bar.close,
                    timestamp: bar.timestamp,
                },
            );

            let snapshot = MarketSnapshot {
                quotes: live_quotes.clone(),
                positions: portfolio
                    .positions()
                    .map(|position| (position.symbol.clone(), position.clone()))
                    .collect(),
                as_of: bar.timestamp.to_rfc3339(),
                history: history.snapshot(),
            };
            let decision = self.strategy.decide(&snapshot);

            for intent in decision.intents {
                let order = self.build_request(&mut order_seq, &intent);
                let notional = self.preflight(&order, &portfolio, &live_quotes);
                let Some(_notional) = notional else {
                    continue;
                };
                if let Ok(ack) = broker.submit_order(&order).await {
                    if let Ok(report) = broker.execution_report(&ack.broker_order_id).await {
                        if report.filled_quantity > 0 {
                            let price = report.average_fill_price.unwrap_or(bar.close);
                            let filled = report.filled_quantity;
                            let fill_notional = Decimal::from(filled) * price;
                            let commission = fill_notional * self.config.commission_rate;
                            let tax = if order.side == OrderSide::Sell {
                                fill_notional * self.config.tax_rate
                            } else {
                                Decimal::ZERO
                            };
                            cash = if order.side == OrderSide::Buy {
                                cash - fill_notional - commission
                            } else {
                                cash + fill_notional - commission - tax
                            };
                            let fill = crate::domain::portfolio::Fill {
                                client_order_id: order.client_order_id.clone(),
                                symbol: order.symbol.clone(),
                                side: order.side,
                                quantity: filled,
                                price,
                            };
                            if let Ok(next) = portfolio.apply_fill(fill.clone()) {
                                portfolio = next;
                            }
                            fills.push(FillRecord {
                                client_order_id: order.client_order_id.clone(),
                                symbol: order.symbol.clone(),
                                side: order.side,
                                quantity: filled,
                                price,
                                commission,
                                tax,
                                timestamp: bar.timestamp,
                            });
                        }
                    }
                }
            }

            // The completed bar enters the history only after this bar's
            // decision — strategies never see the close they just traded on.
            history.push(&bar.symbol, bar.close);

            let open_value: Decimal = portfolio
                .positions()
                .filter(|position| position.quantity > 0)
                .map(|position| {
                    let price = live_quotes
                        .get(&position.symbol)
                        .map(|quote| quote.last_price)
                        .unwrap_or(position.average_price);
                    Decimal::from(position.quantity) * price
                })
                .sum();
            equity_curve.push((bar.timestamp, cash + open_value));
        }

        BacktestResult {
            cash,
            total_commission: fills.iter().map(|f| f.commission).sum(),
            total_tax: fills.iter().map(|f| f.tax).sum(),
            equity: equity_curve
                .last()
                .map(|(_, equity)| *equity)
                .unwrap_or(self.config.initial_cash),
            equity_curve,
            fills,
        }
    }

    fn build_request(&self, order_seq: &mut u64, intent: &OrderIntent) -> OrderRequest {
        *order_seq += 1;
        OrderRequest {
            client_order_id: format!("bt-{:04}", order_seq),
            symbol: intent.symbol.clone(),
            side: intent.side,
            order_type: intent.order_type,
            quantity: intent.quantity,
            limit_price: intent.limit_price,
        }
    }

    fn preflight(
        &self,
        order: &OrderRequest,
        portfolio: &crate::domain::portfolio::Portfolio,
        quotes: &HashMap<Symbol, Quote>,
    ) -> Option<Decimal> {
        if let Some(risk) = &self.risk {
            let domain_order = crate::domain::order::Order::new(
                order.client_order_id.clone(),
                order.symbol.clone(),
                order.side,
                order.order_type,
                order.quantity,
                order.limit_price,
            )
            .ok()?;
            risk.check_order(&domain_order, portfolio, quotes).ok()
        } else {
            Some(Decimal::ZERO)
        }
    }
}

/// Replay helper for tests/demo: the crossing rule mirrors the mock broker.
#[allow(dead_code)]
pub fn bar_fills_limit(side: OrderSide, market: Decimal, limit: Decimal) -> bool {
    limit_crosses(side, market, limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::mock::MockBroker;
    use crate::strategy::OrderIntent;
    use crate::strategy::StrategyDecision;
    use crate::types::OrderType;
    use chrono::TimeZone;

    fn symbol() -> Symbol {
        Symbol::parse("005930").unwrap()
    }

    fn bars(closes: &[i64]) -> Vec<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, close)| Bar {
                symbol: symbol(),
                timestamp: Utc.timestamp_opt(1_700_000_000 + i as i64, 0).unwrap(),
                close: Decimal::from(*close),
            })
            .collect()
    }

    /// Buys on the first snapshot, sells on the second.
    struct BuyOnceSellOnce {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl Strategy for BuyOnceSellOnce {
        fn name(&self) -> &str {
            "buy-once"
        }
        fn version(&self) -> &str {
            "1"
        }
        fn decide(&self, _snapshot: &MarketSnapshot) -> StrategyDecision {
            use std::sync::atomic::Ordering;
            let calls = self.calls.fetch_add(1, Ordering::SeqCst);
            let intents = match calls {
                0 => vec![OrderIntent {
                    symbol: symbol(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: 10,
                    limit_price: None,
                }],
                1 => vec![OrderIntent {
                    symbol: symbol(),
                    side: OrderSide::Sell,
                    order_type: OrderType::Market,
                    quantity: 10,
                    limit_price: None,
                }],
                _ => vec![],
            };
            StrategyDecision {
                strategy_name: "buy-once".into(),
                strategy_version: "1".into(),
                intents,
                features: Default::default(),
                rationale: String::new(),
            }
        }
    }

    #[test]
    fn feed_rejects_future_access_and_sorts() {
        let series = bars(&[1, 2, 3]);
        let mut feed = BacktestFeed::new(series);
        feed.advance().unwrap();
        assert!(feed.bar_at(1).is_err());
        assert!(feed.bar_at(2).is_err());
        assert_eq!(feed.bar_at(0).unwrap().close, Decimal::from(1));

        // Reverse in time (later timestamp first): the feed must sort by
        // timestamp, so the earliest close comes first.
        let mut time_reversed = bars(&[1, 3]);
        time_reversed.reverse();
        let feed = BacktestFeed::new(time_reversed);
        assert_eq!(feed.bars_first(), Decimal::from(1)); // sorted by timestamp
    }

    #[tokio::test]
    async fn costs_reflected_in_equity() {
        let config = BacktestConfig {
            initial_cash: Decimal::from(1_000_000),
            commission_rate: Decimal::from_str_exact("0.001").unwrap(),
            tax_rate: Decimal::from_str_exact("0.01").unwrap(),
            history_capacity: 10,
        };
        let strategy = Box::new(BuyOnceSellOnce {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let engine = BacktestEngine::new(strategy, config.clone());
        let priced_broker = std::sync::Arc::new(MockBroker::new());
        let hook_broker = std::sync::Arc::clone(&priced_broker);
        let engine = engine.with_bar_hook(std::sync::Arc::new(move |bar: &Bar| {
            hook_broker.set_price(&bar.symbol, bar.close);
        }));

        let result = engine
            .run(bars(&[80_000, 90_000, 90_000]), priced_broker.as_ref())
            .await;

        let buy_notional = Decimal::from(10) * Decimal::from(80_000);
        let sell_notional = Decimal::from(10) * Decimal::from(90_000);
        let expected_commission = buy_notional * Decimal::from_str_exact("0.001").unwrap()
            + sell_notional * Decimal::from_str_exact("0.001").unwrap();
        let expected_tax = sell_notional * Decimal::from_str_exact("0.01").unwrap();

        assert_eq!(result.total_commission, expected_commission);
        assert_eq!(result.total_tax, expected_tax);
        assert_eq!(
            result.cash,
            config.initial_cash
                - buy_notional
                - buy_notional * Decimal::from_str_exact("0.001").unwrap()
                + sell_notional
                - sell_notional * Decimal::from_str_exact("0.001").unwrap()
                - expected_tax
        );
        assert_eq!(result.equity, result.cash); // flat at the end
        assert_eq!(result.fills.len(), 2);
    }

    #[tokio::test]
    async fn unfilled_limit_orders_cost_nothing() {
        struct Resting;
        impl Strategy for Resting {
            fn name(&self) -> &str {
                "resting"
            }
            fn version(&self) -> &str {
                "1"
            }
            fn decide(&self, _: &MarketSnapshot) -> StrategyDecision {
                StrategyDecision {
                    strategy_name: "resting".into(),
                    strategy_version: "1".into(),
                    intents: vec![OrderIntent {
                        symbol: symbol(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Limit,
                        quantity: 10,
                        limit_price: Some(Decimal::from(1_000)), // never crossed
                    }],
                    features: Default::default(),
                    rationale: String::new(),
                }
            }
        }
        let broker = std::sync::Arc::new(MockBroker::new());
        let hook_broker = std::sync::Arc::clone(&broker);
        let engine = BacktestEngine::new(Box::new(Resting), BacktestConfig::default())
            .with_bar_hook(std::sync::Arc::new(move |bar: &Bar| {
                hook_broker.set_price(&bar.symbol, bar.close);
            }));
        let result = engine.run(bars(&[80_000, 80_000]), broker.as_ref()).await;
        assert!(result.fills.is_empty());
        assert_eq!(result.total_commission, Decimal::ZERO);
        assert_eq!(result.cash, BacktestConfig::default().initial_cash);
    }

    #[tokio::test]
    async fn same_inputs_reproduce_identical_results() {
        let run = || async {
            let strategy = Box::new(BuyOnceSellOnce {
                calls: std::sync::atomic::AtomicUsize::new(0),
            });
            let broker = std::sync::Arc::new(MockBroker::new());
            let hook_broker = std::sync::Arc::clone(&broker);
            let engine = BacktestEngine::new(strategy, BacktestConfig::default()).with_bar_hook(
                std::sync::Arc::new(move |bar: &Bar| {
                    hook_broker.set_price(&bar.symbol, bar.close);
                }),
            );
            engine.run(bars(&[10, 5, 20, 15]), broker.as_ref()).await
        };
        let first = run().await;
        let second = run().await;
        assert_eq!(first, second);
    }
}
