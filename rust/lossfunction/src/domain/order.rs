//! Order aggregate: shape invariants plus amend/cancel rules.
//!
//! The full transition table lives in the execution layer; this module
//! enforces order-shaped invariants and amend/cancel guards only.

use crate::types::{OrderSide, OrderType, Price, Quantity, Symbol};

use super::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderStatus {
    Pending,
    Submitted,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Unknown,
}

impl OrderStatus {
    pub fn amendable(&self) -> bool {
        // Partially-filled orders stay amendable: the amendment applies to
        // the remainder and must stay >= the already-filled part.
        matches!(
            self,
            OrderStatus::Pending | OrderStatus::Submitted | OrderStatus::PartiallyFilled
        )
    }

    pub fn terminal(&self) -> bool {
        matches!(
            self,
            OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected
        )
    }
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            OrderStatus::Pending => "pending",
            OrderStatus::Submitted => "submitted",
            OrderStatus::PartiallyFilled => "partially_filled",
            OrderStatus::Filled => "filled",
            OrderStatus::Cancelled => "cancelled",
            OrderStatus::Rejected => "rejected",
            OrderStatus::Unknown => "unknown",
        };
        f.write_str(name)
    }
}

/// Parameters for restoring an order at a given status (restart recovery).
#[derive(Debug, Clone)]
pub struct RestoredOrder {
    pub client_order_id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub limit_price: Option<Price>,
    pub status: OrderStatus,
    pub filled_quantity: Quantity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    client_order_id: String,
    symbol: Symbol,
    side: OrderSide,
    order_type: OrderType,
    quantity: Quantity,
    limit_price: Option<Price>,
    status: OrderStatus,
    filled_quantity: Quantity,
}

impl Order {
    pub fn new(
        client_order_id: impl Into<String>,
        symbol: Symbol,
        side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        limit_price: Option<Price>,
    ) -> Result<Self, DomainError> {
        let order = Self {
            client_order_id: client_order_id.into(),
            symbol,
            side,
            order_type,
            quantity,
            limit_price,
            status: OrderStatus::Pending,
            filled_quantity: 0,
        };
        order.validate_shape()?;
        Ok(order)
    }

    fn validate_shape(&self) -> Result<(), DomainError> {
        if self.quantity <= 0 {
            return Err(DomainError::NonPositiveQuantity {
                quantity: self.quantity,
            });
        }
        match (self.order_type, self.limit_price) {
            (OrderType::Limit, None) => return Err(DomainError::LimitNeedsPrice),
            (OrderType::Limit, Some(price)) if price <= Price::ZERO => {
                return Err(DomainError::LimitNeedsPrice);
            }
            (OrderType::Market, Some(_)) => return Err(DomainError::MarketCarriesPrice),
            _ => {}
        }
        if self.filled_quantity < 0 || self.filled_quantity > self.quantity {
            return Err(DomainError::FilledOutOfRange {
                filled: self.filled_quantity,
                quantity: self.quantity,
            });
        }
        Ok(())
    }

