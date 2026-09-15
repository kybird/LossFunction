"""KIS WebSocket client tests with a scripted fake connection."""

import asyncio
import json
from decimal import Decimal

import httpx
import pytest

from lossfunction.broker.base import Quote
from lossfunction.marketdata import (
    H0STCNT0_COLUMNS,
    KIS_WS_URLS,
    KISMarketDataClient,
    build_subscribe_message,
    fetch_approval_key,
    parse_market_data,
)

DROP = object()  # scripted recv that raises to simulate a disconnect
_IDX = {name: i for i, name in enumerate(H0STCNT0_COLUMNS)}
assert len(H0STCNT0_COLUMNS) == 46  # official H0STCNT0 column count


def _data_frame(
    symbol: str = "005930", price: str = "80500", hour: str = "093012", date: str = "20260914"
) -> str:
    values = [""] * len(H0STCNT0_COLUMNS)
    values[_IDX["MKSC_SHRN_ISCD"]] = symbol
    values[_IDX["STCK_CNTG_HOUR"]] = hour
    values[_IDX["STCK_PRPR"]] = price
    values[_IDX["BSOP_DATE"]] = date
    return f"0|H0STCNT0|{symbol}|{chr(94).join(values)}"


class FakeConnection:
    def __init__(self, script: list) -> None:
        self._script = list(script)
        self.sent: list[dict] = []
        self.pongs: list[str] = []
        self.closed = False

    async def send(self, message: str) -> None:
        self.sent.append(json.loads(message))

    async def recv(self) -> str:
        if not self._script:
            await asyncio.Event().wait()  # stay connected until cancelled
        item = self._script.pop(0)
        if item is DROP:
            msg = "connection reset by peer"
            raise ConnectionError(msg)
        return item

    def pong(self, data: str) -> None:
        self.pongs.append(data)

    async def close(self) -> None:
        self.closed = True


class FakeConnect:
    def __init__(self, scripts: list) -> None:
        self.scripts = list(scripts)
        self.connections: list[FakeConnection] = []
        self.urls: list[str] = []

    async def __call__(self, url: str) -> FakeConnection:
        self.urls.append(url)
        script = self.scripts.pop(0)
        connection = FakeConnection(script)
        self.connections.append(connection)
        return connection


async def _noop_sleep(seconds: float) -> None:
    await asyncio.sleep(0)  # always yield so test predicates can run


def _client(connect: FakeConnect, quotes: list) -> KISMarketDataClient:
    async def approval_key() -> str:
        return "approval-1"

    return KISMarketDataClient(
        approval_key=approval_key,
        ws_url="ws://kis.example/ws",
        connect=connect,
        on_quote=quotes.append,
        sleep=_noop_sleep,
        reconnect_delays=(0.0,),
    )


def test_parse_single_and_multi_record_frames() -> None:
    frame = _data_frame(price="80500")
    sep = chr(94)
    two_records = frame + sep + sep.join(frame.split("|")[3].split(sep))
    quotes = parse_market_data(two_records)
    assert len(quotes) == 2
    assert quotes[0].symbol == "005930"
    assert quotes[0].last_price == Decimal("80500")
    assert quotes[0].timestamp.isoformat() == "2026-09-14T09:30:12+09:00"


def test_parse_rejects_malformed_frames() -> None:
    with pytest.raises(ValueError, match="unexpected market-data frame"):
        parse_market_data("not-a-frame")
    with pytest.raises(ValueError, match="not a multiple"):
        parse_market_data("0|H0STCNT0|005930|a|b|c")


def test_subscribe_message_shape() -> None:
    message = build_subscribe_message("approval-1", "H0STCNT0", "005930")
    assert message == {
        "header": {"approval_key": "approval-1", "tr_type": "1", "custtype": "P"},
        "body": {"input": {"tr_id": "H0STCNT0", "tr_key": "005930"}},
    }
    assert (
        build_subscribe_message("k", "H0STCNT0", "005930", subscribe=False)["header"]["tr_type"]
        == "0"
    )


def test_ws_urls_cover_real_and_mock() -> None:
    assert set(KIS_WS_URLS) == {"real", "mock"}


async def test_disconnect_reconnects_and_replays_subscriptions() -> None:
    connect = FakeConnect(
        [
            [_data_frame(price="80500"), DROP],  # first connection: data, then drop
            [_data_frame(price="81000")],  # second connection: data resumes
        ]
    )
    quotes: list[Quote] = []
    client = _client(connect, quotes)

    await client.subscribe("005930")
    task = asyncio.create_task(client.run())
    try:
        await _wait_until(lambda: len(connect.connections) == 2)
        await _wait_until(lambda: len(quotes) >= 2)
    finally:
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task

    # AC: reconnect happened and the subscription was replayed on connection 2.
    assert len(connect.connections) == 2
    first, second = connect.connections
    # connection 1 got the live subscribe; connection 2 got the replay.
    assert len(first.sent) == 1
    assert len(second.sent) == 1
    assert second.sent[0]["body"]["input"]["tr_key"] == "005930"
    assert second.sent[0]["header"]["tr_type"] == "1"
    # AC: quotes resumed after reconnection with fresh prices.
    assert [q.last_price for q in quotes] == [Decimal("80500"), Decimal("81000")]


async def test_pingpong_is_echoed() -> None:
    connect = FakeConnect(
        [
            ['{"header": {"tr_id": "PINGPONG"}}'],
        ]
    )
    quotes: list[Quote] = []
    client = _client(connect, quotes)
    task = asyncio.create_task(client.run())
    try:
        await _wait_until(
            lambda: bool(connect.connections) and len(connect.connections[0].pongs) == 1
        )
    finally:
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
    assert connect.connections[0].pongs == ['{"header": {"tr_id": "PINGPONG"}}']


async def test_unsubscribed_symbols_not_replayed() -> None:
    connect = FakeConnect(
        [
            [DROP],
            [],
        ]
    )
    quotes: list[Quote] = []
    client = _client(connect, quotes)
    await client.subscribe("005930")
    await client.subscribe("035420")
    await client.unsubscribe("035420")
    task = asyncio.create_task(client.run())
    try:
        await _wait_until(lambda: len(connect.connections) == 2)
        await asyncio.sleep(0)
    finally:
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
    replayed = [m["body"]["input"]["tr_key"] for m in connect.connections[1].sent]
    assert replayed == ["005930"]


async def test_desired_state_survives_disconnection() -> None:
    connect = FakeConnect([[DROP], []])
    client = _client(connect, [])
    await client.subscribe("005930")
    task = asyncio.create_task(client.run())
    try:
        await _wait_until(lambda: len(connect.connections) == 2)
        # While disconnected-or-reconnected, desired state is queryable.
        assert client.desired_subscriptions == frozenset({"005930"})
    finally:
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task


async def test_fetch_approval_key_uses_official_body() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(200, json={"approval_key": "approval-xyz"})

    key = await fetch_approval_key(
        "appkey-1",
        "appsecret-1",
        "https://kis.example",
        transport=httpx.MockTransport(handler),
    )
    assert key == "approval-xyz"
    assert calls[0].url.path == "/oauth2/Approval"
    assert json.loads(calls[0].read()) == {
        "grant_type": "client_credentials",
        "appkey": "appkey-1",
        "secretkey": "appsecret-1",  # official field name for this endpoint
    }


async def _wait_until(predicate, timeout: float = 2.0) -> None:
    deadline = asyncio.get_running_loop().time() + timeout
    while not predicate():
        if asyncio.get_running_loop().time() > deadline:
            msg = "condition not met before timeout"
            raise AssertionError(msg)
        await asyncio.sleep(0.01)
