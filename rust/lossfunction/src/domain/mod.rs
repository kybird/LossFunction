//! Pure domain models — no I/O, no clocks, no infrastructure.

pub mod order;
pub mod portfolio;

pub use order::{Order, OrderStatus};
pub use portfolio::{Fill, Portfolio, PositionState};

use rust_decimal::Decimal;

/// Business-rule violations shared by the domain.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DomainError {
    #[error("order quantity must be positive, got {quantity}")]
    NonPositiveQuantity { quantity: i64 },
    #[error("limit orders require a positive limit_price")]
    LimitNeedsPrice,
    #[error("market orders must not carry a limit_price")]
    MarketCarriesPrice,
    #[error("filled_quantity {filled} outside [0, {quantity}]")]
    FilledOutOfRange { filled: i64, quantity: i64 },
    #[error("cannot {action} order in status {status}")]
    WrongStatus {
        action: &'static str,
        status: OrderStatus,
    },
    #[error("amended quantity {quantity} below already-filled {filled}")]
    AmendBelowFilled { quantity: i64, filled: i64 },
    #[error("insufficient position for {symbol}: sell {requested}, held {held}")]
    InsufficientPosition {
        symbol: String,
        requested: i64,
        held: i64,
    },
}

/// Exact average price over `quantity` shares at `price`.
pub(crate) fn blended_average(
    current_qty: i64,
    current_avg: Decimal,
    add_qty: i64,
    add_price: Decimal,
) -> Decimal {
    let total_cost = current_avg * Decimal::from(current_qty) + add_price * Decimal::from(add_qty);
    total_cost / Decimal::from(current_qty + add_qty)
}
