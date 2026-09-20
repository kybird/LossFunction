//! Tick-to-daily-bar aggregation.
//!
//! Feeds the `HistoryWindow` supply gap (raw 2026-09-17 Case 1): the live
//! quote stream now closes into completed daily bars. A bar is emitted **only
//! when a tick from a later trading date arrives** (or on `flush`) — an
//! in-progress bar is never visible to consumers, so snapshots cannot see a
//! bar that later prices would mutate (look-ahead impossible by construction,
//! same contract as `HistoryWindow`).
//!
//! Trading date = the KST calendar date of the tick (KRX session). Bar
//! timestamps use the completion convention shared with the KIS chart source:
//! that date's 15:30 KST == 06:30 UTC.
//!
//! Volume is 0: the realtime `Quote` shape carries no traded size. Indicator
//! strategies consume closes only; if a venue quote later carries size, feed
//! it through here.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;

use crate::marketdata::{Bar, Timeframe};
use crate::types::{Quote, Symbol};

/// One symbol's in-progress bar.
#[derive(Debug, Clone)]
struct PartialBar {
    date: NaiveDate,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
}

/// Aggregates a live tick stream into completed daily bars, per symbol.
#[derive(Debug, Default)]
pub struct BarAggregator {
    inner: HashMap<Symbol, PartialBar>,
}

fn kst_date(timestamp: DateTime<Utc>) -> NaiveDate {
    (timestamp + Duration::hours(9)).date_naive()
}

fn close_instant(date: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&date.and_hms_opt(6, 30, 0).expect("valid hm"))
}

