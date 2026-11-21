//! Strategy family #3 — presets ported from the official KIS repo
//! (strategy_builder presets), reimplemented on our deterministic,
//! completed-closes-only contract (wiki: quant-strategies):
//!
//! - ConsecutiveRunStrategy — buy after N consecutive up closes (momentum
//!   continuation), sell after N consecutive down closes while held.
//! - StrongCloseStrategy — official preset scores the close within the
//!   day's range; strategies here see completed closes only, so this is
//!   the causal daily proxy: the close's position within the last N days'
//!   close range. Upper zone → buy, lower zone → sell.
//! - VolatilitySqueezeBreakoutStrategy — short-horizon volatility
//!   compressed vs the long horizon AND a breakout of the M-day high →
//!   buy; exit on losing the N-day low while held.

use std::collections::BTreeMap;

use rust_decimal::prelude::Decimal;

use crate::strategy::{MarketSnapshot, OrderIntent, Strategy, StrategyDecision};
use crate::types::{OrderSide, OrderType, Quantity, Symbol};

/// 연속 상승/하락 N일 — momentum continuation on close runs.
pub struct ConsecutiveRunStrategy {
    symbols: Vec<Symbol>,
    run_length: usize,
    quantity: Quantity,
}

impl ConsecutiveRunStrategy {
    pub fn new(symbols: Vec<Symbol>, run_length: usize, quantity: Quantity) -> Self {
        Self {
            symbols,
            run_length: run_length.max(1),
            quantity,
        }
    }

    /// Length of the trailing same-direction run ending at the last close,
    /// with its direction (+1 up / -1 down / 0 flat tail).
    fn trailing_run(closes: &[Decimal]) -> (i32, usize) {
        if closes.len() < 2 {
            return (0, 0);
        }
        let direction = if closes[closes.len() - 1] > closes[closes.len() - 2] {
            1
        } else if closes[closes.len() - 1] < closes[closes.len() - 2] {
            -1
        } else {
            return (0, 0);
        };
        // Count matching STEPS (rises/falls), not bars: "3일 연속 상승"
        // means three consecutive rising days.
        let mut run = 0;
        for pair in closes.windows(2).rev() {
            let (prev, cur) = (pair[0], pair[1]);
            let step = if cur > prev {
                1
            } else if cur < prev {
                -1
            } else {
                0
            };
            if step == direction {
                run += 1;
            } else {
                break;
            }
        }
        (direction, run)
    }
}

impl Strategy for ConsecutiveRunStrategy {
    fn name(&self) -> &str {
        "consecutive-run"
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
            let (direction, run) = Self::trailing_run(closes);
            features.insert(format!("{symbol}:run_dir"), direction.to_string());
            features.insert(format!("{symbol}:run_len"), run.to_string());
            let held = snapshot
                .positions
                .get(symbol)
                .map(|position| position.quantity)
                .unwrap_or(0);
            if direction == 1 && run >= self.run_length && held == 0 {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: self.quantity,
                    limit_price: None,
                });
            } else if direction == -1 && run >= self.run_length && held > 0 {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Sell,
                    order_type: OrderType::Market,
                    quantity: held,
                    limit_price: None,
                });
            }
        }
        // Per-trade numbers — the rationale must say why THIS trade.
        let mut reasons: Vec<String> = intents
            .iter()
            .map(|intent| {
                let kind = if intent.side == OrderSide::Buy {
                    "연속상승"
                } else {
                    "연속하락"
                };
                format!(
                    "{} {} {}일 흐름 · {} → {}",
                    intent.symbol,
                    kind,
                    self.run_length,
                    last_of(snapshot, &intent.symbol),
                    side_qty(intent)
                )
            })
            .collect();
        reasons.sort();
        StrategyDecision {
            strategy_name: self.name().to_string(),
            strategy_version: self.version().to_string(),
            rationale: if intents.is_empty() {
                "연속 흐름 신호 없음".to_string()
            } else {
                reasons.join("; ")
            },
            intents,
            features,
        }
    }
}

fn last_of(snapshot: &MarketSnapshot, symbol: &Symbol) -> String {
    snapshot
        .history
        .get(symbol)
        .and_then(|closes| closes.last())
        .map(|close| close.round_dp(0).to_string())
        .unwrap_or_else(|| "?".into())
}

