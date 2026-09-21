//! Strategy registry — the catalog of runnable strategies.
//!
//! One place answers "what can I run?" and turns a key into a configured
//! instance with canonical default parameters. Adding a strategy means
//! implementing the `Strategy` trait and registering it here (docs/
//! strategies.md walks through it).

use rust_decimal::Decimal;

use crate::strategies::{DonchianBreakoutStrategy, RsiReversionStrategy, SmaCrossStrategy};
use crate::strategies2::{BollingerReversionStrategy, MacdCrossStrategy, MomentumRotationStrategy};
use crate::strategy::Strategy;
use crate::types::{Quantity, Symbol};

/// A catalog entry: identity + human-facing description of the defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct StrategySpec {
    /// Stable identifier used by APIs and the backtest form.
    pub key: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Default parameters in prose ("SMA 5/20").
    pub params: &'static str,
}

pub fn registry() -> Vec<StrategySpec> {
    vec![
        StrategySpec {
            key: "sma-cross",
            name: "SMA 교차",
            description: "단기 이동평균이 장기를 상향 돌파하면 매수, 하향 돌파하면 매도",
            params: "빠름 5 · 느림 20",
        },
        StrategySpec {
            key: "rsi-reversion",
            name: "RSI 역행",
            description: "과매도(하한 돌파)에서 매수, 과매수(상한 돌파)에서 매도",
            params: "기간 14 · 30/70",
        },
        StrategySpec {
            key: "donchian-breakout",
            name: "돈키안 브레이크아웃",
            description: "N일 최고가 돌파 매수, N일 최저가 하회 매도",
            params: "기간 20",
        },
        StrategySpec {
            key: "bollinger-reversion",
            name: "볼린저 밴드 역행",
            description: "밴드 하단 이탈에서 매수, 상단 접근에서 매도",
            params: "기간 20 · 2σ",
        },
        StrategySpec {
            key: "momentum-rotation",
            name: "12-1 모멘텀 로테이션",
            description: "12개월 수익률 상위 종목으로 포지션을 회전",
            params: "상위 1종목 · 1개월 리밸런스",
        },
        StrategySpec {
            key: "macd-cross",
            name: "MACD 교차",
            description: "MACD 신호선 상향 교차 매수, 하향 교차 매도",
            params: "12/26/9",
        },
    ]
}

/// Look up a catalog entry by key (cloned — specs are tiny).
pub fn find(key: &str) -> Option<StrategySpec> {
    registry().into_iter().find(|spec| spec.key == key)
}

/// Build a strategy instance by key with canonical defaults.
pub fn build(key: &str, symbols: &[Symbol], quantity: Quantity) -> Option<Box<dyn Strategy>> {
    let symbols = symbols.to_vec();
    match key {
        "sma-cross" => Some(Box::new(SmaCrossStrategy::new(symbols, 5, 20, quantity))),
        "rsi-reversion" => Some(Box::new(RsiReversionStrategy::new(
            symbols,
            14,
            Decimal::from(30),
            Decimal::from(70),
            quantity,
        ))),
        "donchian-breakout" => Some(Box::new(DonchianBreakoutStrategy::new(
            symbols, 20, quantity,
        ))),
        "bollinger-reversion" => Some(Box::new(BollingerReversionStrategy::new(
            symbols,
            20,
            Decimal::from(2),
            quantity,
        ))),
        "momentum-rotation" => Some(Box::new(MomentumRotationStrategy::new(
            symbols, 1, quantity,
        ))),
        "macd-cross" => Some(Box::new(MacdCrossStrategy::new(
            symbols, 12, 26, 9, quantity,
        ))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols() -> Vec<Symbol> {
        vec![Symbol::parse("005930").unwrap()]
    }

    #[test]
    fn registry_lists_six_strategies_with_unique_keys() {
        let specs = registry();
        assert_eq!(specs.len(), 6);
        let mut keys: Vec<_> = specs.iter().map(|spec| spec.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 6, "keys must be unique");
    }

    #[test]
    fn every_registered_key_builds() {
        for spec in registry() {
            let built = build(spec.key, &symbols(), 1);
            assert!(built.is_some(), "failed to build {}", spec.key);
        }
        assert!(build("no-such-strategy", &symbols(), 1).is_none());
    }

    #[test]
    fn find_resolves_keys() {
        assert_eq!(find("sma-cross").unwrap().name, "SMA 교차");
        assert!(find("ghost").is_none());
    }
}
