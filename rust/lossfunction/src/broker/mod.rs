//! Broker abstraction — the only boundary where venue specifics may appear.

mod mock;

pub use mock::MockBroker;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::types::{OrderSide, OrderType, Price, Quantity, Quote, Symbol};

/// A pre-risk-checked order intent ready for submission.
///
/// `client_order_id` is the idempotency key: brokers treat resubmission of
/// the same id as the same order, never a duplicate one.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderRequest {
    pub client_order_id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub limit_price: Option<Price>,
}

/// Broker acceptance of an order submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderAck {
    pub client_order_id: String,
    pub broker_order_id: String,
}

/// Current execution state of an order as known by the broker.
///
/// Orders with remaining quantity and open status are what restart
/// reconciliation must resolve — the broker is the source of truth.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    pub broker_order_id: String,
    pub client_order_id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub order_quantity: Quantity,
    pub filled_quantity: Quantity,
    pub average_fill_price: Option<Price>,
    pub open: bool,
    pub timestamp: DateTime<Utc>,
}

/// A held quantity of one symbol with its average acquisition price.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub symbol: Symbol,
    pub quantity: Quantity,
    pub average_price: Price,
}

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("no quote configured for symbol {0}")]
    UnknownSymbol(String),
    #[error("order {0} is not open")]
    NotOpen(String),
    #[error("insufficient position for {symbol} to sell")]
    InsufficientPosition { symbol: String },
    #[error("order {0} not found")]
    UnknownOrder(String),
    #[error("broker failure: {0}")]
    Internal(String),
}

/// Abstract venue interface for orders, balances, and quotes.
#[async_trait::async_trait]
pub trait Broker: Send + Sync {
    /// Submit an order; idempotent on `client_order_id`.
    async fn submit_order(&self, request: &OrderRequest) -> Result<OrderAck, BrokerError>;

    /// Request cancellation of the full remainder of an open order.
    async fn cancel_order(&self, broker_order_id: &str) -> Result<(), BrokerError>;

    /// Broker-side execution state of one order (source of truth after
    /// timeouts and restarts).
    async fn execution_report(&self, broker_order_id: &str)
        -> Result<ExecutionReport, BrokerError>;

    /// Positions currently held at the venue.
    async fn positions(&self) -> Result<Vec<Position>, BrokerError>;

    /// Recent quote snapshot for one symbol.
    async fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError>;
}

/// Fills limit orders when the market crosses them, at the limit price.
pub(crate) fn limit_crosses(side: OrderSide, market: Decimal, limit: Decimal) -> bool {
    match side {
        OrderSide::Buy => market <= limit,
        OrderSide::Sell => market >= limit,
    }
}