fn side_qty(intent: &OrderIntent) -> String {
    if intent.side == OrderSide::Buy {
        format!("{}주 매수", intent.quantity)
    } else {
        format!("{}주 매도", intent.quantity)
    }
}

/// 강한 종가 — the close's position within the recent close range.
pub struct StrongCloseStrategy {
    symbols: Vec<Symbol>,
    lookback: usize,
    upper: Decimal,
    lower: Decimal,
    quantity: Quantity,
}

impl StrongCloseStrategy {
    pub fn new(
        symbols: Vec<Symbol>,
        lookback: usize,
        upper: Decimal,
        lower: Decimal,
        quantity: Quantity,
    ) -> Self {
        Self {
            symbols,
            lookback: lookback.max(2),
            upper,
            lower,
            quantity,
        }
    }

    /// (position 0..1, range_low, range_high) over the last N closes.
    fn position(closes: &[Decimal], lookback: usize) -> Option<(Decimal, Decimal, Decimal)> {
        if closes.len() < lookback {
            return None;
        }
        let tail = &closes[closes.len() - lookback..];
        let low = *tail.iter().min()?;
        let high = *tail.iter().max()?;
        if high == low {
            return None; // no range — no signal
        }
        let last = *closes.last()?;
        Some(((last - low) / (high - low), low, high))
    }
}

impl Strategy for StrongCloseStrategy {
    fn name(&self) -> &str {
        "strong-close"
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
            let Some((position, low, high)) = Self::position(closes, self.lookback) else {
                continue;
            };
            features.insert(
                format!("{symbol}:close_pos"),
                position.round_dp(3).to_string(),
            );
            features.insert(format!("{symbol}:range"), format!("{low}~{high}"));
            let held = snapshot
                .positions
                .get(symbol)
                .map(|position| position.quantity)
                .unwrap_or(0);
            if position >= self.upper && held == 0 {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: self.quantity,
                    limit_price: None,
                });
            } else if position <= self.lower && held > 0 {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Sell,
                    order_type: OrderType::Market,
                    quantity: held,
                    limit_price: None,
                });
            }
        }
        let mut reasons: Vec<String> = intents
            .iter()
            .map(|intent| {
                format!(
                    "{} 종가위치 {} ({} 범위) → {}",
                    intent.symbol,
                    StrongCloseStrategy::position(
                        snapshot.history.get(&intent.symbol).unwrap(),
                        self.lookback
                    )
                    .map(|(position, _, _)| position.round_dp(2).to_string())
                    .unwrap_or_else(|| "?".into()),
                    self.lookback,
                    side_qty(intent)
                )
            })
            .collect();
        reasons.sort();
        StrategyDecision {
            strategy_name: self.name().to_string(),
            strategy_version: self.version().to_string(),
            rationale: if intents.is_empty() {
                "종가 위치 신호 없음".to_string()
            } else {
                reasons.join("; ")
            },
            intents,
            features,
        }
    }
}

/// 변동성 확장 돌파 — volatility squeeze + M-day high breakout.
pub struct VolatilitySqueezeBreakoutStrategy {
    symbols: Vec<Symbol>,
    vol_short: usize,
    vol_long: usize,
    squeeze_ratio: Decimal,
    breakout: usize,
    exit_lookback: usize,
    quantity: Quantity,
}

impl VolatilitySqueezeBreakoutStrategy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        symbols: Vec<Symbol>,
        vol_short: usize,
        vol_long: usize,
        squeeze_ratio: Decimal,
        breakout: usize,
        exit_lookback: usize,
        quantity: Quantity,
    ) -> Self {
        Self {
            symbols,
            vol_short: vol_short.max(2),
            vol_long: vol_long.max(3),
            squeeze_ratio,
            breakout: breakout.max(2),
            exit_lookback: exit_lookback.max(2),
            quantity,
        }
    }

    /// Mean absolute close-to-close change over the N bars BEFORE the last
    /// one (in %) — the squeeze is the setup measured on completed history;
    /// the breakout bar itself must not inflate the short window.
    fn mean_abs_change(closes: &[Decimal], window: usize) -> Option<Decimal> {
        if closes.len() < window + 2 {
            return None;
        }
        let tail = &closes[closes.len() - 1 - window..closes.len() - 1];
        let changes: Vec<Decimal> = tail[1..]
            .iter()
            .zip(&tail[..tail.len() - 1])
            .map(|(cur, prev)| (*cur - *prev).abs() / *prev * Decimal::from(100))
            .collect();
        let sum: Decimal = changes.iter().sum();
        Some(sum / Decimal::from(changes.len()))
    }
}

