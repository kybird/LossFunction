"""Typed application settings with paper/live separation.

Rules (docs/architecture.md §5):
- Default mode is paper.
- Live mode requires an explicit, separate confirmation flag.
- Secrets never have defaults and are masked in repr.
"""

from enum import StrEnum
from typing import Literal

from pydantic import SecretStr, model_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class TradingMode(StrEnum):
    PAPER = "paper"
    LIVE = "live"


class Settings(BaseSettings):
    """Application settings loaded from environment variables and `.env`."""

    model_config = SettingsConfigDict(
        env_file=".env",
        env_file_encoding="utf-8",
        extra="ignore",
        frozen=True,
    )

    trading_mode: TradingMode = TradingMode.PAPER
    live_trading_confirmed: bool = False

    # KIS API domain: "mock" (모의투자, safe default) or "real".
    kis_environment: Literal["real", "mock"] = "mock"

    # Paper-mode broker: "memory" (in-process, no network) or "kis"
    # (KIS 모의투자 domain, requires mock credentials).
    paper_backend: Literal["memory", "kis"] = "memory"

    # Alerting: webhook is opt-in; console logging is always active.
    alert_webhook_url: str = ""
    alert_min_interval_seconds: float = 30.0

    # Risk limits (KRW; enforce everywhere a RiskManager is built).
    risk_max_order_notional: int = 5_000_000
    risk_max_position_quantity: int = 50
    risk_gross_exposure: int = 20_000_000
    risk_daily_loss_limit: int = 500_000
    risk_stale_quote_seconds: int = 30

    # KIS credentials — environment only, never committed.
    kis_app_key: SecretStr = SecretStr("")
    kis_app_secret: SecretStr = SecretStr("")
    kis_account_number: str = ""

    database_path: str = "data/lossfunction.db"

    @model_validator(mode="after")
    def _validate_mode_consistency(self) -> "Settings":
        if self.trading_mode is TradingMode.LIVE and not self.live_trading_confirmed:
            msg = (
                "trading_mode=live requires live_trading_confirmed=true; "
                "unconfirmed live trading is refused at settings load"
            )
            raise ValueError(msg)
        if self.trading_mode is TradingMode.LIVE and self.kis_environment != "real":
            msg = "trading_mode=live requires kis_environment=real"
            raise ValueError(msg)
        return self


def load_settings() -> Settings:
    """Load settings from the environment (and `.env` if present)."""
    return Settings()
