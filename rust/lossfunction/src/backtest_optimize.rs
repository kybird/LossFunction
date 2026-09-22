//! Parameter walk-forward search — machine-tuned defaults for strategies.
//!
//! For each candidate parameter set of a registered strategy: backtest on
//! the TRAIN slice of real candles, rank, then re-test the survivors on the
//! unseen VALIDATION slice. Only combos that survive out-of-sample are
//! offered — the output is a *suggested default* with evidence, never a
//! silent swap of the strategy's canonical parameters.

use std::sync::Arc;

use rust_decimal::Decimal;

use crate::backtest::{summarize, BacktestConfig, BacktestEngine, Bar, Metrics};
use crate::broker::mock::MockBroker;
use crate::strategies::{DonchianBreakoutStrategy, RsiReversionStrategy, SmaCrossStrategy};
use crate::strategies2::{BollingerReversionStrategy, MacdCrossStrategy, MomentumRotationStrategy};
use crate::strategy::Strategy;
use crate::types::{Quantity, Symbol};

/// One candidate parameter set: a human label + a factory.
/// Factory alias — grids close over parameter values.
pub type StrategyFactory = Arc<dyn Fn(&[Symbol], Quantity) -> Box<dyn Strategy> + Send + Sync>;

#[derive(Clone)]
pub struct ParamSet {
    pub label: String,
    /// True when this is the strategy's canonical default (always included
    /// as the baseline every combo must beat).
    pub is_default: bool,
    pub build: StrategyFactory,
}

