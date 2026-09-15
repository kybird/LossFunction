//! In-memory broker for tests and the backtester.
//!
//! Fills market orders immediately at the configured price; limit orders
//! fill when the market crosses them (at the limit price) and otherwise
//! stay open. Deterministic: the same script of calls yields the same state.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use rust_decimal::Decimal;

use super::{
    limit_crosses, Broker, BrokerError, ExecutionReport, OrderAck, OrderRequest, Position,
};
use crate::domain::blended_average;
use crate::types::{OrderSide, OrderType, Quantity, Quote, Symbol};

struct Inner {
    prices: HashMap<Symbol, Decimal>,
    acks: HashMap<String, OrderAck>, // client_order_id -> ack
    reports: HashMap<String, ExecutionReport>, // broker_order_id -> report
    positions: HashMap<Symbol, (Quantity, Decimal)>,
    next_broker_id: u64,
}

/// Deterministic in-memory [`Broker`] implementation.
pub struct MockBroker {
    inner: Mutex<Inner>,
}

impl MockBroker {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                prices: HashMap::new(),
                acks: HashMap::new(),
                reports: HashMap::new(),
                positions: HashMap::new(),
                next_broker_id: 1,
            }),
        }
    }

    /// Configure the quote price used for subsequent fills.
    pub fn set_price(&self, symbol: &Symbol, price: Decimal) {
        self.inner
            .lock()
            .expect("mock broker lock")
            .prices
            .insert(symbol.clone(), price);
    }
}

impl Default for MockBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Broker for MockBroker {
    async fn submit_order(&self, request: &OrderRequest) -> Result<OrderAck, BrokerError> {
        let mut inner = self.inner.lock().expect("mock broker lock");
        if let Some(existing) = inner.acks.get(&request.client_order_id) {
            return Ok(existing.clone());
        }

        let price = *inner
            .prices
            .get(&request.symbol)
            .ok_or_else(|| BrokerError::UnknownSymbol(request.symbol.to_string()))?;

        let broker_order_id = format!("MOCK-{}", inner.next_broker_id);
        inner.next_broker_id += 1;
        let ack = OrderAck {
            client_order_id: request.client_order_id.clone(),
            broker_order_id: broker_order_id.clone(),
        };

        let fills_now = request.order_type == OrderType::Market
            || request
                .limit_price
                .is_some_and(|limit| limit_crosses(request.side, price, limit));
        let fill_price = match request.order_type {
            OrderType::Market => price,
            OrderType::Limit => request.limit_price.unwrap_or(price),
        };

        let report = if fills_now {
            apply_fill(&mut inner, request, fill_price);
            ExecutionReport {
                broker_order_id,
                client_order_id: request.client_order_id.clone(),
                symbol: request.symbol.clone(),
                side: request.side,
                order_type: request.order_type,
                order_quantity: request.quantity,
                filled_quantity: request.quantity,
                average_fill_price: Some(fill_price),
                open: false,
                timestamp: Utc::now(),
            }
        } else {
            ExecutionReport {
                broker_order_id,
                client_order_id: request.client_order_id.clone(),
                symbol: request.symbol.clone(),
                side: request.side,
                order_type: request.order_type,
                order_quantity: request.quantity,
                filled_quantity: 0,
                average_fill_price: None,
                open: true,
                timestamp: Utc::now(),
            }
        };

        inner.reports.insert(report.broker_order_id.clone(), report);
        inner
            .acks
            .insert(request.client_order_id.clone(), ack.clone());
        Ok(ack)
    }

    async fn cancel_order(&self, broker_order_id: &str) -> Result<(), BrokerError> {
        let mut inner = self.inner.lock().expect("mock broker lock");
        let report = inner
            .reports
            .get_mut(broker_order_id)
            .ok_or_else(|| BrokerError::UnknownOrder(broker_order_id.to_string()))?;
        if !report.open {
            return Err(BrokerError::NotOpen(broker_order_id.to_string()));
        }
        report.open = false;
        report.timestamp = Utc::now();
        Ok(())
    }

    async fn execution_report(
        &self,
        broker_order_id: &str,
    ) -> Result<ExecutionReport, BrokerError> {
        self.inner
            .lock()
            .expect("mock broker lock")
            .reports
            .get(broker_order_id)
            .cloned()
            .ok_or_else(|| BrokerError::UnknownOrder(broker_order_id.to_string()))
    }

    async fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        let inner = self.inner.lock().expect("mock broker lock");
        let mut positions: Vec<Position> = inner
            .positions
            .iter()
            .map(|(symbol, (quantity, average_price))| Position {
                symbol: symbol.clone(),
                quantity: *quantity,
                average_price: *average_price,
            })
            .collect();
        positions.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok(positions)
    }

    async fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        let inner = self.inner.lock().expect("mock broker lock");
        let price = inner
            .prices
            .get(symbol)
            .ok_or_else(|| BrokerError::UnknownSymbol(symbol.to_string()))?;
        Ok(Quote {
            symbol: symbol.clone(),
            last_price: *price,
            timestamp: Utc::now(),
        })
    }
}