    pub fn client_order_id(&self) -> &str {
        &self.client_order_id
    }
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }
    pub fn side(&self) -> OrderSide {
        self.side
    }
    pub fn order_type(&self) -> OrderType {
        self.order_type
    }
    pub fn quantity(&self) -> Quantity {
        self.quantity
    }
    pub fn limit_price(&self) -> Option<Price> {
        self.limit_price
    }
    pub fn status(&self) -> OrderStatus {
        self.status
    }
    pub fn filled_quantity(&self) -> Quantity {
        self.filled_quantity
    }

    /// Build an order directly at a status — restart recovery path only.
    pub fn restored(restored: RestoredOrder) -> Result<Self, DomainError> {
        let mut order = Self::new(
            restored.client_order_id,
            restored.symbol,
            restored.side,
            restored.order_type,
            restored.quantity,
            restored.limit_price,
        )?;
        order.status = restored.status;
        order.filled_quantity = restored.filled_quantity;
        order.validate_shape()?;
        Ok(order)
    }

    /// Return a new order with amended terms (self stays untouched).
    pub fn amend(
        &self,
        quantity: Option<Quantity>,
        limit_price: Option<Price>,
    ) -> Result<Self, DomainError> {
        if !self.status.amendable() {
            return Err(DomainError::WrongStatus {
                action: "amend",
                status: self.status,
            });
        }
        let new_quantity = quantity.unwrap_or(self.quantity);
        if new_quantity < self.filled_quantity {
            return Err(DomainError::AmendBelowFilled {
                quantity: new_quantity,
                filled: self.filled_quantity,
            });
        }
        let mut amended = self.clone();
        amended.quantity = new_quantity;
        if let Some(price) = limit_price {
            amended.limit_price = Some(price);
            amended.validate_shape()?;
        }
        Ok(amended)
    }

    /// Return a new order marked cancelled (remainder after partial fills).
    pub fn cancel(&self) -> Result<Self, DomainError> {
        if self.status.terminal() {
            return Err(DomainError::WrongStatus {
                action: "cancel",
                status: self.status,
            });
        }
        let mut cancelled = self.clone();
        cancelled.status = OrderStatus::Cancelled;
        Ok(cancelled)
    }

    /// Transition hook used by the execution layer (single mutation point).
    pub fn with_status(
        &self,
        status: OrderStatus,
        filled_quantity: Quantity,
    ) -> Result<Self, DomainError> {
        let mut next = self.clone();
        next.status = status;
        next.filled_quantity = filled_quantity;
        next.validate_shape()?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Symbol;
    use rust_decimal::Decimal;

    fn symbol() -> Symbol {
        Symbol::parse("005930").unwrap()
    }

    fn restored(status: OrderStatus, filled: i64) -> Result<Order, DomainError> {
        Order::restored(RestoredOrder {
            client_order_id: "c".into(),
            symbol: symbol(),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: 10,
            limit_price: None,
            status,
            filled_quantity: filled,
        })
    }

    fn order() -> Order {
        Order::new("c-1", symbol(), OrderSide::Buy, OrderType::Market, 10, None).unwrap()
    }

    #[test]
    fn shape_invariants() {
        assert!(matches!(
            Order::new("c", symbol(), OrderSide::Buy, OrderType::Market, 0, None).unwrap_err(),
            DomainError::NonPositiveQuantity { quantity: 0 }
        ));
        assert!(matches!(
            Order::new("c", symbol(), OrderSide::Buy, OrderType::Limit, 10, None).unwrap_err(),
            DomainError::LimitNeedsPrice
        ));
        assert!(matches!(
            Order::new(
                "c",
                symbol(),
                OrderSide::Buy,
                OrderType::Limit,
                10,
                Some(Decimal::ZERO)
            )
            .unwrap_err(),
            DomainError::LimitNeedsPrice
        ));
        assert!(matches!(
            Order::new(
                "c",
                symbol(),
                OrderSide::Buy,
                OrderType::Market,
                10,
                Some(Decimal::from(80_000))
            )
            .unwrap_err(),
            DomainError::MarketCarriesPrice
        ));
        assert!(matches!(
            restored(OrderStatus::Filled, 11).unwrap_err(),
            DomainError::FilledOutOfRange {
                filled: 11,
                quantity: 10
            }
        ));
    }

    #[test]
    fn amend_returns_new_instance_and_respects_guards() {
        let original = Order::new(
            "c",
            symbol(),
            OrderSide::Buy,
            OrderType::Limit,
            10,
            Some(Decimal::from(79_000)),
        )
        .unwrap();

        let amended = original
            .amend(Some(20), Some(Decimal::from(78_500)))
            .unwrap();
        assert_eq!(amended.quantity(), 20);
        assert_eq!(amended.limit_price(), Some(Decimal::from(78_500)));
        assert_eq!(original.quantity(), 10); // original untouched

        let filled = restored(OrderStatus::Filled, 10).unwrap();
        assert!(matches!(
            filled.amend(Some(20), None).unwrap_err(),
            DomainError::WrongStatus {
                action: "amend",
                ..
            }
        ));

        let partial = restored(OrderStatus::PartiallyFilled, 8).unwrap();
        assert!(matches!(
            partial.amend(Some(5), None).unwrap_err(),
            DomainError::AmendBelowFilled {
                quantity: 5,
                filled: 8
            }
        ));
    }

    #[test]
    fn cancel_allowed_while_open_and_after_partial() {
        assert_eq!(order().cancel().unwrap().status(), OrderStatus::Cancelled);
        let partial = restored(OrderStatus::PartiallyFilled, 5).unwrap();
        assert_eq!(partial.cancel().unwrap().status(), OrderStatus::Cancelled);
    }

    #[test]
    fn cancel_rejected_for_terminal_statuses() {
        for status in [
            OrderStatus::Filled,
            OrderStatus::Cancelled,
            OrderStatus::Rejected,
        ] {
            let terminal = restored(status, 10).unwrap();
            assert!(matches!(
                terminal.cancel().unwrap_err(),
                DomainError::WrongStatus {
                    action: "cancel",
                    ..
                }
            ));
        }
    }
}
