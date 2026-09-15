"""KIS REST API client — domestic-stock trading and quotation endpoints.

Endpoint specs verified against the official samples
(koreainvestment/open-trading-api, examples_llm/domestic_stock):
- order-cash:    POST /uapi/domestic-stock/v1/trading/order-cash
                 tr_id real buy/sell = TTTC0012U/TTTC0011U, mock = VTTC0012U/VTTC0011U
                 UPPERCASE string body: CANO, ACNT_PRDT_CD, PDNO, ORD_DVSN,
                 ORD_QTY, ORD_UNPR, EXCG_ID_DVSN_CD, ...
- inquire-balance: GET /uapi/domestic-stock/v1/trading/inquire-balance
                 tr_id real = TTTC8434R, mock = VTTC8434R
                 Paginates via header `tr_cont` M/F with CTX_AREA_FK100/NK100.
- inquire-price: GET /uapi/domestic-stock/v1/quotations/inquire-price
                 tr_id FHKST01010100 (real and mock); price field `stck_prpr`.
"""

import asyncio
import time
import uuid
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime
from decimal import Decimal
from enum import StrEnum
from typing import Any

import httpx

from lossfunction.broker.base import OrderAck, OrderRequest, Position, Quote
from lossfunction.broker.kis.auth import KISAuthClient
from lossfunction.domain.types import OrderSide, OrderType

_ORDER_PATH = "/uapi/domestic-stock/v1/trading/order-cash"
_RVCNCL_PATH = "/uapi/domestic-stock/v1/trading/order-rvsecncl"
_BALANCE_PATH = "/uapi/domestic-stock/v1/trading/inquire-balance"
_PRICE_PATH = "/uapi/domestic-stock/v1/quotations/inquire-price"

_ORD_DVSN = {OrderType.LIMIT: "00", OrderType.MARKET: "01"}


class APIErrorKind(StrEnum):
    NETWORK = "network"
    SERVER = "server_error"
    RATE_LIMITED = "rate_limited"
    INVALID_TOKEN = "invalid_token"  # 401 — token rejected, re-issue once
    API_REJECT = "api_reject"  # HTTP 200 but rt_cd != 0 (business rejection)
    MALFORMED = "malformed_response"


class KISAPIError(Exception):
    """KIS REST failure with machine-readable classification."""

    def __init__(
        self,
        kind: APIErrorKind,
        message: str,
        *,
        status_code: int | None = None,
        rt_cd: str | None = None,
        msg_cd: str | None = None,
    ) -> None:
        super().__init__(message)
        self.kind = kind
        self.status_code = status_code
        self.rt_cd = rt_cd
        self.msg_cd = msg_cd


class AuditRecord:
    """Structured trace of one HTTP attempt — no credentials ever included."""

    __slots__ = (
        "attempt",
        "elapsed_ms",
        "environment",
        "http_status",
        "method",
        "msg_cd",
        "order_summary",
        "path",
        "request_id",
        "rt_cd",
        "timestamp",
        "tr_id",
    )

    def __init__(
        self,
        *,
        request_id: str,
        attempt: int,
        environment: str,
        method: str,
        path: str,
        tr_id: str,
        http_status: int | None,
        rt_cd: str | None,
        msg_cd: str | None,
        elapsed_ms: float,
        timestamp: datetime,
        order_summary: dict[str, str] | None = None,
    ) -> None:
        self.request_id = request_id
        self.attempt = attempt
        self.environment = environment
        self.method = method
        self.path = path
        self.tr_id = tr_id
        self.http_status = http_status
        self.rt_cd = rt_cd
        self.msg_cd = msg_cd
        self.elapsed_ms = elapsed_ms
        self.timestamp = timestamp
        self.order_summary = order_summary

    def as_dict(self) -> dict[str, Any]:
        return {
            "request_id": self.request_id,
            "attempt": self.attempt,
            "environment": self.environment,
            "method": self.method,
            "path": self.path,
            "tr_id": self.tr_id,
            "http_status": self.http_status,
            "rt_cd": self.rt_cd,
            "msg_cd": self.msg_cd,
            "elapsed_ms": self.elapsed_ms,
            "timestamp": self.timestamp.isoformat(),
            "order_summary": self.order_summary,
        }


AuditCallback = Callable[[AuditRecord], None]


def _parse_account_number(account_number: str) -> tuple[str, str]:
    digits = account_number.replace("-", "").strip()
    if len(digits) != 10 or not digits.isdigit():
        msg = (
            "KIS account number must be 10 digits as 'XXXXXXXXXX-XX' "
            f"or 'XXXXXXXXXXXX'; got {account_number!r}"
        )
        raise ValueError(msg)
    return digits[:8], digits[8:]


