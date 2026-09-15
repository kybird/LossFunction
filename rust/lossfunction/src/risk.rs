//! Pre-trade risk checks gating every order.
//!
//! Check order is load-bearing (wiki: risk-layer): kill switch first — it
//! blocks even without quotes — then quote presence, staleness, order
//! notional, per-symbol position cap (buys only), gross exposure, and the
//! daily realized-loss limit. All checks are pure functions of
//! (order, portfolio, quotes, limits, kill state).

use chrono::{DateTime, TimeDelta, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::config::RiskSettings;
use crate::domain::order::Order;
use crate::domain::portfolio::Portfolio;
use crate::types::{OrderSide, Quote, Symbol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskLimits {
    pub max_order_notional: Decimal,
    pub max_position_quantity: i64,
    pub max_gross_exposure: Decimal,
    pub daily_loss_limit: Decimal,
    pub stale_quote_max_age: TimeDelta,
}

impl From<RiskSettings> for RiskLimits {
    fn from(settings: RiskSettings) -> Self {
        Self {
            max_order_notional: Decimal::from(settings.max_order_notional),
            max_position_quantity: settings.max_position_quantity,
            max_gross_exposure: Decimal::from(settings.gross_exposure),
            daily_loss_limit: Decimal::from(settings.daily_loss_limit),
            stale_quote_max_age: TimeDelta::from_std(Duration::from_secs(
                settings.stale_quote_seconds,
            ))
            .expect("stale_quote_seconds within range"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    KillSwitch,
    NoQuote,
    StaleMarketData,
    OrderNotionalExceeded,
    PositionQuantityExceeded,
    GrossExposureExceeded,
    DailyLossLimitExceeded,
}

impl RejectionReason {
    fn as_str(&self) -> &'static str {
        match self {
            RejectionReason::KillSwitch => "kill_switch",
            RejectionReason::NoQuote => "no_quote",
            RejectionReason::StaleMarketData => "stale_market_data",
            RejectionReason::OrderNotionalExceeded => "order_notional_exceeded",
            RejectionReason::PositionQuantityExceeded => "position_quantity_exceeded",
            RejectionReason::GrossExposureExceeded => "gross_exposure_exceeded",
            RejectionReason::DailyLossLimitExceeded => "daily_loss_limit_exceeded",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("{}: {}", .reason.as_str(), detail)]
pub struct OrderRejected {
    pub reason: RejectionReason,
    pub detail: String,
}

struct KillState {
    active: bool,
    reason: Option<String>,
}

/// Stateful only for the kill switch; everything else derives from inputs.
pub struct RiskManager {
    limits: RiskLimits,
    kill: Mutex<KillState>,
    now: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl RiskManager {
    pub fn new(limits: RiskLimits) -> Self {
        Self {
            limits,
            kill: Mutex::new(KillState {
                active: false,
                reason: None,
            }),
            now: Box::new(Utc::now),
        }
    }

    pub fn with_clock(
        limits: RiskLimits,
        now: impl Fn() -> DateTime<Utc> + Send + Sync + 'static,
    ) -> Self {
        Self {
            limits,
            kill: Mutex::new(KillState {
                active: false,
                reason: None,
            }),
            now: Box::new(now),
        }
    }

    pub fn kill_switch_active(&self) -> bool {
        self.kill.lock().expect("kill lock").active
    }

    pub fn kill_reason(&self) -> Option<String> {
        self.kill.lock().expect("kill lock").reason.clone()
    }

    pub fn activate_kill_switch(&self, reason: impl Into<String>) {
        let mut kill = self.kill.lock().expect("kill lock");
        kill.active = true;
        kill.reason = Some(reason.into());
    }

    pub fn deactivate_kill_switch(&self) {
        let mut kill = self.kill.lock().expect("kill lock");
        kill.active = false;
        kill.reason = None;
    }

    /// Validate one order; returns the order notional or rejects.
    pub fn check_order(
        &self,
        order: &Order,
        portfolio: &Portfolio,
        quotes: &HashMap<Symbol, Quote>,
    ) -> Result<Decimal, OrderRejected> {
        if self.kill_switch_active() {
            return Err(OrderRejected {
                reason: RejectionReason::KillSwitch,
                detail: format!(
                    "kill switch active: {}",
                    self.kill_reason().unwrap_or_default()
                ),
            });
        }

        let quote = quotes.get(order.symbol()).ok_or_else(|| OrderRejected {
            reason: RejectionReason::NoQuote,
            detail: format!("no quote available for {}", order.symbol()),
        })?;
        let age = (self.now)() - quote.timestamp;
        if age > self.limits.stale_quote_max_age {
            return Err(OrderRejected {
                reason: RejectionReason::StaleMarketData,
                detail: format!(
                    "quote for {} is {}s old (max {}s)",
                    order.symbol(),
                    age.num_seconds(),
                    self.limits.stale_quote_max_age.num_seconds()
                ),
            });
        }

        let price = order.limit_price().unwrap_or(quote.last_price);
        let notional = Decimal::from(order.quantity()) * price;
        if notional > self.limits.max_order_notional {
            return Err(OrderRejected {
                reason: RejectionReason::OrderNotionalExceeded,
                detail: format!(
                    "order notional {notional} > max {}",
                    self.limits.max_order_notional
                ),
            });
        }

        let held = portfolio
            .position(order.symbol())
            .map(|position| position.quantity)
            .unwrap_or(0);
        if order.side() == OrderSide::Buy {
            if held + order.quantity() > self.limits.max_position_quantity {
                return Err(OrderRejected {
                    reason: RejectionReason::PositionQuantityExceeded,
                    detail: format!(
                        "projected {} quantity {} > max {}",
                        order.symbol(),
                        held + order.quantity(),
                        self.limits.max_position_quantity
                    ),
                });
            }
            let gross_after = gross_exposure(portfolio, quotes) + notional;
            if gross_after > self.limits.max_gross_exposure {
                return Err(OrderRejected {
                    reason: RejectionReason::GrossExposureExceeded,
                    detail: format!(
                        "gross exposure {gross_after} > max {}",
                        self.limits.max_gross_exposure
                    ),
                });
            }
        }

        let realized = portfolio.total_realized_pnl();
        if realized < -self.limits.daily_loss_limit {
            return Err(OrderRejected {
                reason: RejectionReason::DailyLossLimitExceeded,
                detail: format!(
                    "realized loss {realized} beyond limit -{}",
                    self.limits.daily_loss_limit
                ),
            });
        }

        Ok(notional)
    }
}

/// Quote-valued gross exposure; positions without a quote fall back to
/// their average price.
fn gross_exposure(portfolio: &Portfolio, quotes: &HashMap<Symbol, Quote>) -> Decimal {
    portfolio
        .positions()
        .filter(|position| position.quantity > 0)
        .map(|position| {
            let price = quotes
                .get(&position.symbol)
                .map(|quote| quote.last_price)
                .unwrap_or(position.average_price);
            Decimal::from(position.quantity) * price
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::RestoredOrder;
    use crate::domain::portfolio::Fill;
    use crate::types::{OrderType, Symbol};
    use chrono::TimeZone;

    const NOW: chrono::DateTime<Utc> = chrono::DateTime::<Utc>::UNIX_EPOCH;

    fn limits() -> RiskLimits {
        RiskLimits {
            max_order_notional: Decimal::from(10_000_000),
            max_position_quantity: 100,
            max_gross_exposure: Decimal::from(30_000_000),
            daily_loss_limit: Decimal::from(500_000),
            stale_quote_max_age: TimeDelta::seconds(10),
        }
    }

    fn now_fn() -> impl Fn() -> DateTime<Utc> + Send + Sync + Clone + 'static {
        let now = NOW;
        move || now
    }

    fn symbol(code: &str) -> Symbol {
        Symbol::parse(code).unwrap()
    }

    fn order(quantity: i64) -> Order {
        Order::restored(RestoredOrder {
            client_order_id: "c-1".into(),
            symbol: symbol("005930"),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity,
            limit_price: None,
            status: crate::domain::order::OrderStatus::Pending,
            filled_quantity: 0,
        })
        .unwrap()
    }

    fn quotes(now: DateTime<Utc>, price: i64) -> HashMap<Symbol, Quote> {
        let mut map = HashMap::new();
        map.insert(
            symbol("005930"),
            Quote {
                symbol: symbol("005930"),
                last_price: Decimal::from(price),
                timestamp: now,
            },
        );
        map
    }

    #[test]
    fn within_limits_passes_and_returns_notional() {
        let manager = RiskManager::with_clock(limits(), now_fn());
        let notional = manager
            .check_order(&order(10), &Portfolio::new(), &quotes(NOW, 80_000))
            .unwrap();
        assert_eq!(notional, Decimal::from(800_000));
    }

    #[test]
    fn notional_limit_blocks() {
        let mut limits = limits();
        limits.max_order_notional = Decimal::from(500_000);
        let manager = RiskManager::with_clock(limits, now_fn());
        let rejection = manager
            .check_order(&order(10), &Portfolio::new(), &quotes(NOW, 80_000))
            .unwrap_err();
        assert_eq!(rejection.reason, RejectionReason::OrderNotionalExceeded);
    }

    #[test]
    fn position_cap_blocks_buys_not_sells() {
        let portfolio = Portfolio::new()
            .apply_fill(Fill {
                client_order_id: "x".into(),
                symbol: symbol("005930"),
                side: OrderSide::Buy,
                quantity: 95,
                price: Decimal::from(80_000),
            })
            .unwrap();
        let manager = RiskManager::with_clock(limits(), now_fn());
        let rejection = manager
            .check_order(&order(10), &portfolio, &quotes(NOW, 80_000))
            .unwrap_err();
        assert_eq!(rejection.reason, RejectionReason::PositionQuantityExceeded);

        let sell = Order::restored(RestoredOrder {
            client_order_id: "c-2".into(),
            symbol: symbol("005930"),
            side: OrderSide::Sell,
            order_type: OrderType::Market,
            quantity: 10,
            limit_price: None,
            status: crate::domain::order::OrderStatus::Pending,
            filled_quantity: 0,
        })
        .unwrap();
        assert!(manager
            .check_order(&sell, &portfolio, &quotes(NOW, 80_000))
            .is_ok());
    }

    #[test]
    fn gross_exposure_blocks_with_multi_symbol_book() {
        let mut map = HashMap::new();
        for code in ["005930", "035420", "069500"] {
            map.insert(
                symbol(code),
                Quote {
                    symbol: symbol(code),
                    last_price: Decimal::from(200_000),
                    timestamp: NOW,
                },
            );
        }
        let mut portfolio = Portfolio::new();
        for code in ["005930", "035420"] {
            portfolio = portfolio
                .apply_fill(Fill {
                    client_order_id: "x".into(),
                    symbol: symbol(code),
                    side: OrderSide::Buy,
                    quantity: 100,
                    price: Decimal::from(200_000),
                })
                .unwrap();
        }
        // Order the third symbol (held 0, so the per-symbol cap stays out of
        // the way — gross exposure must be the firing check).
        let fresh_symbol_order = Order::restored(RestoredOrder {
            client_order_id: "c-3".into(),
            symbol: symbol("069500"),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: 10,
            limit_price: None,
            status: crate::domain::order::OrderStatus::Pending,
            filled_quantity: 0,
        })
        .unwrap();
        let manager = RiskManager::with_clock(limits(), now_fn());
        let rejection = manager
            .check_order(&fresh_symbol_order, &portfolio, &map)
            .unwrap_err();
        assert_eq!(rejection.reason, RejectionReason::GrossExposureExceeded);
    }

    #[test]
    fn daily_loss_limit_blocks_all_sides() {
        let mut portfolio = Portfolio::new();
        for (side, price) in [(OrderSide::Buy, 80_000i64), (OrderSide::Sell, 74_000)] {
            portfolio = portfolio
                .apply_fill(Fill {
                    client_order_id: "x".into(),
                    symbol: symbol("005930"),
                    side,
                    quantity: 100,
                    price: Decimal::from(price),
                })
                .unwrap();
        }
        let manager = RiskManager::with_clock(limits(), now_fn());
        for side in [OrderSide::Buy, OrderSide::Sell] {
            let order = Order::restored(RestoredOrder {
                client_order_id: "c".into(),
                symbol: symbol("005930"),
                side,
                order_type: OrderType::Market,
                quantity: 1,
                limit_price: None,
                status: crate::domain::order::OrderStatus::Pending,
                filled_quantity: 0,
            })
            .unwrap();
            let rejection = manager
                .check_order(&order, &portfolio, &quotes(NOW, 80_000))
                .unwrap_err();
            assert_eq!(rejection.reason, RejectionReason::DailyLossLimitExceeded);
        }
    }

    #[test]
    fn stale_or_missing_quote_blocks() {
        let manager = RiskManager::with_clock(limits(), now_fn());
        let stale = Utc.timestamp_opt(1, 0).unwrap() - TimeDelta::seconds(30);
        assert_eq!(
            manager
                .check_order(&order(1), &Portfolio::new(), &quotes(stale, 80_000))
                .unwrap_err()
                .reason,
            RejectionReason::StaleMarketData
        );
        assert_eq!(
            manager
                .check_order(&order(1), &Portfolio::new(), &HashMap::new())
                .unwrap_err()
                .reason,
            RejectionReason::NoQuote
        );
    }

    #[test]
    fn kill_switch_blocks_every_path_and_precedes_quotes() {
        let manager = RiskManager::with_clock(limits(), now_fn());
        manager.activate_kill_switch("manual halt");
        assert!(manager.kill_switch_active());
        assert_eq!(manager.kill_reason().as_deref(), Some("manual halt"));

        // Even without any quotes the kill switch is the reported reason.
        let rejection = manager
            .check_order(&order(1), &Portfolio::new(), &HashMap::new())
            .unwrap_err();
        assert_eq!(rejection.reason, RejectionReason::KillSwitch);

        manager.deactivate_kill_switch();
        assert!(!manager.kill_switch_active());
        assert!(manager
            .check_order(&order(1), &Portfolio::new(), &quotes(NOW, 80_000))
            .is_ok());
    }
}
