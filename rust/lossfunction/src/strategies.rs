//! The three most widely used starter algorithms as deterministic strategies.
//!
//! All operate on completed-bar closes from `MarketSnapshot.history`
//! (oldest first); the current live quote is deliberately ignored so every
//! signal is reproducible from stored bars alone. Sells only fire when a
//! position is actually held.

use std::collections::{BTreeMap, HashMap};

use rust_decimal::Decimal;

use crate::history::sma;
use crate::strategy::{MarketSnapshot, OrderIntent, Strategy, StrategyDecision};
use crate::types::{OrderSide, OrderType, Price, Quantity, Symbol};

fn held_shares(snapshot: &MarketSnapshot, symbol: &Symbol) -> i64 {
    snapshot
        .positions
        .get(symbol)
        .map(|position| position.quantity)
        .unwrap_or(0)
}

fn intent(symbol: &Symbol, side: OrderSide, quantity: Quantity) -> OrderIntent {
    OrderIntent {
        symbol: symbol.clone(),
        side,
        order_type: OrderType::Market,
        quantity,
        limit_price: None,
    }
}

fn decision(
    name: &str,
    intents: Vec<OrderIntent>,
    features: BTreeMap<String, String>,
) -> StrategyDecision {
    StrategyDecision {
        strategy_name: name.to_string(),
        strategy_version: "1".to_string(),
        rationale: if intents.is_empty() {
            "no signals".to_string()
        } else {
            format!("{name} signals")
        },
        intents,
        features,
    }
}

/// SMA crossover (golden/dead cross): buy when the fast average crosses
/// above the slow one, sell the position when it crosses below.
pub struct SmaCrossStrategy {
    symbols: Vec<Symbol>,
    fast_period: usize,
    slow_period: usize,
    quantity: Quantity,
}

impl SmaCrossStrategy {
    pub fn new(
        symbols: Vec<Symbol>,
        fast_period: usize,
        slow_period: usize,
        quantity: Quantity,
    ) -> Self {
        assert!(
            fast_period < slow_period,
            "fast period must be < slow period"
        );
        Self {
            symbols,
            fast_period,
            slow_period,
            quantity,
        }
    }

    fn cross(closes: &[Price], fast: usize, slow: usize) -> Option<(bool, bool)> {
        if closes.len() < slow + 1 {
            return None;
        }
        let prefix = &closes[..closes.len() - 1];
        let fast_now = sma(closes, fast)?;
        let slow_now = sma(closes, slow)?;
        let fast_prev = sma(prefix, fast)?;
        let slow_prev = sma(prefix, slow)?;
        Some((
            fast_prev <= slow_prev && fast_now > slow_now, // golden cross
            fast_prev >= slow_prev && fast_now < slow_now, // dead cross
        ))
    }
}

impl Strategy for SmaCrossStrategy {
    fn name(&self) -> &str {
        "sma-cross"
    }
    fn version(&self) -> &str {
        "1"
    }

    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let mut intents = Vec::new();
        let mut features = BTreeMap::new();
        for symbol in &self.symbols {
            let Some(closes) = snapshot.history.get(symbol) else {
                continue;
            };
            if let Some((golden, dead)) = Self::cross(closes, self.fast_period, self.slow_period) {
                let held = held_shares(snapshot, symbol);
                if golden && held == 0 {
                    intents.push(intent(symbol, OrderSide::Buy, self.quantity));
                } else if dead && held > 0 {
                    intents.push(intent(symbol, OrderSide::Sell, held.min(self.quantity)));
                }
                features.insert(format!("{symbol}:golden"), golden.to_string());
                features.insert(format!("{symbol}:dead"), dead.to_string());
            }
            features.insert(format!("{symbol}:bars"), closes.len().to_string());
        }
        decision(self.name(), intents, features)
    }
}

/// Cutler's RSI (simple-average based — exact Decimal, no smoothing state).
pub fn rsi(closes: &[Price], period: usize) -> Option<Decimal> {
    if period == 0 || closes.len() < period + 1 {
        return None;
    }
    let tail = &closes[closes.len() - period - 1..];
    let mut gains = Decimal::ZERO;
    let mut losses = Decimal::ZERO;
    for pair in tail.windows(2) {
        let change = pair[1] - pair[0];
        if change > Decimal::ZERO {
            gains += change;
        } else {
            losses -= change;
        }
    }
    let avg_gain = gains / Decimal::from(period);
    let avg_loss = losses / Decimal::from(period);
    Some(match (avg_gain, avg_loss) {
        (gain, loss) if gain.is_zero() && loss.is_zero() => Decimal::from(50),
        (_, loss) if loss.is_zero() => Decimal::from(100),
        (gain, loss) => {
            let rs = gain / loss;
            Decimal::from(100) - Decimal::from(100) / (Decimal::ONE + rs)
        }
    })
}

