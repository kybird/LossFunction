"""KIS REST client tests against a mock HTTP transport."""

import json
from decimal import Decimal

import httpx
import pytest

from lossfunction.broker.base import OrderRequest
from lossfunction.broker.kis import (
    APIErrorKind,
    AuditRecord,
    KISAPIError,
    KISAuthClient,
    KISRestClient,
)
from lossfunction.domain.types import OrderSide, OrderType


def _order_request(**overrides: object) -> OrderRequest:
    defaults: dict[str, object] = {
        "client_order_id": "c-1",
        "symbol": "005930",
        "side": OrderSide.BUY,
        "order_type": OrderType.MARKET,
        "quantity": 10,
    }
    defaults.update(overrides)
    return OrderRequest(**defaults)  # type: ignore[arg-type]


class KISHarness:
    """Wires an auth client and REST client onto one mock transport."""

    def __init__(self, handler) -> None:
        state = {"issued": 0}

        def wrapped(request: httpx.Request) -> httpx.Response:
            if request.url.path == "/oauth2/tokenP":
                state["issued"] += 1
                return httpx.Response(
                    200,
                    json={
                        "access_token": f"token-{state['issued']}",
                        "access_token_token_expired": "2099-01-01 00:00:00",
                    },
                )
            return handler(request)

        self.transport = httpx.MockTransport(wrapped)
        self.audit_records: list[AuditRecord] = []
        self.auth = KISAuthClient(
            "appkey-1",
            "appsecret-1",
            "https://kis.example",
            transport=self.transport,
            sleep=_no_sleep,
        )
        self.rest = KISRestClient(
            auth=self.auth,
            account_number="12345678-01",
            environment="mock",
            base_url="https://kis.example",
            transport=self.transport,
            sleep=_no_sleep,
            audit=self.audit_records.append,
        )

    async def close(self) -> None:
        await self.rest.aclose()
        await self.auth.aclose()


async def _no_sleep(seconds: float) -> None:
    return None


def _order_ok() -> httpx.Response:
    return httpx.Response(
        200,
        json={
            "rt_cd": "0",
            "msg_cd": "00220000",
            "msg1": "주문 전송 완료",
            "output": {"ODNO": "00001234", "ORD_TMD": "093012"},
        },
    )


async def test_submit_order_builds_official_request_shape() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return _order_ok()

    harness = KISHarness(handler)
    ack = await harness.rest.submit_cash_order(_order_request())

    assert ack == ("c-1", "00001234") or (
        ack.client_order_id == "c-1" and ack.broker_order_id == "00001234"
    )
    request = calls[0]
    assert request.method == "POST"
    assert request.url.path == "/uapi/domestic-stock/v1/trading/order-cash"
    # Official spec: mock env buy uses VTTC0012U, uppercase string body.
    assert request.headers["tr_id"] == "VTTC0012U"
    assert request.headers["custtype"] == "P"
    assert request.headers["authorization"] == "Bearer token-1"
    body = json.loads(request.read())
    assert body == {
        "CANO": "12345678",
        "ACNT_PRDT_CD": "01",
        "PDNO": "005930",
        "ORD_DVSN": "01",
        "ORD_QTY": "10",
        "ORD_UNPR": "0",
        "EXCG_ID_DVSN_CD": "KRX",
        "SLL_TYPE": "",
        "CNDT_PRIC": "",
    }
    await harness.close()


async def test_limit_order_sends_price_and_real_env_tr_id() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return _order_ok()

    harness = KISHarness(handler)
    harness.rest._environment = "real"  # switch domain variant
    await harness.rest.submit_cash_order(
        _order_request(
            order_type=OrderType.LIMIT, limit_price=Decimal("79000"), side=OrderSide.SELL
        )
    )
    body = json.loads(calls[0].read())
    assert body["ORD_DVSN"] == "00"
    assert body["ORD_UNPR"] == "79000"
    assert calls[0].headers["tr_id"] == "TTTC0011U"  # real sell
    await harness.close()


async def test_business_rejection_is_classified() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200,
            json={"rt_cd": "1", "msg_cd": "40150", "msg1": "주문수량이 정상 범위를 벗어났습니다"},
        )

    harness = KISHarness(handler)
    with pytest.raises(KISAPIError) as excinfo:
        await harness.rest.submit_cash_order(_order_request())
    assert excinfo.value.kind is APIErrorKind.API_REJECT
    assert excinfo.value.rt_cd == "1"
    assert excinfo.value.msg_cd == "40150"
    await harness.close()


