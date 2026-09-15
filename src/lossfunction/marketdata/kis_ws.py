"""KIS WebSocket market-data client.

Spec verified against the official samples (koreainvestment/open-trading-api,
kis_auth.py `KISWebSocket`/`data_fetch` and domestic_stock_functions_ws.py):
- Subscribe message: {"header": {"approval_key", "tr_type": "1"|"0",
  "custtype": "P"}, "body": {"input": {"tr_id", "tr_key"}}}
- Data frames are pipe-delimited: `0|TR_ID|TR_KEY|v1^v2^...` where the 4th
  field repeats ^-separated records of the TR's column list.
- Keepalive: the server sends a message whose header tr_id is "PINGPONG";
  echo it back (websockets `pong`).
- Approval key: POST {rest_base}/oauth2/Approval with JSON
  {"grant_type": "client_credentials", "appkey", "secretkey"} (note the field
  is `secretkey`, not `appsecret`).
"""

import asyncio
import json
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime, timedelta, timezone
from typing import Any

import httpx

from lossfunction.broker.base import Quote

KST = timezone(timedelta(hours=9), name="KST")

KIS_WS_URLS: dict[str, str] = {
    "real": "ws://ops.koreainvestment.com:21000",
    "mock": "ws://vops.koreainvestment.com:21000",
}

# 국내주식 실시간체결가(KRX) H0STCNT0 — official column order.
H0STCNT0_COLUMNS: list[str] = [
    "MKSC_SHRN_ISCD",
    "STCK_CNTG_HOUR",
    "STCK_PRPR",
    "PRDY_VRSS_SIGN",
    "PRDY_VRSS",
    "PRDY_CTRT",
    "WGHN_AVRG_STCK_PRC",
    "STCK_OPRC",
    "STCK_HGPR",
    "STCK_LWPR",
    "ASKP1",
    "BIDP1",
    "CNTG_VOL",
    "ACML_VOL",
    "ACML_TR_PBMN",
    "SELN_CNTG_CSNU",
    "SHNU_CNTG_CSNU",
    "NTBY_CNTG_CSNU",
    "CTTR",
    "SELN_CNTG_SMTN",
    "SHNU_CNTG_SMTN",
    "CCLD_DVSN",
    "SHNU_RATE",
    "PRDY_VOL_VRSS_ACML_VOL_RATE",
    "OPRC_HOUR",
    "OPRC_VRSS_PRPR_SIGN",
    "OPRC_VRSS_PRPR",
    "HGPR_HOUR",
    "HGPR_VRSS_PRPR_SIGN",
    "HGPR_VRSS_PRPR",
    "LWPR_HOUR",
    "LWPR_VRSS_PRPR_SIGN",
    "LWPR_VRSS_PRPR",
    "BSOP_DATE",
    "NEW_MKOP_CLS_CODE",
    "TRHT_YN",
    "ASKP_RSQN1",
    "BIDP_RSQN1",
    "TOTAL_ASKP_RSQN",
    "TOTAL_BIDP_RSQN",
    "VOL_TNRT",
    "PRDY_SMNS_HOUR_ACML_VOL",
    "PRDY_SMNS_HOUR_ACML_VOL_RATE",
    "HOUR_CLS_CODE",
    "MRKT_TRTM_CLS_CODE",
    "VI_STND_PRC",
]

_IDX = {name: i for i, name in enumerate(H0STCNT0_COLUMNS)}


def _parse_timestamp(date: str, hour: str) -> datetime:
    return datetime.strptime(f"{date}{hour}", "%Y%m%d%H%M%S").replace(tzinfo=KST)  # noqa: DTZ007


def parse_market_data(raw: str) -> list[Quote]:
    """Parse one H0STCNT0 data frame into `Quote` domain events."""
    fields = raw.split("|")
    if len(fields) < 4 or fields[1] != "H0STCNT0":
        msg = f"unexpected market-data frame: {raw[:80]!r}"
        raise ValueError(msg)
    values = fields[3].split("^")
    width = len(H0STCNT0_COLUMNS)
    if len(values) % width != 0:
        msg = f"frame field count {len(values)} not a multiple of {width}"
        raise ValueError(msg)

    quotes: list[Quote] = []
    for offset in range(0, len(values), width):
        row = values[offset : offset + width]
        quotes.append(
            Quote(
                symbol=row[_IDX["MKSC_SHRN_ISCD"]],
                last_price=_decimal(row[_IDX["STCK_PRPR"]]),
                timestamp=_parse_timestamp(row[_IDX["BSOP_DATE"]], row[_IDX["STCK_CNTG_HOUR"]]),
            )
        )
    return quotes


def _decimal(value: str):
    from decimal import Decimal

    return Decimal(value)


def build_subscribe_message(
    approval_key: str, tr_id: str, tr_key: str, *, subscribe: bool = True
) -> dict[str, Any]:
    """Build the KIS subscribe/unsubscribe message (official shape)."""
    return {
        "header": {
            "approval_key": approval_key,
            "tr_type": "1" if subscribe else "0",
            "custtype": "P",
        },
        "body": {"input": {"tr_id": tr_id, "tr_key": tr_key}},
    }


def is_pingpong(raw: str) -> bool:
    """Detect the server keepalive message (JSON header tr_id PINGPONG)."""
    try:
        header = json.loads(raw).get("header", {})
    except (json.JSONDecodeError, AttributeError):
        return "PINGPONG" in raw
    return header.get("tr_id") == "PINGPONG"


