//! Runtime broker assembly — the Rust port of build_broker (wiki:
//! deployment-operations). paper+memory stays the safe default (in-process
//! mock, zero network); the KIS backend composes KisAuth + KisRestClient
//! behind a Broker adapter (kis::broker::KisBroker — venue code lives in the
//! kis layer; assembly only selects); live refuses without the double
//! confirmation (already enforced at settings load — assembly re-checks as
//! defense).

use std::sync::Arc;

use crate::config::{KisEnvironment, PaperBackend, Settings, TradingMode};
use crate::kis::auth::{kis_base_url, KisAuth};
use crate::kis::broker::KisBroker;
use crate::kis::rest::KisRestClient;

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
    #[error("paper backend must use the mock KIS domain — refusing a real-domain broker under a paper label")]
    PaperRequiresMockDomain,
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
        (TradingMode::Paper, PaperBackend::Kis, KisEnvironment::Mock) => {
            let broker = build_kis(settings, KisEnvironment::Mock)?;
            Ok((
                Some(broker),
                AssembledBroker::Kis {
                    environment: KisEnvironment::Mock,
                },
            ))
        }
        // Defense in depth: settings validation already refuses this — if the
        // two ever disagree, refuse here too rather than trade real money.
        (TradingMode::Paper, PaperBackend::Kis, KisEnvironment::Real) => {
            Err(AssemblyError::PaperRequiresMockDomain)
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

    fn settings_with(
        trading_mode: TradingMode,
        confirmed: bool,
        environment: KisEnvironment,
        backend: PaperBackend,
    ) -> Settings {
        Settings {
            trading_mode,
            live_trading_confirmed: confirmed,
            kis_environment: environment,
            paper_backend: backend,
            kis_account_number: "12345678-01".into(),
            ..Settings::default()
        }
    }

    #[test]
    fn live_without_confirmation_refused() {
        let settings = settings_with(
            TradingMode::Live,
            false,
            KisEnvironment::Real,
            PaperBackend::Kis,
        );
        assert!(matches!(
            assemble_broker(&settings),
            Err(AssemblyError::LiveNotConfirmed)
        ));
    }

    #[test]
    fn live_confirmed_assembles_real_kis() {
        let settings = settings_with(
            TradingMode::Live,
            true,
            KisEnvironment::Real,
            PaperBackend::Kis,
        );
        let (broker, assembled) = assemble_broker(&settings).unwrap();
        assert!(broker.is_some());
        assert_eq!(
            assembled,
            AssembledBroker::Kis {
                environment: KisEnvironment::Real
            }
        );
    }

    /// Defense in depth: even if settings validation regresses, assembly
    /// refuses a real-domain broker under a paper label.
    #[test]
    fn paper_kis_real_domain_refused() {
        let settings = settings_with(
            TradingMode::Paper,
            false,
            KisEnvironment::Real,
            PaperBackend::Kis,
        );
        assert!(matches!(
            assemble_broker(&settings),
            Err(AssemblyError::PaperRequiresMockDomain)
        ));
    }

    #[test]
    fn paper_kis_assembles_mock_domain() {
        let settings = settings_with(
            TradingMode::Paper,
            false,
            KisEnvironment::Mock,
            PaperBackend::Kis,
        );
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