/// RSI mean reversion: buy oversold, sell overbought (when held).
pub struct RsiReversionStrategy {
    symbols: Vec<Symbol>,
    period: usize,
    oversold: Decimal,
    overbought: Decimal,
    quantity: Quantity,
}

impl RsiReversionStrategy {
    pub fn new(
        symbols: Vec<Symbol>,
        period: usize,
        oversold: Decimal,
        overbought: Decimal,
        quantity: Quantity,
    ) -> Self {
        Self {
            symbols,
            period,
            oversold,
            overbought,
            quantity,
        }
    }
}

impl Strategy for RsiReversionStrategy {
    fn name(&self) -> &str {
        "rsi-reversion"
    }
    fn version(&self) -> &str {
        "1"
    }

    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let mut intents = Vec::new();
        let mut features = BTreeMap::new();
        for symbol in &self.symbols {
            let Some(closes) = snapshot.history.get(symbol) else {
                continue;
            };
            let Some(value) = rsi(closes, self.period) else {
                continue;
            };
            features.insert(format!("{symbol}:rsi"), value.round_dp(2).to_string());
            let held = held_shares(snapshot, symbol);
            if value < self.oversold && held == 0 {
                intents.push(intent(symbol, OrderSide::Buy, self.quantity));
            } else if value > self.overbought && held > 0 {
                intents.push(intent(symbol, OrderSide::Sell, held.min(self.quantity)));
            }
        }
        decision(self.name(), intents, features)
    }
}

/// Donchian breakout (turtle entry): buy on close above the prior N-bar
/// high, exit on close below the prior N-bar low.
pub struct DonchianBreakoutStrategy {
    symbols: Vec<Symbol>,
    period: usize,
    quantity: Quantity,
}

impl DonchianBreakoutStrategy {
    pub fn new(symbols: Vec<Symbol>, period: usize, quantity: Quantity) -> Self {
        Self {
            symbols,
            period,
            quantity,
        }
    }
}

impl Strategy for DonchianBreakoutStrategy {
    fn name(&self) -> &str {
        "donchian-breakout"
    }
    fn version(&self) -> &str {
        "1"
    }

    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let mut intents = Vec::new();
        let mut features = BTreeMap::new();
        for symbol in &self.symbols {
            let Some(closes) = snapshot.history.get(symbol) else {
                continue;
            };
            if closes.len() < self.period + 1 {
                continue;
            }
            let last = closes[closes.len() - 1];
            let prior = &closes[closes.len() - 1 - self.period..closes.len() - 1];
            let prior_high = prior.iter().copied().max().expect("non-empty slice");
            let prior_low = prior.iter().copied().min().expect("non-empty slice");
            features.insert(format!("{symbol}:prior_high"), prior_high.to_string());
            features.insert(format!("{symbol}:prior_low"), prior_low.to_string());

            let held = held_shares(snapshot, symbol);
            if last > prior_high && held == 0 {
                intents.push(intent(symbol, OrderSide::Buy, self.quantity));
            } else if last < prior_low && held > 0 {
                intents.push(intent(symbol, OrderSide::Sell, held.min(self.quantity)));
            }
        }
        decision(self.name(), intents, features)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::portfolio::PositionState;
    use chrono::Utc;
    use std::str::FromStr;

    fn symbol() -> Symbol {
        Symbol::parse("005930").unwrap()
    }

    fn snapshot(closes: &[i64], held: i64) -> MarketSnapshot {
        let mut history = HashMap::new();
        history.insert(
            symbol(),
            closes.iter().map(|value| Decimal::from(*value)).collect(),
        );
        let mut positions = HashMap::new();
        if held > 0 {
            positions.insert(
                symbol(),
                PositionState {
                    symbol: symbol(),
                    quantity: held,
                    average_price: Decimal::from(100),
                    realized_pnl: Decimal::ZERO,
                },
            );
        }
        let mut quotes = HashMap::new();
        quotes.insert(
            symbol(),
            crate::types::Quote {
                symbol: symbol(),
                last_price: Decimal::from(*closes.last().unwrap_or(&0)),
                timestamp: Utc::now(),
            },
        );
        MarketSnapshot {
            quotes,
            positions,
            as_of: "t".into(),
            history,
        }
    }