impl Strategy for VolatilitySqueezeBreakoutStrategy {
    fn name(&self) -> &str {
        "vol-squeeze-breakout"
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
            let Some(last) = closes.last() else { continue };
            // Vol figures gate the BUY side only — the exit path must stay
            // reachable even when history is too short for the long window.
            let short_vol = Self::mean_abs_change(closes, self.vol_short);
            let long_vol = Self::mean_abs_change(closes, self.vol_long);
            // Window requirements are per-condition: the buy needs the
            // breakout window, the exit only the (shorter) exit window.
            let prior_high = (closes.len() > self.breakout).then(|| {
                *closes[closes.len() - 1 - self.breakout..closes.len() - 1]
                    .iter()
                    .max()
                    .expect("non-empty slice")
            });
            let prior_low = (closes.len() > self.exit_lookback).then(|| {
                *closes[closes.len() - 1 - self.exit_lookback..closes.len() - 1]
                    .iter()
                    .min()
                    .expect("non-empty slice")
            });
            let squeezed = match (&short_vol, &long_vol) {
                (Some(short), Some(long)) => *short < *long * self.squeeze_ratio,
                _ => false, // not enough history to certify a squeeze — no buy
            };
            if let Some(short) = short_vol {
                features.insert(format!("{symbol}:vol_short"), short.round_dp(3).to_string());
            }
            if let Some(long) = long_vol {
                features.insert(format!("{symbol}:vol_long"), long.round_dp(3).to_string());
            }
            if let Some(high) = prior_high {
                features.insert(format!("{symbol}:prior_high"), high.round_dp(0).to_string());
            }
            let held = snapshot
                .positions
                .get(symbol)
                .map(|position| position.quantity)
                .unwrap_or(0);
            if squeezed && prior_high.is_some_and(|high| last > &high) && held == 0 {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: self.quantity,
                    limit_price: None,
                });
            } else if held > 0 && prior_low.is_some_and(|low| last < &low) {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Sell,
                    order_type: OrderType::Market,
                    quantity: held,
                    limit_price: None,
                });
            }
        }
        let mut reasons: Vec<String> = intents
            .iter()
            .map(|intent| {
                if intent.side == OrderSide::Buy {
                    format!(
                        "{} 변동성 수축 후 {}일 최고가 {} 돌파 → {}",
                        intent.symbol,
                        self.breakout,
                        last_of(snapshot, &intent.symbol),
                        side_qty(intent)
                    )
                } else {
                    format!(
                        "{} {}일 최저가 이탈 → {}",
                        intent.symbol,
                        self.exit_lookback,
                        side_qty(intent)
                    )
                }
            })
            .collect();
        reasons.sort();
        StrategyDecision {
            strategy_name: self.name().to_string(),
            strategy_version: self.version().to_string(),
            rationale: if intents.is_empty() {
                "변동성 돌파 신호 없음".to_string()
            } else {
                reasons.join("; ")
            },
            intents,
            features,
        }
    }
}

