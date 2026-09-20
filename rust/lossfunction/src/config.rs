//! Settings loaded from the environment (and optional `.env` file).
//!
//! Rules ported from the reference implementation:
//! - Default mode is paper.
//! - `live` requires the explicit double confirmation AND the real KIS domain;
//!   violations refuse to load — unconfirmed live trading never falls back.

use std::env;
use std::fmt;

use crate::types::Symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingMode {
    Paper,
    Live,
}

impl fmt::Display for TradingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TradingMode::Paper => "paper",
            TradingMode::Live => "live",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KisEnvironment {
    Mock,
    Real,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperBackend {
    Memory,
    Kis,
}

/// Credentials are kept opaque so they never leak through Debug logs.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Risk limits (KRW, enforced wherever a RiskManager is assembled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskSettings {
    pub max_order_notional: i64,
    pub max_position_quantity: i64,
    pub gross_exposure: i64,
    pub daily_loss_limit: i64,
    pub stale_quote_seconds: u64,
}

impl Default for RiskSettings {
    fn default() -> Self {
        Self {
            max_order_notional: 5_000_000,
            max_position_quantity: 50,
            gross_exposure: 20_000_000,
            daily_loss_limit: 500_000,
            stale_quote_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub trading_mode: TradingMode,
    pub live_trading_confirmed: bool,
    pub kis_environment: KisEnvironment,
    pub paper_backend: PaperBackend,
    pub kis_app_key: Secret,
    pub kis_app_secret: Secret,
    pub kis_account_number: String,
    pub database_path: String,
    pub risk: RiskSettings,
    /// Bounded past-closes window handed to indicator strategies.
    pub history_window_bars: usize,
    /// Symbol universe the runtime trades/watches. WATCHLIST env overrides
    /// (comma-separated 6-digit codes); invalid codes refuse to boot.
    pub watchlist: Vec<Symbol>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trading_mode: TradingMode::Paper,
            live_trading_confirmed: false,
            kis_environment: KisEnvironment::Mock,
            paper_backend: PaperBackend::Memory,
            kis_app_key: Secret::default(),
            kis_app_secret: Secret::default(),
            kis_account_number: String::new(),
            database_path: "data/lossfunction.db".to_string(),
            risk: RiskSettings::default(),
            history_window_bars: 120,
            watchlist: default_watchlist(),
        }
    }
}

fn default_watchlist() -> Vec<Symbol> {
    ["005930", "035420", "069500"]
        .iter()
        .map(|code| Symbol::parse(*code).expect("default watchlist codes are valid"))
        .collect()
}

fn parse_watchlist(raw: &str) -> Result<Vec<Symbol>, ConfigError> {
    let codes: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .collect();
    if codes.is_empty() {
        return Err(ConfigError::Invalid {
            field: "WATCHLIST",
            message: "empty watchlist".to_string(),
        });
    }
    codes
        .into_iter()
        .map(|code| {
            Symbol::parse(code).map_err(|error| ConfigError::Invalid {
                field: "WATCHLIST",
                message: format!("{code:?}: {error}"),
            })
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown TRADING_MODE {0:?} (expected paper|live)")]
    UnknownTradingMode(String),
    #[error("trading_mode=live requires live_trading_confirmed=true; unconfirmed live trading is refused at settings load")]
    LiveNotConfirmed,
    #[error("trading_mode=live requires kis_environment=real")]
    LiveRequiresRealEnvironment,
    #[error("invalid {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
}

impl Settings {
    /// Load from environment variables plus an optional `.env` file,
    /// validating the paper/live separation rules before returning.
    pub fn load() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        let mut settings = Settings::default();

        if let Some(mode) = read_optional("TRADING_MODE")? {
            settings.trading_mode = match mode.as_str() {
                "paper" => TradingMode::Paper,
                "live" => TradingMode::Live,
                other => return Err(ConfigError::UnknownTradingMode(other.to_string())),
            };
        }
        settings.live_trading_confirmed = read_flag("LIVE_TRADING_CONFIRMED")?;
        if let Some(env_) = read_optional("KIS_ENVIRONMENT")? {
            settings.kis_environment = match env_.as_str() {
                "mock" => KisEnvironment::Mock,
                "real" => KisEnvironment::Real,
                other => {
                    return Err(ConfigError::Invalid {
                        field: "KIS_ENVIRONMENT",
                        message: format!("unknown value {other:?} (expected mock|real)"),
                    })
                }
            };
        }
        if let Some(backend) = read_optional("PAPER_BACKEND")? {
            settings.paper_backend = match backend.as_str() {
                "memory" => PaperBackend::Memory,
                "kis" => PaperBackend::Kis,
                other => {
                    return Err(ConfigError::Invalid {
                        field: "PAPER_BACKEND",
                        message: format!("unknown value {other:?} (expected memory|kis)"),
                    })
                }
            };
        }

        settings.kis_app_key = Secret(read_optional("KIS_APP_KEY")?.unwrap_or_default());
        settings.kis_app_secret = Secret(read_optional("KIS_APP_SECRET")?.unwrap_or_default());
        settings.kis_account_number = read_optional("KIS_ACCOUNT_NUMBER")?.unwrap_or_default();
        if let Some(raw) = read_optional("WATCHLIST")? {
            settings.watchlist = parse_watchlist(&raw)?;
        }
        if let Some(path) = read_optional("DATABASE_PATH")? {
            settings.database_path = path;
        }
        settings.risk.max_order_notional =
            read_number("RISK_MAX_ORDER_NOTIONAL", settings.risk.max_order_notional)?;
        settings.risk.max_position_quantity = read_number(
            "RISK_MAX_POSITION_QUANTITY",
            settings.risk.max_position_quantity,
        )?;
        settings.risk.gross_exposure =
            read_number("RISK_GROSS_EXPOSURE", settings.risk.gross_exposure)?;
        settings.risk.daily_loss_limit =
            read_number("RISK_DAILY_LOSS_LIMIT", settings.risk.daily_loss_limit)?;
        settings.risk.stale_quote_seconds = read_number(
            "RISK_STALE_QUOTE_SECONDS",
            settings.risk.stale_quote_seconds,
        )?;

        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.trading_mode == TradingMode::Live {
            if !self.live_trading_confirmed {
                return Err(ConfigError::LiveNotConfirmed);
            }
            if self.kis_environment != KisEnvironment::Real {
                return Err(ConfigError::LiveRequiresRealEnvironment);
            }
        }
        Ok(())
    }
}

fn read_optional(key: &str) -> Result<Option<String>, ConfigError> {
    match env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(raw)) => Err(ConfigError::Invalid {
            field: "environment",
            message: format!("{key} is not valid unicode: {raw:?}"),
        }),
    }
}

