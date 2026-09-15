//! Runtime broker assembly — the Rust port of build_broker (wiki:
//! deployment-operations). paper+memory stays the safe default (in-process
//! mock, zero network); the KIS backend composes KisAuth + KisRestClient
//! behind a Broker adapter; live refuses without the double confirmation
//! (already enforced at settings load — assembly re-checks as defense).

use std::sync::Arc;

use crate::broker::{Broker, BrokerError, ExecutionReport, OrderAck, OrderRequest, Position};
use crate::config::{KisEnvironment, PaperBackend, Settings, TradingMode};
use crate::kis::auth::{kis_base_url, KisAuth};
use crate::kis::rest::KisRestClient;
use crate::types::Quote;

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
            crate::types::OrderType::Limit => ORD_DVSN_LIMIT,
            crate::types::OrderType::Market => ORD_DVSN_MARKET,
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

    async fn quote(&self, symbol: &crate::types::Symbol) -> Result<Quote, BrokerError> {
        self.rest
            .fetch_quote(symbol)
            .await
            .map_err(BrokerError::from)
    }
}

/// What the runtime assembled, for health reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssembledBroker {
    /// In-process paper broker (zero network).
    Memory,
    /// KIS-backed broker (mock domain for paper, real for live).
    Kis { environment: KisEnvironment },
}

#[derive(Debug, thiserror::Error)]
pub enum AssemblyError {
    #[error("live broker assembly requires live_trading_confirmed=true and kis_environment=real")]
    LiveNotConfirmed,
    #[error("backend {0:?} is not available yet")]
    Unavailable(PaperBackend),
}

/// Build the venue broker selected by the loaded settings. `None` means the
/// in-process memory backend (no Broker trait object needed for the demo
/// loop; callers use MockBroker directly).
pub fn assemble_broker(
    settings: &Settings,
) -> Result<(Option<Arc<KisBroker>>, AssembledBroker), AssemblyError> {
    match (
        settings.trading_mode,
        settings.paper_backend,
        settings.kis_environment,
    ) {
        (TradingMode::Paper, PaperBackend::Memory, _) => Ok((None, AssembledBroker::Memory)),
        (TradingMode::Paper, PaperBackend::Kis, environment) => {
            let broker = build_kis(settings, environment)?;
            Ok((Some(broker), AssembledBroker::Kis { environment }))
        }
        (TradingMode::Live, PaperBackend::Kis, KisEnvironment::Real) => {
            if !settings.live_trading_confirmed {
                return Err(AssemblyError::LiveNotConfirmed);
            }
            let broker = build_kis(settings, KisEnvironment::Real)?;
            Ok((
                Some(broker),
                AssembledBroker::Kis {
                    environment: KisEnvironment::Real,
                },
            ))
        }
        (TradingMode::Live, _, _) => Err(AssemblyError::LiveNotConfirmed),
    }
}

fn build_kis(
    settings: &Settings,
    environment: KisEnvironment,
) -> Result<Arc<KisBroker>, AssemblyError> {
    let base_url = kis_base_url(match environment {
        KisEnvironment::Real => "real",
        KisEnvironment::Mock => "mock",
    })
    .to_string();
    let http = reqwest::Client::new();
    let auth = KisAuth::new(
        base_url.clone(),
        settings.kis_app_key.expose().to_string(),
        settings.kis_app_secret.expose().to_string(),
        http.clone(),
    );
    let rest = KisRestClient::new(
        auth,
        &settings.kis_account_number,
        match environment {
            KisEnvironment::Real => "real",
            KisEnvironment::Mock => "mock",
        },
        base_url,
        http,
        &settings.trading_mode.to_string(),
    )
    .map_err(|_| AssemblyError::Unavailable(PaperBackend::Kis))?;
    Ok(Arc::new(KisBroker::new(rest)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    #[test]
    fn paper_memory_is_default_and_network_free() {
        let settings = Settings::default();
        let (broker, assembled) = assemble_broker(&settings).unwrap();
        assert!(broker.is_none());
        assert_eq!(assembled, AssembledBroker::Memory);
    }

    #[test]
    fn live_without_confirmation_refused() {
        let mut settings = Settings::default();
        settings.trading_mode = TradingMode::Live;
        settings.live_trading_confirmed = false;
        settings.kis_environment = KisEnvironment::Real;
        settings.paper_backend = PaperBackend::Kis;
        assert!(matches!(
            assemble_broker(&settings),
            Err(AssemblyError::LiveNotConfirmed)
        ));
    }

    #[test]
    fn live_confirmed_assembles_real_kis() {
        let mut settings = Settings::default();
        settings.trading_mode = TradingMode::Live;
        settings.live_trading_confirmed = true;
        settings.kis_environment = KisEnvironment::Real;
        settings.paper_backend = PaperBackend::Kis;
        settings.kis_account_number = "12345678-01".into();
        let (broker, assembled) = assemble_broker(&settings).unwrap();
        assert!(broker.is_some());
        assert_eq!(
            assembled,
            AssembledBroker::Kis {
                environment: KisEnvironment::Real
            }
        );
    }

    #[test]
    fn paper_kis_assembles_mock_domain() {
        let mut settings = Settings::default();
        settings.paper_backend = PaperBackend::Kis;
        settings.kis_account_number = "12345678-01".into();
        let (broker, assembled) = assemble_broker(&settings).unwrap();
        assert!(broker.is_some());
        assert_eq!(
            assembled,
            AssembledBroker::Kis {
                environment: KisEnvironment::Mock
            }
        );
    }
}