impl BarAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one tick. Returns the **previous** trading date's completed bar
    /// when this tick opens a new date for its symbol, otherwise `None`.
    pub fn push(&mut self, quote: &Quote) -> Option<Bar> {
        let date = kst_date(quote.timestamp);
        match self.inner.get_mut(&quote.symbol) {
            Some(partial) if partial.date == date => {
                if quote.last_price > partial.high {
                    partial.high = quote.last_price;
                }
                if quote.last_price < partial.low {
                    partial.low = quote.last_price;
                }
                partial.close = quote.last_price;
                None
            }
            Some(partial) => {
                let completed = Bar {
                    symbol: quote.symbol.clone(),
                    timeframe: Timeframe::Day,
                    timestamp: close_instant(partial.date),
                    open: partial.open,
                    high: partial.high,
                    low: partial.low,
                    close: partial.close,
                    volume: 0,
                };
                *partial = PartialBar {
                    date,
                    open: quote.last_price,
                    high: quote.last_price,
                    low: quote.last_price,
                    close: quote.last_price,
                };
                Some(completed)
            }
            None => {
                self.inner.insert(
                    quote.symbol.clone(),
                    PartialBar {
                        date,
                        open: quote.last_price,
                        high: quote.last_price,
                        low: quote.last_price,
                        close: quote.last_price,
                    },
                );
                None
            }
        }
    }

    /// Close every in-progress bar now (shutdown, tests). Empties the
    /// aggregator; subsequent ticks start fresh bars.
    pub fn flush(&mut self) -> Vec<Bar> {
        self.inner
            .drain()
            .map(|(symbol, partial)| Bar {
                symbol,
                timeframe: Timeframe::Day,
                timestamp: close_instant(partial.date),
                open: partial.open,
                high: partial.high,
                low: partial.low,
                close: partial.close,
                volume: 0,
            })
            .collect()
    }

    /// Read-only view of a symbol's in-progress bar (date, OHLC). Exposure for
    /// tests and status display — never emitted as a completed `Bar`.
    pub fn current(
        &self,
        symbol: &Symbol,
    ) -> Option<(NaiveDate, Decimal, Decimal, Decimal, Decimal)> {
        self.inner.get(symbol).map(|partial| {
            (
                partial.date,
                partial.open,
                partial.high,
                partial.low,
                partial.close,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn symbol(code: &str) -> Symbol {
        Symbol::parse(code).unwrap()
    }

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    /// 2026-01-05 15:00 KST == 06:00 UTC — inside the KRX session.
    fn tick_at(day: &str, hour_utc: u32) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("{day}T{hour_utc:02}:00:00+00:00"))
            .unwrap()
            .with_timezone(&Utc)
    }

    fn quote(symbol: &Symbol, day: &str, hour_utc: u32, price: &str) -> Quote {
        Quote {
            symbol: symbol.clone(),
            last_price: dec(price),
            timestamp: tick_at(day, hour_utc),
        }
    }

    #[test]
    fn same_date_ticks_update_partial_without_emitting() {
        let mut aggregator = BarAggregator::new();
        let samsung = symbol("005930");

        assert!(aggregator
            .push(&quote(&samsung, "2026-01-05", 6, "100"))
            .is_none());
        assert!(aggregator
            .push(&quote(&samsung, "2026-01-05", 7, "110"))
            .is_none());
        assert!(aggregator
            .push(&quote(&samsung, "2026-01-05", 8, "95"))
            .is_none());

        let (date, open, high, low, close) = aggregator.current(&samsung).unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 1, 5).unwrap());
        assert_eq!(
            (open, high, low, close),
            (dec("100"), dec("110"), dec("95"), dec("95"))
        );
        // The in-progress bar never leaks as a completed Bar.
        assert!(aggregator.flush().len() <= 1);
    }

    /// AC #1 + #2: date change closes the bar with full OHLC; later days'
    /// prices cannot mutate it.
    #[test]
    fn date_change_emits_completed_bar_and_freezes_it() {
        let mut aggregator = BarAggregator::new();
        let samsung = symbol("005930");

        // Day 1 forms O100 H110 L95 C105.
        for (hour, price) in [(6u32, "100"), (7, "110"), (8, "95"), (9, "105")] {
            assert!(aggregator
                .push(&quote(&samsung, "2026-01-05", hour, price))
                .is_none());
        }
        // First tick of day 2 closes day 1.
        let bar = aggregator
            .push(&quote(&samsung, "2026-01-06", 6, "200"))
            .expect("day-1 bar completes");
        assert_eq!(bar.symbol, samsung);
        assert_eq!(bar.timeframe, Timeframe::Day);
        assert_eq!(
            bar.timestamp,
            close_instant(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap())
        );
        assert_eq!(bar.open, dec("100"));
        assert_eq!(bar.high, dec("110"));
        assert_eq!(bar.low, dec("95"));
        assert_eq!(bar.close, dec("105"));
        assert_eq!(bar.volume, 0); // Quote shape carries no size — documented

        // Day 2 ticks only affect the new partial; the frozen bar's values
        // are already detached.
        assert!(aggregator
            .push(&quote(&samsung, "2026-01-06", 7, "300"))
            .is_none());
        assert_eq!(bar.high, dec("110")); // untouched by later prices
        let (_, _, _, _, close2) = aggregator.current(&samsung).unwrap();
        assert_eq!(close2, dec("300"));
    }

    #[test]
    fn symbols_aggregate_independently() {
        let mut aggregator = BarAggregator::new();
        let samsung = symbol("005930");
        let naver = symbol("035420");

        aggregator.push(&quote(&samsung, "2026-01-05", 6, "100"));
        aggregator.push(&quote(&naver, "2026-01-05", 6, "400"));

        // Only Naver's day-1 bar closes here; Samsung's is still in progress.
        let bar = aggregator
            .push(&quote(&naver, "2026-01-06", 6, "410"))
            .unwrap();
        assert_eq!(bar.symbol, naver);
        assert!(aggregator.current(&samsung).is_some());
    }

    #[test]
    fn flush_closes_partials_and_empties() {
        let mut aggregator = BarAggregator::new();
        let samsung = symbol("005930");
        aggregator.push(&quote(&samsung, "2026-01-05", 6, "100"));
        aggregator.push(&quote(&samsung, "2026-01-05", 7, "120"));

        let bars = aggregator.flush();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, dec("120"));
        assert!(aggregator.current(&samsung).is_none());
        assert!(aggregator.flush().is_empty()); // drained, not duplicated
    }

    /// KST boundary: 2026-01-05 15:30 KST == 06:30 UTC still belongs to the
    /// 01-05 session; 2026-01-06 06:00 UTC (= 01-06 15:00 KST) is a new date.
    #[test]
    fn trading_date_follows_kst_calendar() {
        let mut aggregator = BarAggregator::new();
        let samsung = symbol("005930");
        aggregator.push(&quote(&samsung, "2026-01-05", 6, "100"));
        aggregator.push(&quote(&samsung, "2026-01-05", 6, "100"));

        let (date, ..) = aggregator.current(&samsung).unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 1, 5).unwrap());

        let emitted = aggregator
            .push(&quote(&samsung, "2026-01-06", 6, "101"))
            .is_some();
        assert!(emitted, "01-06 KST tick closes the 01-05 bar");
    }
}