    fn decimals(values: &[i64]) -> Vec<Price> {
        values.iter().map(|v| Decimal::from(*v)).collect()
    }

    #[test]
    fn golden_cross_buys_and_dead_cross_sells() {
        // Flat run, then one spike bar: fast SMA(3) crosses above slow(18).
        let mut lifted: Vec<i64> = vec![100; 20];
        lifted.push(300);
        let strategy = SmaCrossStrategy::new(vec![symbol()], 3, 18, 10);
        let buy = strategy.decide(&snapshot(&lifted, 0));
        assert_eq!(buy.intents.len(), 1);
        assert_eq!(buy.intents[0].side, OrderSide::Buy);
        assert_eq!(buy.features["005930:golden"], "true");

        // Uptrend (fast above slow), then a crash bar: dead cross -> sell.
        let mut collapsed: Vec<i64> = vec![100; 20];
        collapsed.extend([150, 150, 150, 150, 150, 150, 30]);
        let sell = strategy.decide(&snapshot(&collapsed, 10));
        assert_eq!(sell.intents.len(), 1);
        assert_eq!(sell.intents[0].side, OrderSide::Sell);

        // Determinism: identical input -> identical decision.
        assert_eq!(strategy.decide(&snapshot(&lifted, 0)), buy);
    }

    #[test]
    fn rsi_buys_oversold_sells_overbought() {
        // Steep decline then flat: losses dominate -> low RSI.
        let declining: Vec<i64> = (0..25).map(|i| 200 - (i as i64) * 6).collect();
        let oversold =
            RsiReversionStrategy::new(vec![symbol()], 14, Decimal::from(30), Decimal::from(70), 10);
        let buy = oversold.decide(&snapshot(&declining, 0));
        assert_eq!(buy.intents.len(), 1, "rsi={}", buy.features["005930:rsi"]);
        assert_eq!(buy.intents[0].side, OrderSide::Buy);

        // Rally while holding -> overbought sell.
        let rising: Vec<i64> = (0..25).map(|i| 100 + (i as i64) * 6).collect();
        let sell = oversold.decide(&snapshot(&rising, 10));
        assert_eq!(sell.intents.len(), 1);
        assert_eq!(sell.intents[0].side, OrderSide::Sell);

        // RSI itself is exact and matches the definition on a small series.
        let value = rsi(&decimals(&[100, 110, 105, 120, 90]), 4).unwrap();
        // gains: 10+15=25 avg 6.25; losses: 5+30=35 avg 8.75; RS=6.25/8.75
        let expected = Decimal::from(100)
            - Decimal::from(100)
                / (Decimal::ONE
                    + Decimal::from_str("6.25").unwrap() / Decimal::from_str("8.75").unwrap());
        assert_eq!(value, expected);
    }

    #[test]
    fn donchian_breaks_out_and_exits() {
        let mut ranging: Vec<i64> = vec![100, 102, 98, 101, 99, 103, 97, 100];
        ranging.push(140); // close above the prior 8-bar high (~103)
        let breakout = DonchianBreakoutStrategy::new(vec![symbol()], 8, 10);
        let buy = breakout.decide(&snapshot(&ranging, 0));
        assert_eq!(buy.intents.len(), 1);
        assert_eq!(buy.intents[0].side, OrderSide::Buy);

        let mut falling = ranging;
        falling.push(60); // below the prior 8-bar low
        let sell = breakout.decide(&snapshot(&falling, 10));
        assert_eq!(sell.intents.len(), 1);
        assert_eq!(sell.intents[0].side, OrderSide::Sell);

        // No breakout, no signal.
        let flat = vec![100; 10];
        assert!(breakout.decide(&snapshot(&flat, 0)).intents.is_empty());
    }

    #[test]
    fn sells_only_when_held() {
        let declining: Vec<i64> = (0..25).map(|i| 200 - (i as i64) * 6).collect();
        let strategy =
            RsiReversionStrategy::new(vec![symbol()], 14, Decimal::from(30), Decimal::from(70), 10);
        // No position and overbought: nothing to sell.
        let rising: Vec<i64> = (0..25).map(|i| 100 + (i as i64) * 6).collect();
        assert!(strategy.decide(&snapshot(&rising, 0)).intents.is_empty());
        // Held and oversold: no duplicate buy.
        assert!(strategy.decide(&snapshot(&declining, 5)).intents.is_empty());
    }
}
