//! KIS venue broker — the `Broker` adapter over the KIS REST client.
//!
//! Lives in the kis layer so the runtime stays a pure selector: venue
//! specifics (order-context bookkeeping for cancels, KIS dvsn codes) belong
//! here, next to the client they wrap.

use crate::broker::{Broker, BrokerError, ExecutionReport, OrderAck, OrderRequest, Position};
use crate::kis::rest::KisRestClient;
use crate::types::{OrderType, Quote, Symbol};

/// Broker adapter over the KIS REST client.
pub struct KisBroker {
    rest: KisRestClient,
    /// In-session cancel context: broker_order_id -> (orgno, ord_dvsn).
    /// Cross-restart context belongs to the reconciliation layer.
    order_context: tokio::sync::Mutex<std::collections::HashMap<String, (String, String)>>,
}

impl KisBroker {
    pub fn new(rest: KisRestClient) -> Self {
        Self {
            rest,
            order_context: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

const ORD_DVSN_LIMIT: &str = "00";
const ORD_DVSN_MARKET: &str = "01";

#[async_trait::async_trait]
impl Broker for KisBroker {
    async fn submit_order(&self, request: &OrderRequest) -> Result<OrderAck, BrokerError> {
        let (ack, output) = self.rest.submit_cash_order(request).await?;
        let orgno = output["KRX_FWDG_ORD_ORGNO"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let dvsn = match request.order_type {
            OrderType::Limit => ORD_DVSN_LIMIT,
            OrderType::Market => ORD_DVSN_MARKET,
        };
        self.order_context
            .lock()
            .await
            .insert(ack.broker_order_id.clone(), (orgno, dvsn.to_string()));
        Ok(ack)
    }

    async fn cancel_order(&self, broker_order_id: &str) -> Result<(), BrokerError> {
        let context = self
            .order_context
            .lock()
            .await
            .get(broker_order_id)
            .cloned();
        let Some((orgno, dvsn)) = context else {
            return Err(BrokerError::Internal(format!(
                "no in-session context for order {broker_order_id}; \
                 restart recovery must reconcile before cancelling"
            )));
        };
        self.rest
            .cancel_cash_order(broker_order_id, &orgno, &dvsn)
            .await
            .map_err(BrokerError::from)
    }

    async fn execution_report(
        &self,
        broker_order_id: &str,
    ) -> Result<ExecutionReport, BrokerError> {
        let row = self
            .rest
            .fetch_order_row(broker_order_id)
            .await
            .map_err(BrokerError::from)?;
        self.rest
            .execution_report_from_row(broker_order_id, "", &row)
            .map_err(BrokerError::from)
    }

    async fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        self.rest.fetch_positions().await.map_err(BrokerError::from)
    }

    async fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        self.rest
            .fetch_quote(symbol)
            .await
            .map_err(BrokerError::from)
    }
}