async def test_positions_follow_pagination() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.params.get("CTX_AREA_NK100") == "NK1":
            return httpx.Response(
                200,
                json={
                    "rt_cd": "0",
                    "ctx_area_fk100": "",
                    "ctx_area_nk100": "",
                    "output1": [
                        {"pdno": "035420", "hldg_qty": "5", "pchs_avg_pric": "41000.0000"},
                    ],
                },
            )
        return httpx.Response(
            200,
            headers={"tr_cont": "M"},
            json={
                "rt_cd": "0",
                "ctx_area_fk100": "FK",
                "ctx_area_nk100": "NK1",
                "output1": [
                    {"pdno": "005930", "hldg_qty": "10", "pchs_avg_pric": "80000.0000"},
                    {"pdno": "069500", "hldg_qty": "0", "pchs_avg_pric": "0"},
                ],
            },
        )

    harness = KISHarness(handler)
    positions = await harness.rest.fetch_positions()
    assert [(p.symbol, p.quantity, p.average_price) for p in positions] == [
        ("005930", 10, Decimal("80000.0000")),
        ("035420", 5, Decimal("41000.0000")),
    ]
    await harness.close()


async def test_fetch_quote_parses_price() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/uapi/domestic-stock/v1/quotations/inquire-price"
        assert request.url.params["FID_COND_MRKT_DIV_CODE"] == "J"
        assert request.url.params["FID_INPUT_ISCD"] == "005930"
        assert request.headers["tr_id"] == "FHKST01010100"
        return httpx.Response(
            200,
            json={"rt_cd": "0", "output": {"stck_shrn_iscd": "005930", "stck_prpr": "80500"}},
        )

    harness = KISHarness(handler)
    quote = await harness.rest.fetch_quote("005930")
    assert quote.last_price == Decimal("80500")
    assert quote.timestamp.tzinfo is not None
    await harness.close()


async def test_server_error_retried_then_classified() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        if len(calls) == 1:
            return httpx.Response(503, text="unavailable")
        return _order_ok()

    harness = KISHarness(handler)
    ack = await harness.rest.submit_cash_order(_order_request())
    assert ack.broker_order_id == "00001234"
    assert len(calls) == 2
    await harness.close()


async def test_expired_token_reissued_once() -> None:
    """A 401 must drop the cached token and retry with a fresh one."""
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        if request.headers.get("authorization") == "Bearer token-1":
            return httpx.Response(401, json={"rt_cd": "7", "msg_cd": "10004010"})
        return _order_ok()

    harness = KISHarness(handler)
    ack = await harness.rest.submit_cash_order(_order_request())
    assert ack.broker_order_id == "00001234"
    # Order attempt with token-1 → 401 → token re-issued as token-2 → retry OK.
    assert len(calls) == 2
    assert calls[0].headers["authorization"] == "Bearer token-1"
    assert calls[1].headers["authorization"] == "Bearer token-2"
    await harness.close()


async def test_audit_records_trace_requests_without_secrets() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return _order_ok()

    harness = KISHarness(handler)
    await harness.rest.submit_cash_order(_order_request())

    assert len(harness.audit_records) == 1
    record = harness.audit_records[0]
    assert record.method == "POST"
    assert record.tr_id == "VTTC0012U"
    assert record.http_status == 200
    assert record.rt_cd == "0"
    assert record.environment == "mock"
    assert record.order_summary["client_order_id"] == "c-1"
    assert record.order_summary["symbol"] == "005930"
    assert record.request_id

    serialized = json.dumps(record.as_dict())
    assert "appsecret-1" not in serialized
    assert "token-1" not in serialized
    assert "authorization" not in serialized
    await harness.close()


async def test_malformed_json_is_classified() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, text="<html>gateway error</html>")

    harness = KISHarness(handler)
    with pytest.raises(KISAPIError) as excinfo:
        await harness.rest.submit_cash_order(_order_request())
    assert excinfo.value.kind is APIErrorKind.MALFORMED
    await harness.close()


def test_account_number_formats() -> None:
    assert _parse("12345678-01") == ("12345678", "01")
    assert _parse("1234567801") == ("12345678", "01")
    with pytest.raises(ValueError, match="10 digits"):
        _parse("12345")


def _parse(account: str) -> tuple[str, str]:
    from lossfunction.broker.kis.rest import _parse_account_number

    return _parse_account_number(account)
