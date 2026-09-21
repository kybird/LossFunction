//! Strategy layer — deterministic decision interface.
//!
//! A strategy is a pure function of the market snapshot: same snapshot in,
//! same decision out — no clocks, no randomness, no I/O. The decision layer
//! wraps a strategy and records every decision (feature digest + signals)
//! so any signal can be reproduced and audited after the fact.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use crate::domain::portfolio::PositionState;
use crate::types::{OrderSide, OrderType, Price, Quantity, Quote, Symbol};

/// Everything a strategy is allowed to see for one decision.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketSnapshot {
    pub quotes: HashMap<Symbol, Quote>,
    pub positions: HashMap<Symbol, PositionState>,
    /// Opaque session label; ordering is handled upstream.
    pub as_of: String,
    /// Completed past-bar closes per symbol (oldest first) — the only past
    /// data strategies may see; look-ahead is impossible by construction.
    pub history: HashMap<Symbol, Vec<Price>>,
}

/// A desired trade, before risk checks and id assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderIntent {
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub limit_price: Option<Price>,
}

/// The output of one strategy invocation, fully self-describing.
#[derive(Debug, Clone, PartialEq)]
pub struct StrategyDecision {
    pub strategy_name: String,
    pub strategy_version: String,
    pub intents: Vec<OrderIntent>,
    /// The exact inputs the signals derive from.
    pub features: BTreeMap<String, String>,
    pub rationale: String,
}

/// Deterministic signal generator.
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision;
}

type DecisionCallback = Box<dyn Fn(&StrategyDecision, &MarketSnapshot) + Send + Sync>;

/// Runs a strategy and records every decision for audit/replay.
pub struct DecisionLayer {
    strategy: Box<dyn Strategy>,
    on_decision: Option<DecisionCallback>,
    decisions: Mutex<Vec<StrategyDecision>>,
}

impl DecisionLayer {
    pub fn new(strategy: Box<dyn Strategy>) -> Self {
        Self {
            strategy,
            on_decision: None,
            decisions: Mutex::new(Vec::new()),
        }
    }

    pub fn with_recording(
        strategy: Box<dyn Strategy>,
        on_decision: impl Fn(&StrategyDecision, &MarketSnapshot) + Send + Sync + 'static,
    ) -> Self {
        Self {
            strategy,
            on_decision: Some(Box::new(on_decision)),
            decisions: Mutex::new(Vec::new()),
        }
    }

    pub fn strategy_version(&self) -> &str {
        self.strategy.version()
    }

    pub fn strategy_name(&self) -> &str {
        self.strategy.name()
    }

    pub fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let decision = self.strategy.decide(snapshot);
        self.decisions
            .lock()
            .expect("decision log lock")
            .push(decision.clone());
        if let Some(on_decision) = &self.on_decision {
            on_decision(&decision, snapshot);
        }
        decision
    }

    pub fn decisions(&self) -> Vec<StrategyDecision> {
        self.decisions.lock().expect("decision log lock").clone()
    }
}

/// Example deterministic strategy — entry-price starter.
///
/// Buys a fixed quantity the first time a symbol trades at or below its
/// configured entry price (no position yet).
pub struct EntryPriceStrategy {
    entry_prices: BTreeMap<Symbol, Price>,
    quantity: Quantity,
}

impl EntryPriceStrategy {
    pub fn new(entry_prices: BTreeMap<Symbol, Price>, quantity: Quantity) -> Self {
        Self {
            entry_prices,
            quantity,
        }
    }
}

impl Strategy for EntryPriceStrategy {
    fn name(&self) -> &str {
        "entry-price"
    }

