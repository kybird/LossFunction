//! Bollinger Band mean-reversion strategy (wiki: quant-strategies).
//!
//! 20-bar SMA ± 2σ bands: enter long on a close below the lower band,
//! exit (sell) on a close above the upper band. Operates on completed-bar
//! closes only; sells fire only when a position is held.

use std::collections::BTreeMap;

use rust_decimal::prelude::{Decimal, ToPrimitive};

use crate::history::sma;
use crate::strategy::{MarketSnapshot, OrderIntent, Strategy, StrategyDecision};
use crate::types::{OrderSide, OrderType, Quantity, Symbol};

pub struct BollingerReversionStrategy {
    symbols: Vec<Symbol>,
    period: usize,
    sigma_multiplier: Decimal,
    quantity: Quantity,
}

impl BollingerReversionStrategy {
    pub fn new(
        symbols: Vec<Symbol>,
        period: usize,
        sigma_multiplier: Decimal,
        quantity: Quantity,
    ) -> Self {
        Self {
            symbols,
            period,
            sigma_multiplier,
            quantity,
        }
    }

    fn bands(&self, closes: &[Decimal]) -> Option<(Decimal, Decimal, Decimal)> {
        if closes.len() < self.period {
            return None;
        }
        let tail = &closes[closes.len() - self.period..];
        let mid = sma(closes, self.period)?;
        let variance = tail
            .iter()
            .map(|close| {
                let diff = *close - mid;
                diff * diff
            })
            .sum::<Decimal>()
            / Decimal::from(self.period);
        // Population standard deviation; Decimal sqrt needs the maths
        // feature, so approximate via f64 and re-check exactness on round.
        let variance_f64 = variance.to_f64()?;
        let sigma = Decimal::from_f64_retain(variance_f64.sqrt())?;
        let band = sigma * self.sigma_multiplier;
        Some((mid - band, mid, mid + band))
    }
}

impl Strategy for BollingerReversionStrategy {
    fn name(&self) -> &str {
        "bollinger-reversion"
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
            if let Some((lower, mid, upper)) = self.bands(closes) {
                features.insert(format!("{symbol}:lower"), lower.round_dp(2).to_string());
                features.insert(format!("{symbol}:mid"), mid.round_dp(2).to_string());
                features.insert(format!("{symbol}:upper"), upper.round_dp(2).to_string());

                let held = snapshot
                    .positions
                    .get(symbol)
                    .map(|position| position.quantity)
                    .unwrap_or(0);
                if last < &lower && held == 0 {
                    intents.push(OrderIntent {
                        symbol: symbol.clone(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Market,
                        quantity: self.quantity,
                        limit_price: None,
                    });
                } else if last > &upper && held > 0 {
                    intents.push(OrderIntent {
                        symbol: symbol.clone(),
                        side: OrderSide::Sell,
                        order_type: OrderType::Market,
                        quantity: held.min(self.quantity),
                        limit_price: None,
                    });
                }
            }
        }
        StrategyDecision {
            strategy_name: self.name().to_string(),
            strategy_version: "1".to_string(),
            rationale: if intents.is_empty() {
                "no band signals".to_string()
            } else {
                "bollinger band reversion".to_string()
            },
            intents,
            features,
        }
    }
}

/// 12-1 momentum rotation (wiki: quant-strategies): rank by trailing
/// 12-month return excluding the most recent month; rebalance monthly.
pub struct MomentumRotationStrategy {
    symbols: Vec<Symbol>,
    top_n: usize,
    quantity: Quantity,
}

impl MomentumRotationStrategy {
    pub fn new(symbols: Vec<Symbol>, top_n: usize, quantity: Quantity) -> Self {
        Self {
            symbols,
            top_n,
            quantity,
        }
    }

    fn momentum_score(closes: &[Decimal]) -> Option<Decimal> {
        // 12-1: return from t-252 to t-21 (≈21 bars/month).
        if closes.len() < 253 {
            return None;
        }
        let recent = closes[closes.len() - 22];
        let base = closes[closes.len() - 253];
        if base.is_zero() {
            return None;
        }
        Some((recent - base) / base)
    }
}

impl Strategy for MomentumRotationStrategy {
    fn name(&self) -> &str {
        "momentum-rotation"
    }
    fn version(&self) -> &str {
        "1"
    }

    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let mut features = BTreeMap::new();
        let mut scores: Vec<(Symbol, Decimal)> = Vec::new();

