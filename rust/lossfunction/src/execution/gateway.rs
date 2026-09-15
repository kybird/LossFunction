//! Order submission gateway and reconciler.
//!
//! The gateway is the only path through which orders reach a broker:
//! - duplicate `client_order_id`s are refused before any broker call —
//!   including orders sitting in Unknown after a timeout. Retrying a
//!   timed-out order means reconciling it, never resubmitting it;
//! - a definitive venue rejection moves the order to Rejected; any other
//!   failure (timeout, transport, garbled response) moves it to Unknown —
//!   the broker may or may not have accepted it.
//!
//! The reconciler resolves Unknown orders against broker-side truth; orders
//! the broker has no record of never reached it (-> Rejected).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::broker::{Broker, BrokerError, ExecutionReport, OrderAck, OrderRequest};
use crate::domain::order::{Order, OrderStatus};
use crate::execution::state_machine::{StateMachine, TransitionError};

#[derive(Debug, thiserror::Error)]
pub enum SubmitError {
    #[error("duplicate order submission blocked: client_order_id {0:?} already used")]
    Duplicate(String),
    #[error("no definitive broker answer; order now sits in UNKNOWN: {0}")]
    Timeout(String),
    #[error("broker error: {0}")]
    Broker(#[from] BrokerError),
    #[error("state machine: {0}")]
    MachineError(#[from] TransitionError),
}

#[derive(Debug, thiserror::Error)]
pub enum ReconcileError {
    #[error("state machine: {0}")]
    Machine(#[from] TransitionError),
    #[error("broker lookup failed: {0}")]
    Broker(String),
}

/// The only submission path; duplicate protection is mechanical.
pub struct OrderGateway {
    broker: Arc<dyn Broker>,
    /// client_order_id -> broker_order_id for submitted orders.
    broker_ids: Mutex<HashMap<String, String>>,
}

impl OrderGateway {
    pub fn new(broker: Arc<dyn Broker>) -> Self {
        Self {
            broker,
            broker_ids: Mutex::new(HashMap::new()),
        }
    }

    pub fn broker_order_id(&self, client_order_id: &str) -> Option<String> {
        self.broker_ids
            .lock()
            .expect("gateway lock")
            .get(client_order_id)
            .cloned()
    }

    pub async fn submit(
        &self,
        machine: &mut StateMachine,
        order: &Order,
    ) -> Result<OrderAck, SubmitError> {
        let client_order_id = order.client_order_id().to_string();
        {
            let ids = self.broker_ids.lock().expect("gateway lock");
            if ids.contains_key(&client_order_id) || machine.status(&client_order_id).is_some() {
                return Err(SubmitError::Duplicate(client_order_id));
            }
        }

        machine
            .register(client_order_id.clone(), OrderStatus::Pending)
            .map_err(SubmitError::MachineError)?;
        let request = OrderRequest {
            client_order_id: client_order_id.clone(),
            symbol: order.symbol().clone(),
            side: order.side(),
            order_type: order.order_type(),
            quantity: order.quantity(),
            limit_price: order.limit_price(),
        };

        let ack = match self.broker.submit_order(&request).await {
            Ok(ack) => ack,
            Err(error) => {
                if matches!(error, BrokerError::Rejected(_)) {
                    machine
                        .transition(
                            &client_order_id,
                            OrderStatus::Rejected,
                            format!("broker rejected: {error}"),
                        )
                        .map_err(SubmitError::MachineError)?;
                    return Err(error.into());
                }
                machine
                    .mark_unknown(&client_order_id, format!("no definitive answer: {error}"))
                    .map_err(SubmitError::MachineError)?;
                return Err(SubmitError::Timeout(error.to_string()));
            }
        };

        machine
            .transition(
                &client_order_id,
                OrderStatus::Submitted,
                format!("broker ack {}", ack.broker_order_id),
            )
            .map_err(SubmitError::MachineError)?;
        self.broker_ids
            .lock()
            .expect("gateway lock")
            .insert(client_order_id, ack.broker_order_id.clone());
        Ok(ack)
    }
}

/// Resolves Unknown/open orders against broker-side truth.
pub struct Reconciler {
    broker: Arc<dyn Broker>,
}

impl Reconciler {
    pub fn new(broker: Arc<dyn Broker>) -> Self {
        Self { broker }
    }

    /// Resolve one order from Unknown using the broker's report.
    pub async fn reconcile(
        &self,
        machine: &mut StateMachine,
        client_order_id: &str,
        broker_order_id: &str,
    ) -> Result<OrderStatus, ReconcileError> {
        let status = machine.status(client_order_id).ok_or_else(|| {
            ReconcileError::Machine(TransitionError::Unregistered(client_order_id.to_string()))
        })?;
        if status != OrderStatus::Unknown {
            return Ok(status); // already resolved; nothing to do
        }

        let report = match self.broker.execution_report(broker_order_id).await {
            Ok(report) => report,
            // Broker has no record: the order never reached it.
            Err(BrokerError::UnknownOrder(_)) => {
                return machine
                    .transition(
                        client_order_id,
                        OrderStatus::Rejected,
                        "not found at broker during reconciliation",
                    )
                    .map_err(ReconcileError::Machine);
            }
            Err(error) => {
                return Err(ReconcileError::Broker(error.to_string()));
            }
        };

        let target = target_status(&report);
        machine
            .transition(
                client_order_id,
                target,
                format!(
                    "reconciled: filled {}/{} open={}",
                    report.filled_quantity, report.order_quantity, report.open
                ),
            )
            .map_err(ReconcileError::Machine)
    }

    /// Restart recovery: reconcile id->broker-id pairs left open. Orders are
    /// registered directly at Unknown (their submission answer was
    /// outstanding when the process died) and then resolved.
    pub async fn reconcile_pending(
        &self,
        machine: &mut StateMachine,
        pending: &HashMap<String, String>,
    ) -> Result<HashMap<String, OrderStatus>, ReconcileError> {
        let mut results = HashMap::new();
        for (client_order_id, broker_order_id) in pending {
            if machine.status(client_order_id).is_none() {
                machine
                    .register(client_order_id.clone(), OrderStatus::Unknown)
                    .map_err(ReconcileError::Machine)?;
            }
            let status = self
                .reconcile(machine, client_order_id, broker_order_id)
                .await?;
            results.insert(client_order_id.clone(), status);
        }
        Ok(results)
    }
}

/// Broker report -> order status (shared with execution sync).
pub fn target_status(report: &ExecutionReport) -> OrderStatus {
    if !report.open && report.filled_quantity >= report.order_quantity {
        OrderStatus::Filled
    } else if report.open {
        if report.filled_quantity > 0 {
            OrderStatus::PartiallyFilled
        } else {
            OrderStatus::Submitted
        }
    } else {
        OrderStatus::Cancelled // closed with unfilled remainder
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::{Broker, OrderRequest, Position};
    use crate::types::{OrderSide, OrderType, Quote, Symbol};
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::collections::HashMap as Map;

    /// Broker with configurable submit outcome and preset reports.
    struct ScriptedBroker {
        submit_error: Option<BrokerError>,
        reports: Map<String, ExecutionReport>,
        submitted: Mutex<Vec<OrderRequest>>,
    }

    impl ScriptedBroker {
        fn with_error(error: BrokerError) -> Arc<Self> {
            Arc::new(Self {
                submit_error: Some(error),
                reports: Map::new(),
                submitted: Mutex::new(Vec::new()),
            })
        }

        fn with_reports(reports: Map<String, ExecutionReport>) -> Arc<Self> {
            Arc::new(Self {
                submit_error: None,
                reports,
                submitted: Mutex::new(Vec::new()),
            })
        }

        fn submit_count(&self) -> usize {
            self.submitted.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl Broker for ScriptedBroker {
        async fn submit_order(&self, request: &OrderRequest) -> Result<OrderAck, BrokerError> {
            self.submitted.lock().unwrap().push(request.clone());
            if let Some(error) = &self.submit_error {
                return Err(clone_error(error));
            }
            Ok(OrderAck {
                client_order_id: request.client_order_id.clone(),
                broker_order_id: "B-1".to_string(),
            })
        }
        async fn cancel_order(&self, _: &str) -> Result<(), BrokerError> {
            Err(BrokerError::Internal("not needed".into()))
        }
        async fn execution_report(&self, id: &str) -> Result<ExecutionReport, BrokerError> {
            self.reports
                .get(id)
                .cloned()
                .ok_or(BrokerError::UnknownOrder(id.to_string()))
        }
        async fn positions(&self) -> Result<Vec<Position>, BrokerError> {
            Ok(Vec::new())
        }
        async fn quote(&self, _: &Symbol) -> Result<Quote, BrokerError> {
            Err(BrokerError::Internal("not needed".into()))
        }
    }

    fn clone_error(error: &BrokerError) -> BrokerError {
        match error {
            BrokerError::Rejected(message) => BrokerError::Rejected(message.clone()),
            BrokerError::UnknownSymbol(s) => BrokerError::UnknownSymbol(s.clone()),
            _ => BrokerError::Internal(error.to_string()),
        }
    }

    fn order(id: &str) -> Order {
        Order::new(
            id,
            Symbol::parse("005930").unwrap(),
            OrderSide::Buy,
            OrderType::Limit,
            10,
            Some(Decimal::from(79_000)),
        )
        .unwrap()
    }

    fn report(order_quantity: i64, filled: i64, open: bool) -> ExecutionReport {
        ExecutionReport {
            broker_order_id: "B-1".into(),
            client_order_id: "c-1".into(),
            symbol: Symbol::parse("005930").unwrap(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            order_quantity,
            filled_quantity: filled,
            average_fill_price: if filled > 0 {
                Some(Decimal::from(79_000))
            } else {
                None
            },
            open,
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn duplicate_submission_blocked_before_broker_call() {
        let broker = ScriptedBroker::with_reports(Map::new());
        let gateway = OrderGateway::new(broker.clone());
        let mut machine = StateMachine::new();

        gateway.submit(&mut machine, &order("c-1")).await.unwrap();
        assert!(matches!(
            gateway.submit(&mut machine, &order("c-1")).await,
            Err(SubmitError::Duplicate(_))
        ));
        assert_eq!(broker.submit_count(), 1);
    }

    #[tokio::test]
    async fn timeout_moves_to_unknown_and_blocks_retry() {
        let broker = ScriptedBroker::with_error(BrokerError::Internal("network glitch".into()));
        let gateway = OrderGateway::new(broker.clone());
        let mut machine = StateMachine::new();

        assert!(matches!(
            gateway.submit(&mut machine, &order("c-1")).await,
            Err(SubmitError::Timeout(_))
        ));
        assert_eq!(machine.status("c-1"), Some(OrderStatus::Unknown));

        // Retrying the same logical order is refused — reconciliation is the
        // only way out.
        assert!(matches!(
            gateway.submit(&mut machine, &order("c-1")).await,
            Err(SubmitError::Duplicate(_))
        ));
        assert_eq!(broker.submit_count(), 1);
    }

    #[tokio::test]
    async fn definitive_rejection_moves_to_rejected() {
        let broker =
            ScriptedBroker::with_error(BrokerError::Rejected("rt_cd=1 msg_cd=40150".into()));
        let gateway = OrderGateway::new(broker);
        let mut machine = StateMachine::new();

        assert!(matches!(
            gateway.submit(&mut machine, &order("c-1")).await,
            Err(SubmitError::Broker(BrokerError::Rejected(_)))
        ));
        assert_eq!(machine.status("c-1"), Some(OrderStatus::Rejected));
    }

    #[tokio::test]
    async fn reconcile_resolves_all_four_report_shapes() {
        for (report, expected) in [
            (report(10, 10, false), OrderStatus::Filled),
            (report(10, 4, true), OrderStatus::PartiallyFilled),
            (report(10, 0, true), OrderStatus::Submitted),
            (report(10, 4, false), OrderStatus::Cancelled),
        ] {
            let mut reports = Map::new();
            reports.insert("B-1".to_string(), report);
            let reconciler = Reconciler::new(ScriptedBroker::with_reports(reports));
            let mut machine = StateMachine::new();
            machine.register("c-1", OrderStatus::Pending).unwrap();
            machine.mark_unknown("c-1", "timeout").unwrap();
            let status = reconciler
                .reconcile(&mut machine, "c-1", "B-1")
                .await
                .unwrap();
            assert_eq!(status, expected);
        }
    }

    #[tokio::test]
    async fn order_missing_at_broker_is_rejected() {
        let reconciler = Reconciler::new(ScriptedBroker::with_reports(Map::new()));
        let mut machine = StateMachine::new();
        machine.register("c-1", OrderStatus::Pending).unwrap();
        machine.mark_unknown("c-1", "timeout").unwrap();
        let status = reconciler
            .reconcile(&mut machine, "c-1", "B-1")
            .await
            .unwrap();
        assert_eq!(status, OrderStatus::Rejected);
    }

    #[tokio::test]
    async fn reconcile_is_idempotent() {
        let mut reports = Map::new();
        reports.insert("B-1".to_string(), report(10, 10, false));
        let reconciler = Reconciler::new(ScriptedBroker::with_reports(reports));
        let mut machine = StateMachine::new();
        machine.register("c-1", OrderStatus::Pending).unwrap();
        machine.mark_unknown("c-1", "timeout").unwrap();

        let first = reconciler
            .reconcile(&mut machine, "c-1", "B-1")
            .await
            .unwrap();
        let again = reconciler
            .reconcile(&mut machine, "c-1", "B-1")
            .await
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(machine.history("c-1").len(), 2); // unknown + resolve, no dupes
    }

    #[tokio::test]
    async fn restart_reconciles_pending_orders() {
        let mut reports = Map::new();
        reports.insert("B-1".to_string(), report(10, 10, false));
        reports.insert("B-2".to_string(), report(7, 0, true));
        let reconciler = Reconciler::new(ScriptedBroker::with_reports(reports));
        let mut machine = StateMachine::new(); // fresh process

        let mut pending = Map::new();
        pending.insert("c-1".to_string(), "B-1".to_string());
        pending.insert("c-2".to_string(), "B-2".to_string());
        let results = reconciler
            .reconcile_pending(&mut machine, &pending)
            .await
            .unwrap();
        assert_eq!(
            results,
            Map::from([
                ("c-1".to_string(), OrderStatus::Filled),
                ("c-2".to_string(), OrderStatus::Submitted),
            ])
        );
    }
}