#[cfg(test)]
fn decimal(value: f64) -> Decimal {
    Decimal::from_f64_retain(value).expect("test decimal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::MarketSnapshot;
    use rust_decimal::Decimal;
    use std::collections::HashMap;

    fn symbol() -> Symbol {
        Symbol::parse("005930").unwrap()
    }

    fn snapshot(closes: &[f64], held: i64) -> MarketSnapshot {
        let mut history = HashMap::new();
        history.insert(
            symbol(),
            closes
                .iter()
                .map(|close| Decimal::from_f64_retain(*close).unwrap())
                .collect::<Vec<_>>(),
        );
        let mut positions = HashMap::new();
        if held > 0 {
            positions.insert(
                symbol(),
                crate::domain::portfolio::PositionState {
                    symbol: symbol(),
                    quantity: held,
                    average_price: decimal(closes[0]),
                    realized_pnl: Decimal::ZERO,
                },
            );
        }
        MarketSnapshot {
            history,
            positions,
            ..MarketSnapshot::default()
        }
    }

    fn strategy_of(key: &str) -> Box<dyn Strategy> {
        let one = vec![symbol()];
        match key {
            "consecutive-run" => Box::new(ConsecutiveRunStrategy::new(one, 3, 5)),
            "strong-close" => Box::new(StrongCloseStrategy::new(
                one,
                10,
                decimal(0.8),
                decimal(0.2),
                5,
            )),
            _ => Box::new(VolatilitySqueezeBreakoutStrategy::new(
                one,
                5,
                20,
                decimal(0.6),
                20,
                10,
                5,
            )),
        }
    }

    #[test]
    fn consecutive_run_buys_three_ups_and_is_deterministic() {
        let strategy = strategy_of("consecutive-run");
        let snap = snapshot(&[100.0, 101.0, 102.0, 103.0], 0);
        let decision = strategy.decide(&snap);
        assert_eq!(decision.intents.len(), 1);
        assert_eq!(decision.intents[0].side, OrderSide::Buy);
        assert_eq!(
            decision,
            strategy.decide(&snap),
            "same snapshot must decide identically"
        );
        // Two ups only — no signal.
        let early = snapshot(&[100.0, 101.0, 102.0], 0);
        assert!(strategy_of("consecutive-run")
            .decide(&early)
            .intents
            .is_empty());
        // Held and three downs — sell everything.
        let down = snapshot(&[100.0, 99.0, 98.0, 97.0], 5);
        let sell = strategy_of("consecutive-run").decide(&down);
        assert_eq!(sell.intents.len(), 1);
        assert_eq!(sell.intents[0].side, OrderSide::Sell);
        assert_eq!(sell.intents[0].quantity, 5);
    }

    #[test]
    fn strong_close_buys_upper_zone_sells_lower_zone() {
        // Close at the very top of the 10-day range.
        let range: Vec<f64> = (0..9).map(|i| 100.0 + i as f64).collect();
        let mut top = range.clone();
        top.push(109.5);
        let decision = strategy_of("strong-close").decide(&snapshot(&top, 0));
        assert_eq!(decision.intents.len(), 1);
        assert_eq!(decision.intents[0].side, OrderSide::Buy);
        // Same series but collapsing to the bottom while held — sell.
        let mut bottom = range;
        bottom.push(100.4);
        let sell = strategy_of("strong-close").decide(&snapshot(&bottom, 5));
        assert_eq!(sell.intents.len(), 1);
        assert_eq!(sell.intents[0].side, OrderSide::Sell);
        // Mid-range — nothing.
        let mid: Vec<f64> = vec![
            100.0, 101.0, 102.0, 103.0, 104.0, 106.0, 107.0, 108.0, 109.0, 105.5,
        ];
        assert!(strategy_of("strong-close")
            .decide(&snapshot(&mid, 0))
            .intents
            .is_empty());
    }

    #[test]
    fn vol_squeeze_breakout_requires_both_conditions() {
        // Volatile early bars, then a tight stretch (squeeze), then a
        // breakout close above the prior 20-day high.
        let mut closes: Vec<f64> = Vec::new();
        for i in 0..12 {
            let swing = if i % 2 == 0 { 1.5 } else { -1.5 };
            closes.push(100.0 + swing); // ~1.5% swings — wide long window
        }
        for i in 0..12 {
            closes.push(101.0 + (i as f64) * 0.01); // tight drift — squeeze
        }
        closes.push(103.0); // breakout bar
        let decision = strategy_of("vol-squeeze-breakout").decide(&snapshot(&closes, 0));
        assert_eq!(decision.intents.len(), 1, "squeeze + breakout → buy");
        assert_eq!(decision.intents[0].side, OrderSide::Buy);
        // Same breakout with the tight stretch replaced by wide swings —
        // no squeeze, no buy.
        let mut wild: Vec<f64> = Vec::new();
        for i in 0..24 {
            let swing = if i % 2 == 0 { 1.5 } else { -1.5 };
            wild.push(100.0 + swing);
        }
        wild.push(103.0);
        assert!(strategy_of("vol-squeeze-breakout")
            .decide(&snapshot(&wild, 0))
            .intents
            .is_empty());
        // Held and losing the 10-day low — sell.
        let mut falling: Vec<f64> = (0..18).map(|i| 100.0 - (i as f64) * 0.1).collect();
        falling.push(98.0);
        let sell = strategy_of("vol-squeeze-breakout").decide(&snapshot(&falling, 5));
        assert_eq!(sell.intents.len(), 1);
        assert_eq!(sell.intents[0].side, OrderSide::Sell);
    }
}