def _backoff_delay(attempt: int) -> float:
    return 0.25 * (2 ** (attempt - 1))


class KISRestClient:
    """Typed wrapper over the KIS domestic-stock REST endpoints."""

    def __init__(
        self,
        *,
        auth: KISAuthClient,
        account_number: str,
        environment: str,
        base_url: str | None = None,
        transport: httpx.AsyncBaseTransport | None = None,
        clock: Callable[[], datetime] | None = None,
        sleep: Callable[[float], Awaitable[None]] | None = None,
        audit: AuditCallback | None = None,
        max_retries: int = 3,
        trading_mode: str = "paper",
    ) -> None:
        if environment not in ("real", "mock"):
            msg = f"environment must be 'real' or 'mock', got {environment!r}"
            raise ValueError(msg)
        self._auth = auth
        self._cano, self._prdt = _parse_account_number(account_number)
        self._environment = environment
        self._trading_mode = trading_mode
        self._base_url = (base_url or self._default_base_url(environment)).rstrip("/")
        self._transport = transport
        self._clock = clock or (lambda: datetime.now(tz=UTC))
        self._sleep = sleep or asyncio.sleep
        self._audit = audit
        self._max_retries = max_retries
        self._client: httpx.AsyncClient | None = None

    @staticmethod
    def _default_base_url(environment: str) -> str:
        from lossfunction.broker.kis.env import kis_base_url

        return kis_base_url(environment)

    def _http(self) -> httpx.AsyncClient:
        if self._client is None:
            self._client = (
                httpx.AsyncClient(transport=self._transport, timeout=10.0)
                if self._transport is not None
                else httpx.AsyncClient(timeout=10.0)
            )
        return self._client

    async def aclose(self) -> None:
        if self._client is not None:
            await self._client.aclose()
            self._client = None

    # ── public operations ──────────────────────────────────────────

    async def submit_cash_order(self, request: OrderRequest) -> tuple[OrderAck, dict[str, Any]]:
        """Submit a domestic-stock cash order (order-cash).

        Returns the ack plus the raw output dict (carries
        KRX_FWDG_ORD_ORGNO needed for later cancellation).
        """
        tr_id = self._order_tr_id(request.side)
        body = {
            "CANO": self._cano,
            "ACNT_PRDT_CD": self._prdt,
            "PDNO": request.symbol,
            "ORD_DVSN": _ORD_DVSN[request.order_type],
            "ORD_QTY": str(request.quantity),
            "ORD_UNPR": (
                str(request.limit_price) if request.order_type is OrderType.LIMIT else "0"
            ),
            "EXCG_ID_DVSN_CD": "KRX",
            "SLL_TYPE": "",
            "CNDT_PRIC": "",
        }
        payload = await self._request(
            "POST",
            _ORDER_PATH,
            tr_id,
            json_body=body,
            order_summary={
                "client_order_id": request.client_order_id,
                "symbol": request.symbol,
                "side": request.side.value,
                "order_type": request.order_type.value,
                "quantity": str(request.quantity),
                "limit_price": str(request.limit_price or ""),
                "trading_mode": self._trading_mode,
            },
        )
        output = payload.get("output", {})
        odno = output.get("ODNO")
        if not isinstance(odno, str) or not odno:
            raise KISAPIError(APIErrorKind.MALFORMED, "order-cash response missing output.ODNO")
        ack = OrderAck(client_order_id=request.client_order_id, broker_order_id=odno)
        return ack, output if isinstance(output, dict) else {}

    async def cancel_cash_order(
        self,
        *,
        broker_order_id: str,
        krx_fwdg_ord_orgno: str,
        ord_dvsn: str,
    ) -> None:
        """Cancel the full remainder of an order (order-rvsecncl).

        RVSE_CNCL_DVSN_CD "02" = cancel; QTY_ALL_ORD_YN "Y" = full remainder,
        so quantity/price are placeholders ("0").
        """
        body = {
            "CANO": self._cano,
            "ACNT_PRDT_CD": self._prdt,
            "KRX_FWDG_ORD_ORGNO": krx_fwdg_ord_orgno,
            "ORGN_ODNO": broker_order_id,
            "ORD_DVSN": ord_dvsn,
            "RVSE_CNCL_DVSN_CD": "02",
            "ORD_QTY": "0",
            "ORD_UNPR": "0",
            "QTY_ALL_ORD_YN": "Y",
            "EXCG_ID_DVSN_CD": "KRX",
        }
        tr_id = "VTTC0013U" if self._environment == "mock" else "TTTC0013U"
        await self._request(
            "POST",
            _RVCNCL_PATH,
            tr_id,
            json_body=body,
            order_summary={
                "client_order_id": "",
                "symbol": "",
                "side": "",
                "order_type": "",
                "quantity": "",
                "limit_price": "",
                "trading_mode": self._trading_mode,
                "action": "cancel",
                "broker_order_id": broker_order_id,
            },
        )

    async def fetch_positions(self) -> list[Position]:
        """Fetch held positions (inquire-balance), following pagination."""
        positions: list[dict[str, Any]] = []
        fk = nk = ""
        tr_cont = ""
        pages = 0
        while True:
            pages += 1
            if pages > 20:
                msg = "inquire-balance pagination exceeded 20 pages"
                raise KISAPIError(APIErrorKind.MALFORMED, msg)
            params = {
                "CANO": self._cano,
                "ACNT_PRDT_CD": self._prdt,
                "AFHR_FLPR_YN": "N",
                "OFL_YN": "",
                "INQR_DVSN": "02",
                "UNPR_DVSN": "01",
                "FUND_STTL_ICLD_YN": "N",
                "FNCG_AMT_AUTO_RDPT_YN": "N",
                "PRCS_DVSN": "00",
                "CTX_AREA_FK100": fk,
                "CTX_AREA_NK100": nk,
            }
            tr_id = "VTTC8434R" if self._environment == "mock" else "TTTC8434R"
            payload, headers = await self._request(
                "GET",
                _BALANCE_PATH,
                tr_id,
                params=params,
                tr_cont=tr_cont,
                want_headers=True,
            )
            rows = payload.get("output1")
            if isinstance(rows, list):
                positions.extend(row for row in rows if isinstance(row, dict))
            if headers.get("tr_cont") not in ("M", "F"):
                break
            fk = payload.get("ctx_area_fk100", "")
            nk = payload.get("ctx_area_nk100", "")
            tr_cont = "N"

        result: list[Position] = []
        for row in positions:
            try:
                quantity = int(str(row["hldg_qty"]))
                if quantity <= 0:
                    continue
                result.append(
                    Position(
                        symbol=str(row["pdno"]),
                        quantity=quantity,
                        average_price=Decimal(str(row["pchs_avg_pric"])),
                    )
                )
            except (KeyError, ValueError, ArithmeticError):
                raise KISAPIError(
                    APIErrorKind.MALFORMED,
                    f"inquire-balance row missing/invalid fields: {row!r:.200}",
                ) from None
        return result

    async def fetch_quote(self, symbol: str) -> Quote:
        """Fetch the latest price snapshot (inquire-price)."""
        params = {
            "FID_COND_MRKT_DIV_CODE": "J",
            "FID_INPUT_ISCD": symbol,
        }
        payload = await self._request("GET", _PRICE_PATH, "FHKST01010100", params=params)
        output = payload.get("output", {})
        raw_price = output.get("stck_prpr")
        try:
            price = Decimal(str(raw_price))
        except (TypeError, ArithmeticError):
            raise KISAPIError(
                APIErrorKind.MALFORMED,
                f"inquire-price response missing/invalid stck_prpr: {raw_price!r}",
            ) from None
        return Quote(symbol=symbol, last_price=price, timestamp=self._clock())

    # ── internals ──────────────────────────────────────────────────

    def _order_tr_id(self, side: OrderSide) -> str:
        if self._environment == "mock":
            return "VTTC0012U" if side is OrderSide.BUY else "VTTC0011U"
        return "TTTC0012U" if side is OrderSide.BUY else "TTTC0011U"

    async def _request(
        self,
        method: str,
        path: str,
        tr_id: str,
        *,
        params: dict[str, str] | None = None,
        json_body: dict[str, str] | None = None,
        tr_cont: str = "",
        order_summary: dict[str, str] | None = None,
        want_headers: bool = False,
    ) -> Any:
        request_id = uuid.uuid4().hex
        token = await self._auth.get_access_token()
        headers = {
            "authorization": f"Bearer {token}",
            "appkey": self._auth.app_key,
            "appsecret": self._auth.app_secret,
            "tr_id": tr_id,
            "custtype": "P",
            "tr_cont": tr_cont,
            "Content-Type": "application/json; charset=utf-8",
        }
        url = f"{self._base_url}{path}"
        last_error: KISAPIError | None = None
        token_reissued = False

        for attempt in range(self._max_retries + 1):
            if attempt > 0:
                await self._sleep(_backoff_delay(attempt))
            started = time.perf_counter()
            try:
                response = await self._http().request(
                    method, url, headers=headers, params=params, json=json_body
                )
            except httpx.HTTPError as exc:
                elapsed_ms = (time.perf_counter() - started) * 1000
                error = KISAPIError(APIErrorKind.NETWORK, f"network failure calling {path}: {exc}")
                self._emit_audit(
                    AuditRecord(
                        request_id=request_id,
                        attempt=attempt,
                        environment=self._environment,
                        method=method,
                        path=path,
                        tr_id=tr_id,
                        http_status=None,
                        rt_cd=None,
                        msg_cd=None,
                        elapsed_ms=elapsed_ms,
                        timestamp=self._clock(),
                        order_summary=order_summary,
                    )
                )
                last_error = error
                continue

            elapsed_ms = (time.perf_counter() - started) * 1000

            if response.status_code != 200:
                self._emit_audit(
                    self._audit_record(
                        request_id,
                        attempt,
                        method,
                        path,
                        tr_id,
                        response,
                        elapsed_ms,
                        order_summary,
                    )
                )
                if response.status_code == 401 and not token_reissued:
                    # Token rejected — drop the cache and retry once with a
                    # freshly issued token instead of failing the order path.
                    token_reissued = True
                    self._auth.invalidate()
                    headers["authorization"] = f"Bearer {await self._auth.get_access_token()}"
                    continue
                if response.status_code == 429:
                    last_error = KISAPIError(
                        APIErrorKind.RATE_LIMITED,
                        f"rate limited calling {path}",
                        status_code=response.status_code,
                    )
                    continue
                if response.status_code >= 500:
                    last_error = KISAPIError(
                        APIErrorKind.SERVER,
                        f"server error {response.status_code} calling {path}: "
                        f"{response.text[:120]!r}",
                        status_code=response.status_code,
                    )
                    continue
                raise KISAPIError(
                    APIErrorKind.INVALID_TOKEN
                    if response.status_code in (401, 403)
                    else APIErrorKind.MALFORMED,
                    f"unexpected status {response.status_code} calling {path}: "
                    f"{response.text[:120]!r}",
                    status_code=response.status_code,
                )

            try:
                payload: dict[str, Any] = response.json()
            except ValueError:
                error = KISAPIError(
                    APIErrorKind.MALFORMED,
                    f"non-JSON response from {path}: {response.text[:120]!r}",
                    status_code=response.status_code,
                )
                self._emit_audit(
                    self._audit_record(
                        request_id,
                        attempt,
                        method,
                        path,
                        tr_id,
                        response,
                        elapsed_ms,
                        order_summary,
                    )
                )
                raise error from None

            rt_cd = payload.get("rt_cd")
            self._emit_audit(
                self._audit_record(
                    request_id,
                    attempt,
                    method,
                    path,
                    tr_id,
                    response,
                    elapsed_ms,
                    order_summary,
                )
            )

            if rt_cd == "0":
                if want_headers:
                    return payload, response.headers
                return payload

            raise KISAPIError(
                APIErrorKind.API_REJECT,
                f"KIS rejected {path}: rt_cd={rt_cd} "
                f"msg_cd={payload.get('msg_cd')} msg1={payload.get('msg1')}",
                status_code=response.status_code,
                rt_cd=rt_cd,
                msg_cd=payload.get("msg_cd"),
            )

        assert last_error is not None
        raise last_error

    def _audit_record(
        self,
        request_id: str,
        attempt: int,
        method: str,
        path: str,
        tr_id: str,
        response: httpx.Response,
        elapsed_ms: float,
        order_summary: dict[str, str] | None,
    ) -> AuditRecord:
        payload: dict[str, Any] = {}
        try:
            payload = response.json()
        except ValueError:
            payload = {}
        return AuditRecord(
            request_id=request_id,
            attempt=attempt,
            environment=self._environment,
            method=method,
            path=path,
            tr_id=tr_id,
            http_status=response.status_code,
            rt_cd=payload.get("rt_cd"),
            msg_cd=payload.get("msg_cd"),
            elapsed_ms=elapsed_ms,
            timestamp=self._clock(),
            order_summary=order_summary,
        )

    def _emit_audit(self, record: AuditRecord) -> None:
        if self._audit is not None:
            self._audit(record)
