"""Broker assembly — where paper/live separation actually happens.

Strategy/risk/order code receives a `Broker` and never learns which venue
implementation it got (architecture doc §5): paper defaults to the in-memory
broker, live requires the double confirmation already enforced at settings
load and is re-verified here as defense in depth.
"""

import httpx

from lossfunction.broker.base import Broker
from lossfunction.broker.kis.rest import AuditCallback
from lossfunction.broker.mock import MockBroker
from lossfunction.config import Settings, TradingMode


class LiveTradingNotConfirmedError(Exception):
    """Live assembly attempted without the explicit confirmation flag."""


def build_broker(
    settings: Settings,
    *,
    transport: httpx.AsyncBaseTransport | None = None,
    audit: AuditCallback | None = None,
) -> Broker:
    """Build the venue broker selected by the loaded settings."""
    if settings.trading_mode is TradingMode.LIVE:
        if not settings.live_trading_confirmed or settings.kis_environment != "real":
            msg = (
                "live broker assembly requires live_trading_confirmed=true and "
                "kis_environment=real; refusing to assemble"
            )
            raise LiveTradingNotConfirmedError(msg)
        return _build_kis(settings, environment="real", transport=transport, audit=audit)

    if settings.paper_backend == "kis":
        # Paper trading through the KIS 모의투자 domain (needs mock credentials).
        return _build_kis(
            settings, environment=settings.kis_environment, transport=transport, audit=audit
        )

    # Safe default: pure in-memory paper broker — no network at all.
    return MockBroker()


def _build_kis(
    settings: Settings,
    *,
    environment: str,
    transport: httpx.AsyncBaseTransport | None,
    audit: AuditCallback | None,
) -> Broker:
    from lossfunction.broker.kis.broker import build_kis_broker

    return build_kis_broker(
        app_key=settings.kis_app_key.get_secret_value(),
        app_secret=settings.kis_app_secret.get_secret_value(),
        account_number=settings.kis_account_number,
        environment=environment,
        trading_mode=settings.trading_mode.value,
        transport=transport,
        audit=audit,
    )