async def fetch_approval_key(
    app_key: str,
    app_secret: str,
    base_url: str,
    *,
    transport: httpx.AsyncBaseTransport | None = None,
) -> str:
    """Fetch the WebSocket approval key (POST /oauth2/Approval)."""
    async with (
        httpx.AsyncClient(transport=transport, timeout=10.0)
        if transport is not None
        else httpx.AsyncClient(timeout=10.0) as client
    ):
        response = await client.post(
            f"{base_url.rstrip('/')}/oauth2/Approval",
            json={
                "grant_type": "client_credentials",
                "appkey": app_key,
                "secretkey": app_secret,
            },
            headers={"Content-Type": "application/json"},
        )
    payload = response.json()
    approval_key = payload.get("approval_key")
    if response.status_code != 200 or not approval_key:
        msg = (
            f"failed to fetch approval key: status={response.status_code} "
            f"body={response.text[:120]!r}"
        )
        raise RuntimeError(msg)
    return str(approval_key)


class WSConnection:
    """Minimal async text protocol over a WebSocket connection."""

    async def send(self, message: str) -> None:
        raise NotImplementedError

    async def recv(self) -> str:
        raise NotImplementedError

    def pong(self, data: str) -> None:
        """Queue a pong response (keepalive echo)."""


ConnectionFactory = Callable[[str], "WSConnection"]


async def websockets_factory(url: str) -> WSConnection:
    """Real connection factory backed by the `websockets` library."""
    import websockets

    class _WebsocketsConnection(WSConnection):
        def __init__(self, inner) -> None:
            self._inner = inner

        async def send(self, message: str) -> None:
            await self._inner.send(message)

        async def recv(self) -> str:
            return await self._inner.recv()

        def pong(self, data: str) -> None:
            self._inner.pong(data)

    connection = await websockets.connect(url, ping_interval=None)
    return _WebsocketsConnection(connection)


TR_ID = "H0STCNT0"


class KISMarketDataClient:
    """Streaming client with desired-state subscription replay.

    Desired subscriptions live in this object, not in the connection, so a
    reconnect can replay them — the failure mode documented in the raw log
    (subscription state lost with the connection) cannot recur.
    """

    def __init__(
        self,
        *,
        approval_key: Callable[[], Awaitable[str]],
        ws_url: str,
        connect: ConnectionFactory,
        on_quote: Callable[[Quote], None],
        clock: Callable[[], datetime] | None = None,
        sleep: Callable[[float], Awaitable[None]] | None = None,
        reconnect_delays: tuple[float, ...] = (1.0, 2.0, 4.0, 8.0, 16.0, 30.0),
    ) -> None:
        self._approval_key = approval_key
        self._ws_url = ws_url
        self._connect = connect
        self._on_quote = on_quote
        self._clock = clock or (lambda: datetime.now(tz=UTC))
        self._sleep = sleep or asyncio.sleep
        self._reconnect_delays = reconnect_delays
        self._desired: set[str] = set()
        self._connection: WSConnection | None = None

    @property
    def desired_subscriptions(self) -> frozenset[str]:
        return frozenset(self._desired)

    async def subscribe(self, symbol: str) -> None:
        self._desired.add(symbol)
        if self._connection is not None:
            await self._send_subscription(self._connection, symbol, subscribe=True)

    async def unsubscribe(self, symbol: str) -> None:
        self._desired.discard(symbol)
        if self._connection is not None:
            await self._send_subscription(self._connection, symbol, subscribe=False)

    async def _send_subscription(
        self, connection: WSConnection, symbol: str, *, subscribe: bool
    ) -> None:
        message = build_subscribe_message(
            await self._approval_key(), TR_ID, symbol, subscribe=subscribe
        )
        await connection.send(json.dumps(message))

    async def run(self) -> None:
        """Connect, replay subscriptions, stream until cancelled.

        Disconnects are expected: the loop reconnects with capped backoff.
        """
        delay_index = 0
        while True:
            try:
                connection = await self._connect(self._ws_url)
            except Exception:
                await self._sleep(self._reconnect_delays[delay_index])
                delay_index = min(delay_index + 1, len(self._reconnect_delays) - 1)
                continue

            self._connection = connection
            delay_index = 0
            try:
                for symbol in sorted(self._desired):
                    await self._send_subscription(connection, symbol, subscribe=True)
                while True:
                    try:
                        raw = await connection.recv()
                    except asyncio.CancelledError:
                        raise
                    except Exception:
                        # Read failure = disconnect; desired state is kept and
                        # replayed on the next connection.
                        self._connection = None
                        break
                    try:
                        await self._handle_raw(connection, raw)
                    except asyncio.CancelledError:
                        raise
                    except ValueError:
                        # Malformed frame — drop it, keep the connection.
                        continue
            except asyncio.CancelledError:
                raise
            finally:
                self._connection = None
            await self._sleep(self._reconnect_delays[delay_index])
            delay_index = min(delay_index + 1, len(self._reconnect_delays) - 1)

    async def _handle_raw(self, connection: WSConnection, raw: str) -> None:
        if raw.startswith(("0|", "1|")):
            for quote in parse_market_data(raw):
                self._on_quote(quote)
            return
        if is_pingpong(raw):
            connection.pong(raw)
            return
        # Other system messages (subscription acks, errors) — nothing to do yet.
