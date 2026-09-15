"""Paper/live environment separation tests."""

import json
from decimal import Decimal

import httpx
import pytest

from lossfunction.broker.base import OrderRequest
from lossfunction.broker.factory import build_broker
from lossfunction.broker.kis.broker import KISBroker, PendingReconciliationError
from lossfunction.broker.mock import MockBroker
from lossfunction.config import Settings, TradingMode
from lossfunction.domain.types import OrderSide, OrderType


def _settings(**overrides: object) -> Settings:
    defaults: dict[str, object] = {
        "kis_account_number": "12345678-01",
        "kis_app_key": "appkey-1",
        "kis_app_secret": "appsecret-1",
    }
    defaults.update(overrides)
    return Settings(_env_file=None, **defaults)  # type: ignore[call-arg]


def _order_request() -> OrderRequest:
    return OrderRequest(
        client_order_id="c-1",
        symbol="005930",
        side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        quantity=10,
    )


def _kis_handler(
    calls: list[httpx.Request],
) -> httpx.MockTransport:
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/oauth2/tokenP":
            return httpx.Response(
                200,
                json={
                    "access_token": "token-1",
                    "access_token_token_expired": "2099-01-01 00:00:00",
                },
            )
        calls.append(request)
        return httpx.Response(
            200,
            json={
                "rt_cd": "0",
                "msg_cd": "00220000",
                "msg1": "주문 전송 완료",
                "output": {"ODNO": "00001234", "ORD_TMD": "093012", "KRX_FWDG_ORD_ORGNO": "01290"},
            },
        )

    return httpx.MockTransport(handler)


# ── AC1: paper mode never reaches the live order endpoint ──────────


def test_paper_default_builds_in_memory_broker() -> None:
    broker = build_broker(_settings())
    assert isinstance(broker, MockBroker)


async def test_paper_kis_backend_targets_mock_domain_only() -> None:
    calls: list[httpx.Request] = []
    settings = _settings(paper_backend="kis", kis_environment="mock")
    broker = build_broker(settings, transport=_kis_handler(calls))
    assert isinstance(broker, KISBroker)
    assert broker.trading_mode == "paper"

    ack = await broker.submit_order(_order_request())
    assert ack.broker_order_id == "00001234"
    # Every HTTP call went to the mock domain, never the real one.
    assert all(c.url.host == "openapivts.koreainvestment.com" for c in calls)
    assert all(
        c.url.path != "/uapi/domestic-stock/v1/trading/order-cash"
        or c.headers["tr_id"].startswith("VTTC")
        for c in calls
    )


# ── AC2: live entry demands explicit confirmation ──────────────────


def test_live_requires_confirmation_at_settings_level() -> None:
    with pytest.raises(Exception, match="live_trading_confirmed"):
        _settings(trading_mode="live")


def test_confirmed_live_builds_real_domain_broker() -> None:
    calls: list[httpx.Request] = []
    settings = _settings(
        trading_mode="live",
        live_trading_confirmed=True,
        kis_environment="real",
    )
    assert settings.trading_mode is TradingMode.LIVE
    broker = build_broker(settings, transport=_kis_handler(calls))
    assert isinstance(broker, KISBroker)
    assert broker.trading_mode == "live"


def test_paper_backend_cannot_sneak_into_live_domain() -> None:
    with pytest.raises(Exception, match="kis_environment=real"):
        _settings(
            trading_mode="live",
            live_trading_confirmed=True,
            kis_environment="mock",
        )


# ── AC3: the mode is recorded on every order trace ─────────────────


async def test_trading_mode_recorded_in_order_traces() -> None:
    for mode, environment in (("paper", "mock"), ("live", "real")):
        calls: list[httpx.Request] = []
        records: list = []
        if mode == "live":
            settings = _settings(
                trading_mode="live",
                live_trading_confirmed=True,
                kis_environment="real",
            )
        else:
            settings = _settings(paper_backend="kis")
        broker = build_broker(settings, transport=_kis_handler(calls), audit=records.append)
        ack = await broker.submit_order(_order_request())
        await broker.cancel_order(ack.broker_order_id)

        traces = [r for r in records]
        assert traces, f"no audit records for {mode}"
        submit_trace = next(r for r in traces if r.path.endswith("order-cash"))
        cancel_trace = next(r for r in traces if r.path.endswith("order-rvsecncl"))
        for trace in (submit_trace, cancel_trace):
            assert trace.order_summary["trading_mode"] == mode
            assert trace.environment == environment


# ── KISBroker cancel behavior ──────────────────────────────────────


async def test_cancel_posts_official_rvsecncl_body() -> None:
    calls: list[httpx.Request] = []
    settings = _settings(paper_backend="kis")
    broker = build_broker(settings, transport=_kis_handler(calls))
    ack = await broker.submit_order(
        OrderRequest(
            client_order_id="c-9",
            symbol="005930",
            side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=5,
            limit_price=Decimal("79000"),
        )
    )
    await broker.cancel_order(ack.broker_order_id)

    cancel = next(c for c in calls if c.url.path.endswith("order-rvsecncl"))
    assert cancel.headers["tr_id"] == "VTTC0013U"
    body = json.loads(cancel.read())
    assert body == {
        "CANO": "12345678",
        "ACNT_PRDT_CD": "01",
        "KRX_FWDG_ORD_ORGNO": "01290",
        "ORGN_ODNO": "00001234",
        "ORD_DVSN": "00",
        "RVSE_CNCL_DVSN_CD": "02",
        "ORD_QTY": "0",
        "ORD_UNPR": "0",
        "QTY_ALL_ORD_YN": "Y",
        "EXCG_ID_DVSN_CD": "KRX",
    }


async def test_cancel_unknown_order_requires_reconciliation() -> None:
    calls: list[httpx.Request] = []
    broker = build_broker(_settings(paper_backend="kis"), transport=_kis_handler(calls))
    with pytest.raises(LookupError, match="reconcile"):
        await broker.cancel_order("UNKNOWN-ODNO")


async def test_execution_report_awaits_reconciliation_layer() -> None:
    calls: list[httpx.Request] = []
    broker = build_broker(_settings(paper_backend="kis"), transport=_kis_handler(calls))
    with pytest.raises(PendingReconciliationError):
        await broker.get_execution_report("00001234")
