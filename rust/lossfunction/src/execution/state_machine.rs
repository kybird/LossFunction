//! Order status state machine — the sole owner of order lifecycle transitions.
//!
//! Contract (wiki: order-lifecycle): a submission timeout or lost broker
//! answer moves the order to Unknown, never to a terminal failure —
//! reconciliation against the broker is the only way out. Terminal states
//! accept no further transitions. Every legal transition emits exactly one
//! event; an illegal transition errors and emits nothing.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::domain::order::OrderStatus;

/// from-status -> statuses it may move to.
pub fn transitions(from: OrderStatus) -> &'static [OrderStatus] {
    use OrderStatus::*;
    match from {
        Pending => &[Submitted, Rejected, Cancelled, Unknown],
        Submitted => &[PartiallyFilled, Filled, Cancelled, Rejected, Unknown],
        PartiallyFilled => &[PartiallyFilled, Filled, Cancelled, Unknown],
        Unknown => &[Submitted, PartiallyFilled, Filled, Cancelled, Rejected],
        Filled | Cancelled | Rejected => &[],
    }
}

/// Unknown is reachable only while a broker answer is outstanding.
fn unknown_entry_allowed_from(status: OrderStatus) -> bool {
    matches!(status, OrderStatus::Pending | OrderStatus::Submitted)
}

pub type TransitionCallback = Box<dyn Fn(&TransitionEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionEvent {
    pub client_order_id: String,
    pub from_status: OrderStatus,
    pub to_status: OrderStatus,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TransitionError {
    #[error("illegal transition for {client_order_id}: {from:?} -> {to:?}")]
    Illegal {
        client_order_id: String,
        from: OrderStatus,
        to: OrderStatus,
    },
    #[error(
        "UNKNOWN entry only allowed from an outstanding submission, not from {from:?} (order {client_order_id})"
    )]
    UnknownEntryBlocked {
        client_order_id: String,
        from: OrderStatus,
    },
    #[error("unknown order {0}; register() it first")]
    Unregistered(String),
    #[error("order {0} already registered")]
    AlreadyRegistered(String),
}

