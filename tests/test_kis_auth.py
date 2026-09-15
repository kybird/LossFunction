"""KIS auth client tests against a mock HTTP transport."""

import json
from datetime import datetime, timedelta, timezone

import httpx
import pytest

from lossfunction.broker.kis import (
    KIS_BASE_URLS,
    AuthErrorKind,
    KISAuthClient,
    KISAuthError,
    kis_base_url,
)

KST = timezone(timedelta(hours=9))


class FakeClock:
    def __init__(self, start: datetime) -> None:
        self.now = start

    def __call__(self) -> datetime:
        return self.now


class SleepRecorder:
    def __init__(self) -> None:
        self.delays: list[float] = []

    async def __call__(self, seconds: float) -> None:
        self.delays.append(seconds)


def _token_payload(expires_at: str = "2026-09-15 10:00:00") -> dict:
    return {
        "access_token": "token-abc",
        "access_token_token_expired": expires_at,
        "token_type": "Bearer",
        "expires_in": 86400,
    }


def _make_client(
    handler: httpx.MockTransport,
    *,
    now: datetime = datetime(2026, 9, 14, 10, 0, 0, tzinfo=KST),
) -> tuple[KISAuthClient, FakeClock, SleepRecorder]:
    clock = FakeClock(now)
    sleep = SleepRecorder()
    client = KISAuthClient(
        "appkey-1",
        "appsecret-1",
        "https://kis.example",
        transport=httpx.MockTransport(handler),
        clock=clock,
        sleep=sleep,
    )
    return client, clock, sleep


def _issue_request_bodies(handler_calls: list[httpx.Request]) -> list[dict]:
    return [json.loads(r.read()) for r in handler_calls if r.url.path == "/oauth2/tokenP"]


async def test_issue_token_success() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(200, json=_token_payload())

    client, clock, _ = _make_client(handler)
    token = await client.get_access_token()
    assert token == "token-abc"

    bodies = _issue_request_bodies(calls)
    assert len(bodies) == 1
    assert bodies[0] == {
        "grant_type": "client_credentials",
        "appkey": "appkey-1",
        "appsecret": "appsecret-1",
    }
    assert calls[0].headers["content-type"].startswith("application/json")
    await client.aclose()


async def test_token_is_cached_until_near_expiry() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(200, json=_token_payload())

    client, clock, _ = _make_client(handler)
    await client.get_access_token()
    await client.get_access_token()
    assert len(_issue_request_bodies(calls)) == 1

    # 10 minutes before expiry: still more than the 5-minute margin → cached.
    clock.now = datetime(2026, 9, 15, 9, 50, 0, tzinfo=KST)
    await client.get_access_token()
    assert len(_issue_request_bodies(calls)) == 1

    # 4 minutes before expiry: past the refresh margin → proactively re-issued.
    clock.now = datetime(2026, 9, 15, 9, 56, 0, tzinfo=KST)
    await client.get_access_token()
    assert len(_issue_request_bodies(calls)) == 2
    await client.aclose()


async def test_token_reissued_after_expiry() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(200, json=_token_payload(expires_at="2026-09-14 11:00:00"))

    client, clock, _ = _make_client(handler)
    await client.get_access_token()
    clock.now = datetime(2026, 9, 14, 12, 0, 0, tzinfo=KST)  # past expiry
    await client.get_access_token()
    assert len(_issue_request_bodies(calls)) == 2
    await client.aclose()


async def test_invalid_credentials_not_retried() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(401, text='{"rt_cd": "1", "msg_cd": "1000"}')

    client, _, _ = _make_client(handler)
    with pytest.raises(KISAuthError) as excinfo:
        await client.get_access_token()
    assert excinfo.value.kind is AuthErrorKind.INVALID_CREDENTIALS
    assert excinfo.value.retryable is False
    assert len(_issue_request_bodies(calls)) == 1
    await client.aclose()


async def test_server_error_retried_with_backoff_then_succeeds() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        if len(calls) == 1:
            return httpx.Response(503, text="service unavailable")
        return httpx.Response(200, json=_token_payload())

    client, _, sleep = _make_client(handler)
    token = await client.get_access_token()
    assert token == "token-abc"
    assert len(_issue_request_bodies(calls)) == 2
    assert sleep.delays == [pytest.approx(0.25)]
    await client.aclose()


async def test_network_error_retried_then_raises() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        raise httpx.ConnectError("connection refused")

    client, _, sleep = _make_client(handler)
    with pytest.raises(KISAuthError) as excinfo:
        await client.get_access_token()
    assert excinfo.value.kind is AuthErrorKind.NETWORK
    assert len(_issue_request_bodies(calls)) == 4  # 1 + 3 retries
    assert len(sleep.delays) == 3
    await client.aclose()


async def test_rate_limit_is_retryable() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        if len(calls) == 1:
            return httpx.Response(429, text="too many requests")
        return httpx.Response(200, json=_token_payload())

    client, _, _ = _make_client(handler)
    assert await client.get_access_token() == "token-abc"
    await client.aclose()


async def test_malformed_response_not_retried() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(200, json={"unexpected": "shape"})

    client, _, sleep = _make_client(handler)
    with pytest.raises(KISAuthError) as excinfo:
        await client.get_access_token()
    assert excinfo.value.kind is AuthErrorKind.MALFORMED_RESPONSE
    assert sleep.delays == []
    assert len(_issue_request_bodies(calls)) == 1
    await client.aclose()


async def test_concurrent_calls_single_flight() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(200, json=_token_payload())

    client, _, _ = _make_client(handler)
    tokens = await _gather_tokens(client)
    assert tokens == ["token-abc"] * 5
    assert len(_issue_request_bodies(calls)) == 1
    await client.aclose()


async def _gather_tokens(client: KISAuthClient) -> list[str]:
    import asyncio

    return list(await asyncio.gather(*(client.get_access_token() for _ in range(5))))


def test_base_urls_for_real_and_mock_environments() -> None:
    assert kis_base_url("real") == "https://openapi.koreainvestment.com:9443"
    assert kis_base_url("mock") == "https://openapivts.koreainvestment.com:9443"
    assert set(KIS_BASE_URLS) == {"real", "mock"}
    with pytest.raises(ValueError, match="unknown KIS environment"):
        kis_base_url("moon")