        for symbol in &self.symbols {
            if let Some(closes) = snapshot.history.get(symbol) {
                if let Some(score) = Self::momentum_score(closes) {
                    features.insert(format!("{symbol}:score"), score.round_dp(4).to_string());
                    scores.push((symbol.clone(), score));
                }
            }
        }
        scores.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))); // deterministic tiebreak

        let mut intents = Vec::new();
        for (symbol, _) in scores.iter().take(self.top_n) {
            let held = snapshot
                .positions
                .get(symbol)
                .map(|position| position.quantity)
                .unwrap_or(0);
            if held == 0 {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: self.quantity,
                    limit_price: None,
                });
            }
        }
        // Exit: held symbols that fell out of the top-N.
        for (symbol, _) in scores.iter().skip(self.top_n) {
            if let Some(position) = snapshot.positions.get(symbol) {
                if position.quantity > 0 {
                    intents.push(OrderIntent {
                        symbol: symbol.clone(),
                        side: OrderSide::Sell,
                        order_type: OrderType::Market,
                        quantity: position.quantity.min(self.quantity),
                        limit_price: None,
                    });
                }
            }
        }

        StrategyDecision {
            strategy_name: self.name().to_string(),
            strategy_version: "1".to_string(),
            rationale: format!("top-{} by 12-1 momentum", self.top_n),
            intents,
            features,
        }
    }
}

/// MACD crossover (wiki: quant-strategies): MACD(12,26,9); buy on signal
/// line cross-up, sell on cross-down. Complements the SMA crossover.
pub struct MacdCrossStrategy {
    symbols: Vec<Symbol>,
    fast: usize,
    slow: usize,
    signal: usize,
    quantity: Quantity,
}

impl MacdCrossStrategy {
    pub fn new(
        symbols: Vec<Symbol>,
        fast: usize,
        slow: usize,
        signal: usize,
        quantity: Quantity,
    ) -> Self {
        Self {
            symbols,
            fast,
            slow,
            signal,
            quantity,
        }
    }

    fn ema_series(closes: &[Decimal], period: usize) -> Vec<Decimal> {
        if closes.len() < period {
            return Vec::new();
        }
        let alpha = Decimal::from(2) / Decimal::from(period + 1);
        let mut series = Vec::with_capacity(closes.len() - period + 1);
        let seed: Decimal =
            closes[..period].iter().copied().sum::<Decimal>() / Decimal::from(period);
        series.push(seed);
        for close in &closes[period..] {
            let previous = series.last().expect("seeded");
            series.push(alpha * close + (Decimal::ONE - alpha) * previous);
        }
        series
    }

    fn cross(&self, closes: &[Decimal]) -> Option<(bool, bool)> {
        if closes.len() < self.slow + self.signal {
            return None;
        }
        let fast_ema = Self::ema_series(closes, self.fast);
        let slow_ema = Self::ema_series(closes, self.slow);
        // Align tails: slow_ema is shorter.
        let offset = fast_ema.len() - slow_ema.len();
        let macd: Vec<Decimal> = slow_ema
            .iter()
            .enumerate()
            .map(|(i, slow)| fast_ema[i + offset] - *slow)
            .collect();
        if macd.len() < self.signal + 1 {
            return None;
        }
        let signal_series = Self::ema_series(&macd, self.signal);
        let macd_now = *macd.last()?;
        let signal_now = *signal_series.last()?;
        let macd_prev = macd[macd.len() - 2];
        let signal_prev = signal_series[signal_series.len() - 2];
        Some((
            macd_prev <= signal_prev && macd_now > signal_now, // bullish cross
            macd_prev >= signal_prev && macd_now < signal_now, // bearish cross
        ))
    }
}

