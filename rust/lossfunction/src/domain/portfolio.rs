//! Portfolio aggregate — position aggregation over fills.
//!
//! Exact arithmetic via Decimal; average price blends on buys and survives
//! sells. Realized P&L accrues on sells against the average price at sell
//! time. Fully-closed positions stay with quantity 0 (KIS inquire-balance
//! parity; realized P&L preserved on the entry).

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::types::{OrderSide, Price, Quantity, Symbol};

use super::{blended_average, DomainError};

#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    pub client_order_id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub price: Price,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionState {
    pub symbol: Symbol,
    pub quantity: Quantity,
    pub average_price: Price,
    pub realized_pnl: Price,
}

/// Aggregate of positions; updated only through [`Portfolio::apply_fill`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Portfolio {
    positions: BTreeMap<Symbol, PositionState>,
}

impl Portfolio {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn position(&self, symbol: &Symbol) -> Option<&PositionState> {
        self.positions.get(symbol)
    }

    pub fn positions(&self) -> impl Iterator<Item = &PositionState> {
        self.positions.values()
    }

    pub fn total_realized_pnl(&self) -> Price {
        self.positions
            .values()
            .map(|position| position.realized_pnl)
            .sum()
    }

    /// Return a new portfolio with the fill applied.
    pub fn apply_fill(&self, fill: Fill) -> Result<Portfolio, DomainError> {
        let mut next = self.clone();
        let entry = next.positions.get(&fill.symbol).cloned();
        match fill.side {
            OrderSide::Buy => {
                let updated = match entry {
                    None => PositionState {
                        symbol: fill.symbol.clone(),
                        quantity: fill.quantity,
                        average_price: fill.price,
                        realized_pnl: Decimal::ZERO,
                    },
                    Some(current) => PositionState {
                        symbol: fill.symbol.clone(),
                        quantity: current.quantity + fill.quantity,
                        average_price: blended_average(
                            current.quantity,
                            current.average_price,
                            fill.quantity,
                            fill.price,
                        ),
                        realized_pnl: current.realized_pnl,
                    },
                };
                next.positions.insert(fill.symbol.clone(), updated);
            }
            OrderSide::Sell => {
                let current = entry.ok_or_else(|| DomainError::InsufficientPosition {
                    symbol: fill.symbol.to_string(),
                    requested: fill.quantity,
                    held: 0,
                })?;
                if current.quantity < fill.quantity {
                    return Err(DomainError::InsufficientPosition {
                        symbol: fill.symbol.to_string(),
                        requested: fill.quantity,
                        held: current.quantity,
                    });
                }
                let realized = (fill.price - current.average_price) * Decimal::from(fill.quantity);
                let updated = PositionState {
                    symbol: fill.symbol.clone(),
                    quantity: current.quantity - fill.quantity,
                    average_price: current.average_price,
                    realized_pnl: current.realized_pnl + realized,
                };
                next.positions.insert(fill.symbol.clone(), updated);
            }
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Symbol;

    fn symbol(code: &str) -> Symbol {
        Symbol::parse(code).unwrap()
    }

    fn fill(code: &str, side: OrderSide, qty: i64, price: i64) -> Fill {
        Fill {
            client_order_id: "c-1".into(),
            symbol: symbol(code),
            side,
            quantity: qty,
            price: Decimal::from(price),
        }
    }

    #[test]
    fn buy_creates_position_at_fill_price() {
        let portfolio = Portfolio::new()
            .apply_fill(fill("005930", OrderSide::Buy, 10, 80_000))
            .unwrap();
        let position = portfolio.position(&symbol("005930")).unwrap();
        assert_eq!(position.quantity, 10);
        assert_eq!(position.average_price, Decimal::from(80_000));
    }

    #[test]
    fn average_price_blends_across_buys() {
        let portfolio = Portfolio::new()
            .apply_fill(fill("005930", OrderSide::Buy, 10, 80_000))
            .unwrap()
            .apply_fill(fill("005930", OrderSide::Buy, 10, 82_000))
            .unwrap();
        let position = portfolio.position(&symbol("005930")).unwrap();
        assert_eq!(position.quantity, 20);
        assert_eq!(position.average_price, Decimal::from(81_000));
    }

    #[test]
    fn sell_keeps_average_price_and_books_realized_pnl() {
        let portfolio = Portfolio::new()
            .apply_fill(fill("005930", OrderSide::Buy, 10, 80_000))
            .unwrap()
            .apply_fill(fill("005930", OrderSide::Sell, 4, 85_000))
            .unwrap();
        let position = portfolio.position(&symbol("005930")).unwrap();
        assert_eq!(position.quantity, 6);
        assert_eq!(position.average_price, Decimal::from(80_000));
        assert_eq!(position.realized_pnl, Decimal::from(20_000)); // (85000-80000) * 4
        assert_eq!(portfolio.total_realized_pnl(), Decimal::from(20_000));
    }

    #[test]
    fn full_sell_leaves_zero_quantity_entry() {
        let portfolio = Portfolio::new()
            .apply_fill(fill("005930", OrderSide::Buy, 10, 80_000))
            .unwrap()
            .apply_fill(fill("005930", OrderSide::Sell, 10, 90_000))
            .unwrap();
        let position = portfolio.position(&symbol("005930")).unwrap();
        assert_eq!(position.quantity, 0);
        assert_eq!(position.realized_pnl, Decimal::from(100_000));
    }

    #[test]
    fn oversell_and_bare_sell_rejected() {
        let portfolio = Portfolio::new()
            .apply_fill(fill("005930", OrderSide::Buy, 5, 80_000))
            .unwrap();
        assert!(matches!(
            portfolio.apply_fill(fill("005930", OrderSide::Sell, 6, 85_000)),
            Err(DomainError::InsufficientPosition {
                requested: 6,
                held: 5,
                ..
            })
        ));
        assert!(matches!(
            Portfolio::new().apply_fill(fill("005930", OrderSide::Sell, 1, 80_000)),
            Err(DomainError::InsufficientPosition {
                requested: 1,
                held: 0,
                ..
            })
        ));
    }

    #[test]
    fn positions_are_independent() {
        let portfolio = Portfolio::new()
            .apply_fill(fill("005930", OrderSide::Buy, 10, 80_000))
            .unwrap()
            .apply_fill(fill("035420", OrderSide::Buy, 3, 41_000))
            .unwrap();
        assert_eq!(
            portfolio.position(&symbol("005930")).unwrap().average_price,
            Decimal::from(80_000)
        );
        assert_eq!(
            portfolio.position(&symbol("035420")).unwrap().average_price,
            Decimal::from(41_000)
        );
    }

    #[test]
    fn blending_handles_fractional_averages_exactly() {
        let portfolio = Portfolio::new()
            .apply_fill(fill("005930", OrderSide::Buy, 3, 10))
            .unwrap()
            .apply_fill(fill("005930", OrderSide::Buy, 4, 20))
            .unwrap();
        // (3*10 + 4*20) / 7 = 110/7 — exact Decimal, no float drift.
        assert_eq!(
            portfolio.position(&symbol("005930")).unwrap().average_price,
            Decimal::from(110) / Decimal::from(7)
        );
    }
}