fn apply_fill(inner: &mut Inner, request: &OrderRequest, price: Decimal) {
    let entry = inner.positions.get(&request.symbol).cloned();
    match request.side {
        OrderSide::Buy => {
            let updated = match entry {
                None => (request.quantity, price),
                Some((quantity, average)) => (
                    quantity + request.quantity,
                    blended_average(quantity, average, request.quantity, price),
                ),
            };
            inner.positions.insert(request.symbol.clone(), updated);
        }
        OrderSide::Sell => {
            let Some((quantity, average)) = entry else {
                // The mock mirrors the domain rule; unreachable when the
                // caller respects sell guards.
                return;
            };
            if quantity < request.quantity {
                return;
            }
            let remaining = quantity - request.quantity;
            if remaining == 0 {
                inner.positions.remove(&request.symbol);
            } else {
                inner
                    .positions
                    .insert(request.symbol.clone(), (remaining, average));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Symbol;
    use std::str::FromStr;

    fn symbol() -> Symbol {
        Symbol::parse("005930").unwrap()
    }

    fn request(id: &str) -> OrderRequest {
        OrderRequest {
            client_order_id: id.into(),
            symbol: symbol(),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: 10,
            limit_price: None,
        }
    }

    fn broker() -> MockBroker {
        let broker = MockBroker::new();
        broker.set_price(&symbol(), Decimal::from(80_000));
        broker
    }

    #[tokio::test]
    async fn market_order_fills_and_updates_position() {
        let broker = broker();
        let ack = broker.submit_order(&request("c-1")).await.unwrap();
        let report = broker.execution_report(&ack.broker_order_id).await.unwrap();
        assert_eq!(report.filled_quantity, 10);
        assert_eq!(report.average_fill_price, Some(Decimal::from(80_000)));
        assert!(!report.open);

        let positions = broker.positions().await.unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, 10);
        assert_eq!(positions[0].average_price, Decimal::from(80_000));
    }

    #[tokio::test]
    async fn submit_is_idempotent_on_client_order_id() {
        let broker = broker();
        let first = broker.submit_order(&request("c-1")).await.unwrap();
        let again = broker.submit_order(&request("c-1")).await.unwrap();
        assert_eq!(first, again);
        let positions = broker.positions().await.unwrap();
        assert_eq!(positions[0].quantity, 10); // not 20 — no duplicate fill
    }

    #[tokio::test]
    async fn average_price_blends_across_buys() {
        let broker = broker();
        broker.submit_order(&request("c-1")).await.unwrap();
        broker.set_price(&symbol(), Decimal::from(82_000));
        broker.submit_order(&request("c-2")).await.unwrap();
        let positions = broker.positions().await.unwrap();
        assert_eq!(positions[0].quantity, 20);
        assert_eq!(positions[0].average_price, Decimal::from(81_000));
    }

    #[tokio::test]
    async fn limit_cross_fills_at_limit_price() {
        let broker = broker();
        let request = OrderRequest {
            client_order_id: "c-l".into(),
            symbol: symbol(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: 10,
            limit_price: Some(Decimal::from_str("80000.5").unwrap()), // >= market 80000
        };
        let ack = broker.submit_order(&request).await.unwrap();
        let report = broker.execution_report(&ack.broker_order_id).await.unwrap();
        assert!(!report.open);
        assert_eq!(
            report.average_fill_price,
            Some(Decimal::from_str("80000.5").unwrap())
        );
    }

    #[tokio::test]
    async fn limit_below_market_stays_open_until_cancelled() {
        let broker = broker();
        let request = OrderRequest {
            client_order_id: "c-l".into(),
            symbol: symbol(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: 10,
            limit_price: Some(Decimal::from(79_000)),
        };
        let ack = broker.submit_order(&request).await.unwrap();
        let report = broker.execution_report(&ack.broker_order_id).await.unwrap();
        assert!(report.open);
        assert_eq!(report.filled_quantity, 0);

        broker.cancel_order(&ack.broker_order_id).await.unwrap();
        let report = broker.execution_report(&ack.broker_order_id).await.unwrap();
        assert!(!report.open);

        assert!(matches!(
            broker.cancel_order(&ack.broker_order_id).await,
            Err(BrokerError::NotOpen(_))
        ));
    }

    #[tokio::test]
    async fn sell_reduces_position_and_keeps_average() {
        let broker = broker();
        broker.submit_order(&request("c-1")).await.unwrap();
        let sell = OrderRequest {
            client_order_id: "c-2".into(),
            symbol: symbol(),
            side: OrderSide::Sell,
            order_type: OrderType::Market,
            quantity: 4,
            limit_price: None,
        };
        broker.submit_order(&sell).await.unwrap();
        let positions = broker.positions().await.unwrap();
        assert_eq!(positions[0].quantity, 6);
        assert_eq!(positions[0].average_price, Decimal::from(80_000));

        let unknown = Symbol::parse("999999").unwrap();
        assert!(matches!(
            broker.quote(&unknown).await,
            Err(BrokerError::UnknownSymbol(_))
        ));
    }
}