impl Strategy for MacdCrossStrategy {
    fn name(&self) -> &str {
        "macd-cross"
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
            if let Some((bullish, bearish)) = self.cross(closes) {
                features.insert(format!("{symbol}:bullish"), bullish.to_string());
                features.insert(format!("{symbol}:bearish"), bearish.to_string());
                let held = snapshot
                    .positions
                    .get(symbol)
                    .map(|position| position.quantity)
                    .unwrap_or(0);
                if bullish && held == 0 {
                    intents.push(OrderIntent {
                        symbol: symbol.clone(),
                        side: OrderSide::Buy,
                        order_type: OrderType::Market,
                        quantity: self.quantity,
                        limit_price: None,
                    });
                } else if bearish && held > 0 {
                    intents.push(OrderIntent {
                        symbol: symbol.clone(),
                        side: OrderSide::Sell,
                        order_type: OrderType::Market,
                        quantity: held.min(self.quantity),
                        limit_price: None,
                    });
                }
            }
        }
        StrategyDecision {
            strategy_name: self.name().to_string(),
            strategy_version: "1".to_string(),
            rationale: if intents.is_empty() {
                "no macd signals".to_string()
            } else {
                "macd crossover".to_string()
            },
            intents,
            features,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::portfolio::PositionState;
    use chrono::Utc;
    use rust_decimal::prelude::{Decimal, ToPrimitive};
    use std::str::FromStr;

    fn symbol(code: &str) -> Symbol {
        Symbol::parse(code).unwrap()
    }

    fn snapshot(histories: Vec<(&str, Vec<i64>)>, held: &[(&str, i64)]) -> MarketSnapshot {
        let mut history = HashMap::new();
        for (code, closes) in histories {
            history.insert(
                symbol(code),
                closes.iter().map(|v| Decimal::from(*v)).collect::<Vec<_>>(),
            );
        }
        let mut positions = HashMap::new();
        for (code, qty) in held {
            positions.insert(
                symbol(code),
                PositionState {
                    symbol: symbol(code),
                    quantity: *qty,
                    average_price: Decimal::from(100),
                    realized_pnl: Decimal::ZERO,
                },
            );
        }
        MarketSnapshot {
            quotes: HashMap::new(),
            positions,
            as_of: "t".into(),
            history,
        }
    }

    #[test]
    fn bollinger_buys_lower_sells_upper() {
        let strategy =
            BollingerReversionStrategy::new(vec![symbol("005930")], 20, Decimal::from(2), 10);
        // Flat series then a spike below: std>0, last < lower band.
        let mut closes = vec![100; 25];
        closes.push(60);
        let buy = strategy.decide(&snapshot(vec![("005930", closes.clone())], &[]));
        assert_eq!(buy.intents.len(), 1);
        assert_eq!(buy.intents[0].side, OrderSide::Buy);

        // Held + spike above the upper band -> sell.
        let mut rally = vec![100; 25];
        rally.push(140);
        let sell = strategy.decide(&snapshot(vec![("005930", rally)], &[("005930", 10)]));
        assert_eq!(sell.intents.len(), 1);
        assert_eq!(sell.intents[0].side, OrderSide::Sell);

        // Determinism.
        assert_eq!(
            strategy.decide(&snapshot(vec![("005930", closes)], &[])),
            buy
        );
    }

    #[test]
    fn momentum_ranks_and_exits() {
        // 253+ bars per symbol; one strong uptrend, one flat, one decline.
        let rising: Vec<i64> = (0..260).map(|i| 100 + i).collect();
        let flat: Vec<i64> = vec![100; 260];
        let falling: Vec<i64> = (0..260).map(|i| 300 - i).collect();
        let strategy = MomentumRotationStrategy::new(
            vec![symbol("005930"), symbol("035420"), symbol("069500")],
            1, // top 1
            10,
        );
        let view = snapshot(
            vec![("005930", rising), ("035420", flat), ("069500", falling)],
            &[("069500", 10)], // held the loser -> must exit
        );
        let decision = strategy.decide(&view);
        assert!(decision
            .intents
            .iter()
            .any(|i| i.side == OrderSide::Buy && i.symbol == symbol("005930"))); // winner bought
        assert!(decision
            .intents
            .iter()
            .any(|i| i.side == OrderSide::Sell && i.symbol == symbol("069500")));
        // loser exited
    }

    #[test]
    fn macd_detects_crossovers() {
        let strategy = MacdCrossStrategy::new(vec![symbol("005930")], 12, 26, 9, 10);
        // Downtrend then sharp uptrend: bullish cross at the end.
        // Long flat series then a single jump bar: MACD crosses above the
        // signal line on exactly the last bar.
        let mut series: Vec<i64> = vec![100; 50];
        series.extend([100, 100, 100, 200]);
        let buy = strategy.decide(&snapshot(vec![("005930", series)], &[]));
        assert!(buy.intents.iter().any(|i| i.side == OrderSide::Buy));

        // Determinism: same series, same decision.
        let mut repeat: Vec<i64> = vec![100; 50];
        repeat.extend([100, 100, 100, 200]);
        let again = strategy.decide(&snapshot(vec![("005930", repeat)], &[]));
        assert_eq!(buy, again);
    }

    #[test]
    fn ema_matches_definition_on_tiny_series() {
        let closes: Vec<Decimal> = [1, 2, 3, 4, 5].iter().map(|v| Decimal::from(*v)).collect();
        let ema3 = MacdCrossStrategy::ema_series(&closes, 3);
        assert_eq!(ema3.len(), 3);
        // seed = (1+2+3)/3 = 2; alpha = 2/4 = 0.5
        assert_eq!(ema3[0], Decimal::from(2));
        assert_eq!(ema3[1], Decimal::from(3)); // 0.5*4 + 0.5*2
        assert_eq!(ema3[2], Decimal::from(4)); // 0.5*5 + 0.5*3
    }
}