    fn version(&self) -> &str {
        "1"
    }

    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let mut intents = Vec::new();
        let mut features = BTreeMap::new();
        for (symbol, entry) in &self.entry_prices {
            let Some(quote) = snapshot.quotes.get(symbol) else {
                features.insert(symbol.to_string(), "no-quote".to_string());
                continue;
            };
            features.insert(format!("{symbol}:price"), quote.last_price.to_string());
            features.insert(format!("{symbol}:entry"), entry.to_string());
            let held = snapshot
                .positions
                .get(symbol)
                .map(|position| position.quantity)
                .unwrap_or(0);
            features.insert(format!("{symbol}:held"), held.to_string());
            if held == 0 && quote.last_price <= *entry {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                    quantity: self.quantity,
                    limit_price: Some(quote.last_price),
                });
            }
        }
        StrategyDecision {
            strategy_name: self.name().to_string(),
            strategy_version: self.version().to_string(),
            rationale: if intents.is_empty() {
                "no entry signals".to_string()
            } else {
                "buy symbols at/below entry with no existing position".to_string()
            },
            intents,
            features,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn snapshot() -> MarketSnapshot {
        let mut quotes = HashMap::new();
        for (code, price) in [("005930", 79_000i64), ("035420", 42_000)] {
            let symbol = Symbol::parse(code).unwrap();
            quotes.insert(
                symbol.clone(),
                Quote {
                    symbol,
                    last_price: Decimal::from(price),
                    timestamp: Utc::now(),
                },
            );
        }
        MarketSnapshot {
            quotes,
            positions: HashMap::new(),
            as_of: "session-1".to_string(),
            history: HashMap::new(),
        }
    }

    fn strategy() -> EntryPriceStrategy {
        EntryPriceStrategy::new(
            [
                (Symbol::parse("005930").unwrap(), Decimal::from(80_000)),
                (Symbol::parse("035420").unwrap(), Decimal::from(41_000)),
            ]
            .into_iter()
            .collect(),
            5,
        )
    }

    #[test]
    fn buys_at_entry_and_records_features() {
        let decision = strategy().decide(&snapshot());
        assert_eq!(decision.strategy_name, "entry-price");
        assert_eq!(decision.intents.len(), 1);
        let intent = &decision.intents[0];
        assert_eq!(intent.symbol.as_str(), "005930"); // 79000 <= 80000
        assert_eq!(intent.limit_price, Some(Decimal::from(79_000)));
        // 035420 at 42000 > 41000 entry -> no signal, but features recorded.
        assert_eq!(
            decision.features.get("035420:price").map(String::as_str),
            Some("42000")
        );
        assert_eq!(
            decision.features.get("035420:entry").map(String::as_str),
            Some("41000")
        );
    }

    #[test]
    fn same_snapshot_yields_identical_decisions() {
        let first = strategy().decide(&snapshot());
        let second = strategy().decide(&snapshot());
        assert_eq!(first, second); // full structural equality

        let layer = DecisionLayer::new(Box::new(strategy()));
        assert_eq!(layer.decide(&snapshot()), layer.decide(&snapshot()));
    }

    #[test]
    fn skips_held_symbols_and_missing_quotes() {
        let symbol = Symbol::parse("005930").unwrap();
        let mut held = snapshot();
        held.positions.insert(
            symbol.clone(),
            PositionState {
                symbol: symbol.clone(),
                quantity: 3,
                average_price: Decimal::from(78_000),
                realized_pnl: Decimal::ZERO,
            },
        );
        let decision = strategy().decide(&held);
        assert!(decision.intents.is_empty());
        assert_eq!(
            decision.features.get("005930:held").map(String::as_str),
            Some("3")
        );

        // Watcher tracks only an unquoted symbol — no signal, feature noted.
        let watcher = EntryPriceStrategy::new(
            [(Symbol::parse("999999").unwrap(), Decimal::from(1_000))]
                .into_iter()
                .collect(),
            5,
        );
        let decision = watcher.decide(&snapshot());
        assert!(decision.intents.is_empty());
        assert_eq!(
            decision.features.get("999999").map(String::as_str),
            Some("no-quote")
        );
    }

    #[test]
    fn decision_layer_records_input_and_output() {
        let recorded = Arc::new(AtomicUsize::new(0));
        let counter = {
            let recorded = Arc::clone(&recorded);
            move |_: &StrategyDecision, _: &MarketSnapshot| {
                recorded.fetch_add(1, Ordering::SeqCst);
            }
        };
        let layer = DecisionLayer::with_recording(Box::new(strategy()), counter);
        let decision = layer.decide(&snapshot());
        assert_eq!(recorded.load(Ordering::SeqCst), 1);
        assert_eq!(layer.decisions(), vec![decision]);
    }
}
