//! Bounded past-closes window for indicator strategies.
//!
//! Strategies receive only completed bars through this window; the current
//! live quote rides separately in `MarketSnapshot.quotes`. Look-ahead is
//! impossible by construction: a snapshot captured at time t contains only
//! the values pushed before t, and later pushes can never mutate an
//! already-produced snapshot.

use std::collections::{HashMap, VecDeque};

use rust_decimal::Decimal;

use crate::types::{Price, Symbol};

/// Per-symbol ring of the most recent `capacity` closes.
#[derive(Debug, Clone)]
pub struct HistoryWindow {
    capacity: usize,
    inner: HashMap<Symbol, VecDeque<Price>>,
}

impl HistoryWindow {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "history window capacity must be positive");
        Self {
            capacity,
            inner: HashMap::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Record one completed bar close; keeps the newest `capacity` values.
    pub fn push(&mut self, symbol: &Symbol, close: Price) {
        let queue = self.inner.entry(symbol.clone()).or_default();
        queue.push_back(close);
        while queue.len() > self.capacity {
            queue.pop_front();
        }
    }

    /// Copy the window (oldest first) for snapshot assembly.
    pub fn snapshot(&self) -> HashMap<Symbol, Vec<Price>> {
        self.inner
            .iter()
            .map(|(symbol, queue)| (symbol.clone(), queue.iter().copied().collect()))
            .collect()
    }

    pub fn len(&self, symbol: &Symbol) -> usize {
        self.inner.get(symbol).map_or(0, VecDeque::len)
    }
}

/// Simple moving average over the tail of a close series (exact Decimal).
pub fn sma(closes: &[Price], period: usize) -> Option<Price> {
    if period == 0 || closes.len() < period {
        return None;
    }
    let tail = &closes[closes.len() - period..];
    Some(tail.iter().copied().sum::<Decimal>() / Decimal::from(period))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(code: &str) -> Symbol {
        Symbol::parse(code).unwrap()
    }

    fn price(value: i64) -> Price {
        Decimal::from(value)
    }

    #[test]
    fn window_is_bounded_and_per_symbol() {
        let mut window = HistoryWindow::new(3);
        for value in 1..=5 {
            window.push(&symbol("005930"), price(value));
        }
        window.push(&symbol("035420"), price(100));
        assert_eq!(window.len(&symbol("005930")), 3);
        assert_eq!(window.len(&symbol("035420")), 1);
        assert_eq!(
            window.snapshot()[&symbol("005930")],
            vec![price(3), price(4), price(5)] // newest kept, oldest dropped
        );
    }

    /// Look-ahead guard: a captured snapshot cannot see later pushes.
    #[test]
    fn snapshot_cannot_see_future_pushes() {
        let mut window = HistoryWindow::new(10);
        for value in 1..=4 {
            window.push(&symbol("005930"), price(value));
        }
        let frozen = window.snapshot();
        for value in 5..=8 {
            window.push(&symbol("005930"), price(value));
        }
        assert_eq!(
            frozen[&symbol("005930")],
            vec![price(1), price(2), price(3), price(4)]
        );
        assert_eq!(window.snapshot()[&symbol("005930")].len(), 8);
    }

    #[test]
    fn sma_is_exact_over_the_tail() {
        let closes = [price(1), price(2), price(3), price(4)];
        assert_eq!(sma(&closes, 2), Some(Decimal::from(7) / Decimal::from(2))); // (3+4)/2
        assert_eq!(sma(&closes, 4), Some(Decimal::from(10) / Decimal::from(4)));
        assert_eq!(sma(&closes, 5), None);
        assert_eq!(sma(&closes, 0), None);
    }

    #[test]
    fn window_capacity_comes_from_settings() {
        use crate::config::Settings;
        assert_eq!(Settings::default().history_window_bars, 120);
    }
}