fn read_flag(key: &str) -> Result<bool, ConfigError> {
    read_optional(key)?
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Ok(true),
            "0" | "false" | "no" | "" => Ok(false),
            other => Err(ConfigError::Invalid {
                field: "flag",
                message: format!("{key}={other:?} is not a boolean"),
            }),
        })
        .transpose()
        .map(|flag| flag.unwrap_or(false))
}

fn read_number<T: std::str::FromStr>(key: &str, default: T) -> Result<T, ConfigError> {
    read_optional(key)?
        .map(|value| {
            value.parse::<T>().map_err(|_| ConfigError::Invalid {
                field: "number",
                message: format!("{key}={value:?} is not a valid number"),
            })
        })
        .transpose()
        .map(|parsed| parsed.unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Environment is process-global and tests run in parallel threads —
    /// serialize every test that touches env vars through this lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_vars() {
        for key in [
            "TRADING_MODE",
            "LIVE_TRADING_CONFIRMED",
            "KIS_ENVIRONMENT",
            "PAPER_BACKEND",
            "DATABASE_PATH",
            "WATCHLIST",
        ] {
            env::remove_var(key);
        }
    }

    #[test]
    fn watchlist_env_overrides_and_rejects_bad_codes() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_vars();

        // Unset -> historical default trio (back-compat).
        let settings = Settings::load().unwrap();
        assert_eq!(settings.watchlist, default_watchlist());

        // Comma-separated override parses to symbols.
        env::set_var("WATCHLIST", " 005930, 000660 ");
        let settings = Settings::load().unwrap();
        assert_eq!(settings.watchlist.len(), 2);
        assert_eq!(settings.watchlist[0].as_str(), "005930");
        assert_eq!(settings.watchlist[1].as_str(), "000660");

        // Invalid code refuses to boot.
        env::set_var("WATCHLIST", "005930,GOOSE");
        assert!(matches!(
            Settings::load().unwrap_err(),
            ConfigError::Invalid {
                field: "WATCHLIST",
                ..
            }
        ));

        // Empty list refuses too.
        env::set_var("WATCHLIST", " , ");
        assert!(matches!(
            Settings::load().unwrap_err(),
            ConfigError::Invalid {
                field: "WATCHLIST",
                ..
            }
        ));
        env::remove_var("WATCHLIST");
    }

    #[test]
    fn settings_rules_in_sequence() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_vars();

        // Default is paper with the in-memory backend.
        let settings = Settings::load().unwrap();
        assert_eq!(settings.trading_mode, TradingMode::Paper);
        assert_eq!(settings.paper_backend, PaperBackend::Memory);
        assert_eq!(settings.database_path, "data/lossfunction.db");

        // Env overrides load.
        env::set_var("DATABASE_PATH", "data/custom.db");
        let settings = Settings::load().unwrap();
        assert_eq!(settings.database_path, "data/custom.db");

        // Unknown mode is rejected.
        env::set_var("TRADING_MODE", "moon");
        assert!(matches!(
            Settings::load().unwrap_err(),
            ConfigError::UnknownTradingMode(_)
        ));

        // Live without confirmation is refused.
        env::set_var("TRADING_MODE", "live");
        env::remove_var("LIVE_TRADING_CONFIRMED");
        assert!(matches!(
            Settings::load().unwrap_err(),
            ConfigError::LiveNotConfirmed
        ));

        // Live confirmed but mock environment is refused.
        env::set_var("LIVE_TRADING_CONFIRMED", "true");
        env::set_var("KIS_ENVIRONMENT", "mock");
        assert!(matches!(
            Settings::load().unwrap_err(),
            ConfigError::LiveRequiresRealEnvironment
        ));

        // Fully confirmed live loads.
        env::set_var("KIS_ENVIRONMENT", "real");
        let settings = Settings::load().unwrap();
        assert_eq!(settings.trading_mode, TradingMode::Live);

        // Secrets never render.
        env::set_var("KIS_APP_KEY", "very-secret-key");
        let debug = format!("{:?}", Settings::load().unwrap());
        assert!(!debug.contains("very-secret-key"), "secret leaked: {debug}");

        clear_vars();
    }
}