/// Tracks per-order status and enforces the transition table.
pub struct StateMachine {
    statuses: HashMap<String, OrderStatus>,
    history: HashMap<String, Vec<TransitionEvent>>,
    on_event: Option<TransitionCallback>,
    now: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl StateMachine {
    pub fn new() -> Self {
        Self {
            statuses: HashMap::new(),
            history: HashMap::new(),
            on_event: None,
            now: Box::new(Utc::now),
        }
    }

    pub fn with_on_event(on_event: impl Fn(&TransitionEvent) + Send + Sync + 'static) -> Self {
        Self {
            on_event: Some(Box::new(on_event)),
            ..Self::new()
        }
    }

    /// Introduce an order at a starting status (Pending, or a restored
    /// status after restart recovery).
    pub fn register(
        &mut self,
        client_order_id: impl Into<String>,
        status: OrderStatus,
    ) -> Result<(), TransitionError> {
        let client_order_id = client_order_id.into();
        if self.statuses.contains_key(&client_order_id) {
            return Err(TransitionError::AlreadyRegistered(client_order_id));
        }
        self.statuses.insert(client_order_id.clone(), status);
        self.history.entry(client_order_id).or_default();
        Ok(())
    }

    pub fn status(&self, client_order_id: &str) -> Option<OrderStatus> {
        self.statuses.get(client_order_id).copied()
    }

    pub fn history(&self, client_order_id: &str) -> &[TransitionEvent] {
        self.history
            .get(client_order_id)
            .map(|events| events.as_slice())
            .unwrap_or(&[])
    }

    pub fn can_transition(from: OrderStatus, to: OrderStatus) -> bool {
        transitions(from).contains(&to)
    }

    /// Move an order to `to`, emitting exactly one audit event.
    pub fn transition(
        &mut self,
        client_order_id: &str,
        to: OrderStatus,
        reason: impl Into<String>,
    ) -> Result<OrderStatus, TransitionError> {
        let current = self
            .statuses
            .get(client_order_id)
            .copied()
            .ok_or_else(|| TransitionError::Unregistered(client_order_id.to_string()))?;
        if !Self::can_transition(current, to) {
            return Err(TransitionError::Illegal {
                client_order_id: client_order_id.to_string(),
                from: current,
                to,
            });
        }
        self.statuses.insert(client_order_id.to_string(), to);
        let event = TransitionEvent {
            client_order_id: client_order_id.to_string(),
            from_status: current,
            to_status: to,
            reason: reason.into(),
            occurred_at: (self.now)(),
        };
        self.history
            .entry(client_order_id.to_string())
            .or_default()
            .push(event.clone());
        if let Some(on_event) = &self.on_event {
            on_event(&event);
        }
        Ok(to)
    }

    /// Timeout/lost-answer path: Pending|Submitted -> Unknown.
    pub fn mark_unknown(
        &mut self,
        client_order_id: &str,
        reason: impl Into<String>,
    ) -> Result<OrderStatus, TransitionError> {
        let current = self
            .statuses
            .get(client_order_id)
            .copied()
            .ok_or_else(|| TransitionError::Unregistered(client_order_id.to_string()))?;
        if !unknown_entry_allowed_from(current) {
            return Err(TransitionError::UnknownEntryBlocked {
                client_order_id: client_order_id.to_string(),
                from: current,
            });
        }
        self.transition(client_order_id, OrderStatus::Unknown, reason)
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const ALL: [OrderStatus; 7] = [
        OrderStatus::Pending,
        OrderStatus::Submitted,
        OrderStatus::PartiallyFilled,
        OrderStatus::Filled,
        OrderStatus::Cancelled,
        OrderStatus::Rejected,
        OrderStatus::Unknown,
    ];

    #[test]
    fn transition_table_covers_every_status() {
        for status in ALL {
            assert!(!transitions(status).is_empty() || status.terminal());
        }
    }

    #[test]
    fn terminal_statuses_accept_nothing() {
        for status in [
            OrderStatus::Filled,
            OrderStatus::Cancelled,
            OrderStatus::Rejected,
        ] {
            assert!(transitions(status).is_empty());
        }
    }

    /// Exhaustive: every legal pair transitions; every illegal pair errors.
    #[test]
    fn all_pairs_classified() {
        for from in ALL {
            let legal: Vec<OrderStatus> = transitions(from).to_vec();
            for to in ALL {
                let mut machine = StateMachine::new();
                machine.register("o-1", from).unwrap();
                let result = machine.transition("o-1", to, "test");
                if legal.contains(&to) {
                    assert_eq!(result.unwrap(), to, "{from:?}->{to:?} must be legal");
                } else if to != from {
                    assert!(result.is_err(), "{from:?}->{to:?} must be illegal");
                }
            }
        }
    }

    #[test]
    fn unknown_entry_paths() {
        for source in [OrderStatus::Pending, OrderStatus::Submitted] {
            let mut machine = StateMachine::new();
            machine.register("o-1", source).unwrap();
            machine.mark_unknown("o-1", "submit timeout").unwrap();
            assert_eq!(machine.status("o-1"), Some(OrderStatus::Unknown));
        }
        for blocked in [
            OrderStatus::PartiallyFilled,
            OrderStatus::Filled,
            OrderStatus::Cancelled,
            OrderStatus::Rejected,
            OrderStatus::Unknown,
        ] {
            let mut machine = StateMachine::new();
            machine.register("o-1", blocked).unwrap();
            assert!(matches!(
                machine.mark_unknown("o-1", "late timeout"),
                Err(TransitionError::UnknownEntryBlocked { .. })
            ));
        }
    }

    #[test]
    fn every_legal_transition_emits_exactly_one_event() {
        let emitted = Arc::new(AtomicUsize::new(0));
        let counter = {
            let emitted = Arc::clone(&emitted);
            move |_: &TransitionEvent| {
                emitted.fetch_add(1, Ordering::SeqCst);
            }
        };
        let mut machine = StateMachine::with_on_event(counter);
        machine.register("o-1", OrderStatus::Pending).unwrap();
        machine
            .transition("o-1", OrderStatus::Submitted, "broker ack")
            .unwrap();
        machine
            .transition("o-1", OrderStatus::PartiallyFilled, "fill 10/20")
            .unwrap();
        machine
            .transition("o-1", OrderStatus::Filled, "fill 20/20")
            .unwrap();
        assert_eq!(emitted.load(Ordering::SeqCst), 3);
        assert_eq!(machine.history("o-1").len(), 3);

        let events = machine.history("o-1");
        assert_eq!(events[0].from_status, OrderStatus::Pending);
        assert_eq!(events[2].to_status, OrderStatus::Filled);
        assert_eq!(events[1].reason, "fill 10/20");
    }

    #[test]
    fn illegal_transition_emits_nothing_and_keeps_status() {
        let emitted = Arc::new(AtomicUsize::new(0));
        let counter = {
            let emitted = Arc::clone(&emitted);
            move |_: &TransitionEvent| {
                emitted.fetch_add(1, Ordering::SeqCst);
            }
        };
        let mut machine = StateMachine::with_on_event(counter);
        machine.register("o-1", OrderStatus::Filled).unwrap();
        assert!(machine
            .transition("o-1", OrderStatus::Submitted, "late ack")
            .is_err());
        assert_eq!(emitted.load(Ordering::SeqCst), 0);
        assert_eq!(machine.status("o-1"), Some(OrderStatus::Filled));
        assert!(machine.history("o-1").is_empty());
    }

    #[test]
    fn unregistered_and_duplicate_registration_rejected() {
        let mut machine = StateMachine::new();
        assert!(matches!(
            machine.transition("o-404", OrderStatus::Submitted, "?"),
            Err(TransitionError::Unregistered(_))
        ));
        machine.register("o-1", OrderStatus::Pending).unwrap();
        assert!(matches!(
            machine.register("o-1", OrderStatus::Pending),
            Err(TransitionError::AlreadyRegistered(_))
        ));
    }

    #[test]
    fn unknown_resolves_via_reconciliation() {
        for resolved in [
            OrderStatus::Filled,
            OrderStatus::Cancelled,
            OrderStatus::Submitted,
        ] {
            let mut machine = StateMachine::new();
            machine.register("o-1", OrderStatus::Pending).unwrap();
            machine.mark_unknown("o-1", "timeout").unwrap();
            machine
                .transition("o-1", resolved, "broker reconciliation")
                .unwrap();
        }
    }
}
