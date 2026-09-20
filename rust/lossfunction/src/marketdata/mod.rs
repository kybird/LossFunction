//! Market data boundary — historical bar retrieval behind a source-agnostic
//! trait (wiki: marketdata-is-not-broker).
//!
//! The trading core never imports a venue client. A venue (KIS today, anything
//! later) implements `MarketDataSource` and hands plain `Bar` values to the
//! storage layer; swapping or cross-checking sources is an implementation
//! change at this boundary only. This module therefore depends on nothing but
//! the shared kernel (`types`) — importing the venue module here is a layering
//! bug.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

pub mod aggregator;

pub use aggregator::BarAggregator;

use crate::types::{Price, Symbol};

/// Bar resolution. Extensible; the string form is what the `candles` table
/// stores in its `timeframe` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeframe {
    Day,
}

impl Timeframe {
    pub fn as_str(self) -> &'static str {
        match self {
            Timeframe::Day => "day",
        }
    }
}

/// One completed OHLCV bar. Incomplete (in-progress) bars never travel through
/// this type — a source emits a bar only once its period has closed.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub symbol: Symbol,
    pub timeframe: Timeframe,
    /// Period close instant (UTC). Stored as ISO-8601 text; lexicographic
    /// order equals chronological order.
    pub timestamp: DateTime<Utc>,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum MarketDataError {
    #[error("source failure: {0}")]
    Source(String),
}

/// Source-agnostic historical bars. Implementations must return bars
/// oldest-first, only for periods fully closed before `to`, and must not
/// fabricate bars for periods with no trading (holidays, pre-listing).
#[async_trait::async_trait]
pub trait MarketDataSource: Send + Sync {
    /// Completed daily bars for `symbol` with close instants in `[from, to]`.
    async fn daily_bars(
        &self,
        symbol: &Symbol,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Bar>, MarketDataError>;
}

/// Convenience for tests and seeding; not used by the core.
pub fn bar(
    symbol: &Symbol,
    timestamp: DateTime<Utc>,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: i64,
) -> Bar {
    Bar {
        symbol: symbol.clone(),
        timeframe: Timeframe::Day,
        timestamp,
        open,
        high,
        low,
        close,
        volume,
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

    fn instant(day: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("{day}T15:30:00+00:00"))
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Synthetic source: the reference shape any implementation must produce.
    struct SyntheticSource {
        bars: Vec<Bar>,
    }

    #[async_trait::async_trait]
    impl MarketDataSource for SyntheticSource {
        async fn daily_bars(
            &self,
            symbol: &Symbol,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
        ) -> Result<Vec<Bar>, MarketDataError> {
            Ok(self
                .bars
                .iter()
                .filter(|b| &b.symbol == symbol && b.timestamp >= from && b.timestamp <= to)
                .cloned()
                .collect())
        }
    }

    #[tokio::test]
    async fn trait_dispatch_returns_requested_window_only() {
        let samsung = symbol("005930");
        let source = SyntheticSource {
            bars: vec![
                bar(
                    &samsung,
                    instant("2026-01-05"),
                    dec("100"),
                    dec("110"),
                    dec("99"),
                    dec("105"),
                    1_000,
                ),
                bar(
                    &samsung,
                    instant("2026-01-06"),
                    dec("105"),
                    dec("111"),
                    dec("104"),
                    dec("108"),
                    1_100,
                ),
                bar(
                    &samsung,
                    instant("2026-01-07"),
                    dec("108"),
                    dec("109"),
                    dec("100"),
                    dec("101"),
                    900,
                ),
                bar(
                    &symbol("035420"),
                    instant("2026-01-06"),
                    dec("400"),
                    dec("405"),
                    dec("398"),
                    dec("402"),
                    500,
                ),
            ],
        };
        let got = source
            .daily_bars(&samsung, instant("2026-01-06"), instant("2026-01-07"))
            .await
            .unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|b| b.symbol == samsung));
        // oldest first
        assert!(got[0].timestamp < got[1].timestamp);
    }

    /// AC #1: synthetic source -> candles upsert -> re-load leaves no duplicates.
    #[tokio::test]
    async fn upsert_then_reload_is_idempotent() {
        use crate::storage::Repository;

        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("candles.db");
        let repository = Repository::open(db.to_str().unwrap()).await.unwrap();
        repository.migrate().await.unwrap();

        let samsung = symbol("005930");
        let source = SyntheticSource {
            bars: vec![
                bar(
                    &samsung,
                    instant("2026-01-05"),
                    dec("100"),
                    dec("110"),
                    dec("99"),
                    dec("105"),
                    1_000,
                ),
                bar(
                    &samsung,
                    instant("2026-01-06"),
                    dec("105"),
                    dec("111"),
                    dec("104"),
                    dec("108"),
                    1_100,
                ),
                bar(
                    &symbol("035420"),
                    instant("2026-01-06"),
                    dec("400"),
                    dec("405"),
                    dec("398"),
                    dec("402"),
                    500,
                ),
            ],
        };
        let from = instant("2026-01-01");
        let to = instant("2026-01-31");
        let naver = symbol("035420");
        let mut first = source.daily_bars(&samsung, from, to).await.unwrap();
        first.extend(source.daily_bars(&naver, from, to).await.unwrap());
        assert_eq!(first.len(), 3);

        let inserted = repository.upsert_candles(&first).await.unwrap();
        assert_eq!(inserted, 3);

        // Re-load the same window (backfill re-run) with one corrected close.
        let mut second = first.clone();
        second[1].close = dec("109");
        let affected = repository.upsert_candles(&second).await.unwrap();
        assert_eq!(affected, 3); // upserts, not appends

        assert_eq!(repository.candle_count().await.unwrap(), 3); // PK dedup: 3, not 6
        let stored = repository
            .daily_candles(&samsung, Timeframe::Day)
            .await
            .unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[1].close, dec("109")); // corrected value won
        assert!(stored[0].timestamp < stored[1].timestamp); // oldest first

        repository.close().await;
    }
}