/// The parameter grid per strategy key — small, curated, and bounded
/// (≤ 16 combos) so a full walk-forward stays in low minutes.
pub fn param_grid(strategy_key: &str) -> Vec<ParamSet> {
    fn set(
        label: &str,
        is_default: bool,
        build: impl Fn(&[Symbol], Quantity) -> Box<dyn Strategy> + Send + Sync + 'static,
    ) -> ParamSet {
        ParamSet {
            label: label.to_string(),
            is_default,
            build: Arc::new(build),
        }
    }
    match strategy_key {
        "sma-cross" => {
            let mut grid = vec![set("기본 5/20", true, |s, q| {
                Box::new(SmaCrossStrategy::new(s.to_vec(), 5, 20, q))
            })];
            for slow in [20usize, 30, 40, 60] {
                for fast in [3usize, 7, 10] {
                    if fast < slow {
                        grid.push(set(&format!("{fast}/{slow}"), false, move |s, q| {
                            Box::new(SmaCrossStrategy::new(s.to_vec(), fast, slow, q))
                        }));
                    }
                }
            }
            grid
        }
        "rsi-reversion" => {
            let mut grid = vec![set("기본 14·30/70", true, |s, q| {
                Box::new(RsiReversionStrategy::new(
                    s.to_vec(),
                    14,
                    Decimal::from(30),
                    Decimal::from(70),
                    q,
                ))
            })];
            for period in [7usize, 14, 21] {
                for (low, high) in [(30i32, 70), (25, 75), (20, 80)] {
                    if period == 14 && low == 30 {
                        continue; // that's the default
                    }
                    grid.push(set(
                        &format!("{period}·{low}/{high}"),
                        false,
                        move |s, q| {
                            Box::new(RsiReversionStrategy::new(
                                s.to_vec(),
                                period,
                                Decimal::from(low),
                                Decimal::from(high),
                                q,
                            ))
                        },
                    ));
                }
            }
            grid
        }
        "donchian-breakout" => [10usize, 20, 30, 40, 55]
            .iter()
            .map(|&period| {
                set(&format!("{period}일"), period == 20, move |s, q| {
                    Box::new(DonchianBreakoutStrategy::new(s.to_vec(), period, q))
                })
            })
            .collect(),
        "bollinger-reversion" => {
            let mut grid = Vec::new();
            for period in [10usize, 20, 30] {
                for sigma in ["1.5", "2", "2.5"] {
                    let is_default = period == 20 && sigma == "2";
                    let sigma_value = Decimal::from_str_exact(sigma).unwrap();
                    grid.push(set(
                        &format!("{period}·{sigma}σ"),
                        is_default,
                        move |s, q| {
                            Box::new(BollingerReversionStrategy::new(
                                s.to_vec(),
                                period,
                                sigma_value,
                                q,
                            ))
                        },
                    ));
                }
            }
            grid
        }
        "macd-cross" => [
            (12usize, 26usize, 9usize),
            (8, 17, 9),
            (10, 20, 9),
            (5, 35, 5),
        ]
        .iter()
        .map(|&(fast, slow, signal)| {
            set(
                &format!("{fast}/{slow}/{signal}"),
                fast == 12,
                move |s, q| Box::new(MacdCrossStrategy::new(s.to_vec(), fast, slow, signal, q)),
            )
        })
        .collect(),
        "momentum-rotation" => [1usize, 2, 3]
            .iter()
            .map(|&top_n| {
                set(&format!("상위 {top_n}"), top_n == 1, move |s, q| {
                    Box::new(MomentumRotationStrategy::new(s.to_vec(), top_n, q))
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// One combo's outcome across both windows.
#[derive(Debug, Clone)]
pub struct WalkForwardOutcome {
    pub label: String,
    pub is_default: bool,
    pub train_return_pct: Decimal,
    pub validation: Metrics,
}

/// Train/validation split point: bars are chronological; the first
/// `train_ratio` fraction trains, the rest validates.
fn split_bars(bars: &[Bar], train_ratio: f64) -> (Vec<Bar>, Vec<Bar>) {
    let cut = ((bars.len() as f64) * train_ratio) as usize;
    (bars[..cut].to_vec(), bars[cut..].to_vec())
}

async fn run_one(
    bars: &[Bar],
    build: &StrategyFactory,
    symbols: &[Symbol],
    config: &BacktestConfig,
) -> Metrics {
    let broker = Arc::new(MockBroker::new());
    let hook_broker = Arc::clone(&broker);
    let engine = BacktestEngine::new(build(symbols, 10), config.clone()).with_bar_hook(Arc::new(
        move |bar: &Bar| {
            hook_broker.set_price(&bar.symbol, bar.close);
        },
    ));
    let result = engine.run(bars.to_vec(), &*broker).await;
    summarize(&result, config.initial_cash)
}

/// Walk-forward over the full grid: rank by TRAIN return, keep the top
/// `survivors` plus the default, then rank those by VALIDATION return.
/// The default is always included as the baseline.
pub async fn walk_forward(
    bars: &[Bar],
    strategy_key: &str,
    symbols: &[Symbol],
    config: BacktestConfig,
    train_ratio: f64,
    survivors: usize,
) -> Result<Vec<WalkForwardOutcome>, String> {
    let grid = param_grid(strategy_key);
    if grid.is_empty() {
        return Err(format!(
            "전략 '{strategy_key}'에 정의된 파라미터 격자가 없음"
        ));
    }
    if bars.len() < 120 {
        return Err(format!("봉 부족: {}개 (120 이상 필요)", bars.len()));
    }
    let (train, validation) = split_bars(bars, train_ratio);
    if train.len() < 60 || validation.len() < 30 {
        return Err("훈련/검증 구간이 너무 짧음".to_string());
    }

    // Train pass.
    let mut trained: Vec<(ParamSet, Metrics)> = Vec::new();
    for params in &grid {
        let metrics = run_one(&train, &params.build, symbols, &config).await;
        trained.push((params.clone(), metrics));
    }
    trained.sort_by(|a, b| b.1.return_pct.cmp(&a.1.return_pct));

    // Keep top survivors; always keep the default for the baseline.
    let mut kept: Vec<ParamSet> = trained
        .iter()
        .take(survivors)
        .map(|(params, _)| params.clone())
        .collect();
    if let Some(default) = grid.iter().find(|p| p.is_default) {
        if !kept.iter().any(|p| p.label == default.label) {
            kept.push(default.clone());
        }
    }

    // Validation pass on the survivors.
    let mut outcomes: Vec<WalkForwardOutcome> = Vec::new();
    for params in kept {
        let validation_metrics = run_one(&validation, &params.build, symbols, &config).await;
        let train_metrics = trained
            .iter()
            .find(|(p, _)| p.label == params.label)
            .map(|(_, m)| m)
            .cloned()
            .expect("kept params came from the trained set");
        outcomes.push(WalkForwardOutcome {
            label: params.label,
            is_default: params.is_default,
            train_return_pct: train_metrics.return_pct,
            validation: validation_metrics,
        });
    }
    outcomes.sort_by(|a, b| b.validation.return_pct.cmp(&a.validation.return_pct));
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OrderType;
    use chrono::TimeZone;

    fn seeded_bars(rising_then_fall: bool) -> Vec<Bar> {
        let symbol = Symbol::parse("005930").unwrap();
        let mut bars = Vec::new();
        for i in 0..200i64 {
            let day =
                chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + chrono::Duration::days(i);
            let ts = chrono::Utc.from_utc_datetime(&day.and_hms_opt(6, 30, 0).unwrap());
            let (base, amp) = if rising_then_fall {
                (70_000 + i * 200, 1_500)
            } else {
                (70_000, 3_000)
            };
            let wiggle = if i % 20 < 10 { amp } else { -amp };
            let close = Decimal::from(base + wiggle);
            let _ = OrderType::Market; // keep the import honest in cfg(test)
            bars.push(Bar {
                symbol: symbol.clone(),
                timestamp: ts,
                close,
            });
        }
        bars
    }

    #[test]
    fn grids_are_bounded_and_include_defaults() {
        for key in [
            "sma-cross",
            "rsi-reversion",
            "donchian-breakout",
            "bollinger-reversion",
            "macd-cross",
            "momentum-rotation",
        ] {
            let grid = param_grid(key);
            assert!(!grid.is_empty(), "{key} grid empty");
            assert!(grid.len() <= 16, "{key} grid too large: {}", grid.len());
            assert!(
                grid.iter().any(|p| p.is_default),
                "{key} missing default baseline"
            );
        }
        assert!(param_grid("ghost").is_empty());
    }

    /// End-to-end on synthetic bars: the walk completes, ranks by validation
    /// return, and the default is present as the baseline row.
    #[tokio::test]
    async fn walk_forward_ranks_and_keeps_default() {
        let bars = seeded_bars(true);
        let symbols = vec![Symbol::parse("005930").unwrap()];
        let outcomes = walk_forward(
            &bars,
            "sma-cross",
            &symbols,
            BacktestConfig {
                initial_cash: Decimal::from(1_000_000),
                history_capacity: 10,
                ..BacktestConfig::default()
            },
            0.7,
            3,
        )
        .await
        .unwrap();

        assert!(outcomes.len() >= 3);
        assert!(
            outcomes.iter().any(|o| o.is_default),
            "default must survive as baseline"
        );
        let returns: Vec<Decimal> = outcomes.iter().map(|o| o.validation.return_pct).collect();
        let mut sorted = returns.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(returns, sorted, "outcomes ranked by validation return");
    }

    #[tokio::test]
    async fn walk_forward_needs_history() {
        let bars = seeded_bars(true)[..50].to_vec();
        let symbols = vec![Symbol::parse("005930").unwrap()];
        assert!(walk_forward(
            &bars,
            "sma-cross",
            &symbols,
            BacktestConfig::default(),
            0.7,
            3
        )
        .await
        .is_err());
    }
}
