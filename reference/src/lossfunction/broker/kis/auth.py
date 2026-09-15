"""KIS OAuth access-token lifecycle.

Verified against the official sample (koreainvestment/open-trading-api,
examples_llm/kis_auth.py `auth()`):
- `POST {base_url}/oauth2/tokenP`, JSON body
  `{"grant_type": "client_credentials", "appkey": ..., "appsecret": ...}`
- 200 response carries `access_token` and `access_token_token_expired`
  (`"YYYY-MM-DD HH:MM:SS"`, KST wall time, ~1 day validity; re-issuing within
  6 hours returns the same token server-side).

The client caches the token, re-issues it before expiry (with a safety
margin), and classifies failures so callers can decide between retry and halt.
"""

import asyncio
import json
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime, timedelta, timezone
from enum import StrEnum
from typing import Any

import httpx

_KST = timezone(timedelta(hours=9), name="KST")

_EXPIRY_FORMAT = "%Y-%m-%d %H:%M:%S"

_RETRYABLE_STATUSES = frozenset({429, 500, 502, 503, 504})


class AuthErrorKind(StrEnum):
    INVALID_CREDENTIALS = "invalid_credentials"  # 401/403 — retrying cannot help
    RATE_LIMITED = "rate_limited"  # 429 — retry after backoff
    SERVER_ERROR = "server_error"  # 5xx — retry after backoff
    NETWORK = "network"  # transport-level failure
    MALFORMED_RESPONSE = "malformed_response"  # 200 body unusable
    UNEXPECTED_STATUS = "unexpected_status"  # other 4xx


_RETRYABLE_KINDS = frozenset(
    {AuthErrorKind.RATE_LIMITED, AuthErrorKind.SERVER_ERROR, AuthErrorKind.NETWORK}
)


class KISAuthError(Exception):
    """KIS authentication failure with a machine-readable classification."""

    def __init__(
        self,
        kind: AuthErrorKind,
        message: str,
        *,
        status_code: int | None = None,
    ) -> None:
        super().__init__(message)
        self.kind = kind
        self.status_code = status_code

    @property
    def retryable(self) -> bool:
        return self.kind in _RETRYABLE_KINDS


class _TokenInfo:
    __slots__ = ("access_token", "expires_at")

    def __init__(self, access_token: str, expires_at: datetime) -> None:
        self.access_token = access_token
        self.expires_at = expires_at


def _parse_expiry(raw: str) -> datetime:
    return datetime.strptime(raw, _EXPIRY_FORMAT).replace(tzinfo=_KST)  # noqa: DTZ007


class KISAuthClient:
    """Issues and caches KIS access tokens for one app credential pair."""

    def __init__(
        self,
        app_key: str,
        app_secret: str,
        base_url: str,
        *,
        transport: httpx.AsyncBaseTransport | None = None,
        clock: Callable[[], datetime] | None = None,
        sleep: Callable[[float], Awaitable[None]] | None = None,
        max_retries: int = 3,
        refresh_margin: timedelta = timedelta(minutes=5),
    ) -> None:
        self._app_key = app_key
        self._app_secret = app_secret
        self._base_url = base_url.rstrip("/")
        self._transport = transport
        self._clock = clock or (lambda: datetime.now(tz=UTC))
        self._sleep = sleep or asyncio.sleep
        self._max_retries = max_retries
        self._refresh_margin = refresh_margin
        self._token: _TokenInfo | None = None
        self._issue_lock = asyncio.Lock()
        self._client: httpx.AsyncClient | None = None

    @property
    def app_key(self) -> str:
        return self._app_key

    @property
    def app_secret(self) -> str:
        return self._app_secret

    @property
    def base_url(self) -> str:
        return self._base_url

    def invalidate(self) -> None:
        """Drop the cached token so the next `get_access_token()` re-issues."""
        self._token = None

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

    def _token_is_valid(self) -> bool:
        return (
            self._token is not None
            and self._clock() < self._token.expires_at - self._refresh_margin
        )

    async def get_access_token(self) -> str:
        """Return a valid access token, issuing a new one when needed.

        Concurrent callers coalesce into a single issue request (double-checked
        lock).
        """
        if self._token_is_valid():
            return self._token.access_token  # type: ignore[union-attr]

        async with self._issue_lock:
            if self._token_is_valid():
                return self._token.access_token  # type: ignore[union-attr]
            self._token = await self._issue_token()
            return self._token.access_token

    async def _issue_token(self) -> _TokenInfo:
        body = {
            "grant_type": "client_credentials",
            "appkey": self._app_key,
            "appsecret": self._app_secret,
        }
        url = f"{self._base_url}/oauth2/tokenP"
        last_error: KISAuthError | None = None

        for attempt in range(self._max_retries + 1):
            if attempt > 0:
                await self._sleep(_backoff_delay(attempt))
            try:
                response = await self._http().post(
                    url,
                    json=body,
                    headers={"Content-Type": "application/json"},
                )
            except httpx.HTTPError as exc:
                last_error = KISAuthError(
                    AuthErrorKind.NETWORK,
                    f"network failure issuing KIS access token: {exc}",
                )
                continue

            if response.status_code == 200:
                parsed = _parse_token_response(response.text)
                if parsed is None:
                    raise KISAuthError(
                        AuthErrorKind.MALFORMED_RESPONSE,
                        "KIS token response missing access_token/access_token_token_expired",
                        status_code=200,
                    )
                access_token, expires_at = parsed
                return _TokenInfo(access_token, expires_at)

            error = _error_from_status(response.status_code, response.text)
            if not error.retryable:
                raise error
            last_error = error

        assert last_error is not None
        raise last_error


def _backoff_delay(attempt: int) -> float:
    return 0.25 * (2 ** (attempt - 1))


def _parse_token_response(text: str) -> tuple[str, datetime] | None:
    try:
        payload: dict[str, Any] = json.loads(text)
        access_token = payload["access_token"]
        expires_at = _parse_expiry(payload["access_token_token_expired"])
    except (json.JSONDecodeError, KeyError, ValueError, TypeError):
        return None
    if not isinstance(access_token, str) or not access_token:
        return None
    return access_token, expires_at


def _error_from_status(status_code: int, body: str) -> KISAuthError:
    detail = body[:200] if body else "(empty body)"
    if status_code in (401, 403):
        kind = AuthErrorKind.INVALID_CREDENTIALS
    elif status_code == 429:
        kind = AuthErrorKind.RATE_LIMITED
    elif status_code >= 500 or status_code in _RETRYABLE_STATUSES:
        kind = AuthErrorKind.SERVER_ERROR
    else:
        kind = AuthErrorKind.UNEXPECTED_STATUS
    return KISAuthError(
        kind,
        f"KIS token request failed with status {status_code}: {detail}",
        status_code=status_code,
    )
