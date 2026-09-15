"""GLM market-analysis integration.

Calls an OpenAI-compatible chat-completions endpoint (default: Zhipu GLM)
and validates the reply against a strict schema — a malformed analysis is
rejected, never best-effort parsed. On any failure the service returns an
explicit `unknown` fallback so the trading system keeps running without
analysis; models are advisory and their absence must never halt trading.
"""

import asyncio
import json
import logging
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime
from enum import StrEnum
from typing import Any, Literal

import httpx
from pydantic import BaseModel, ConfigDict, Field, ValidationError

logger = logging.getLogger(__name__)


class MarketRegimeAnalysis(BaseModel):
    """The one schema a GLM market-regime reply must satisfy."""

    model_config = ConfigDict(frozen=True)

    regime: Literal["trending_up", "trending_down", "range", "volatile", "unknown"]
    confidence: float = Field(ge=0.0, le=1.0)
    summary: str = Field(min_length=1, max_length=2000)
    risk_notes: list[str] = Field(default_factory=list)


class GLMErrorKind(StrEnum):
    NETWORK = "network"
    SERVER = "server"
    AUTH = "auth"
    SCHEMA_VIOLATION = "schema_violation"
    EMPTY_RESPONSE = "empty_response"


class GLMAnalysisError(Exception):
    def __init__(self, kind: GLMErrorKind, message: str) -> None:
        super().__init__(f"{kind.value}: {message}")
        self.kind = kind


class AnalysisFallback(BaseModel):
    """What the system uses when GLM is unavailable — explicit, not silent."""

    model_config = ConfigDict(frozen=True)

    analysis: MarketRegimeAnalysis
    used_fallback: bool
    error_kind: str | None = None


_SYSTEM_PROMPT = (
    "You are a market analysis service. Reply with ONLY a JSON object with "
    'keys "regime" (one of trending_up, trending_down, range, volatile, '
    'unknown), "confidence" (0.0-1.0), "summary" (string), "risk_notes" '
    "(array of strings). No markdown, no extra text."
)


def _extract_json(text: str) -> Any:
    """Parse JSON possibly wrapped in markdown code fences."""
    stripped = text.strip()
    if stripped.startswith("```"):
        stripped = stripped.strip("`")
        if stripped.startswith("json"):
            stripped = stripped[4:]
        stripped = stripped.strip()
    return json.loads(stripped)


class GLMAnalysisClient:
    """Raw client: one call, one validated analysis or one classified error."""

    def __init__(
        self,
        *,
        api_key: str,
        base_url: str = "https://open.bigmodel.cn/api/paas/v4",
        model: str = "glm-4-flash",
        transport: httpx.AsyncBaseTransport | None = None,
        sleep: Callable[[float], Awaitable[None]] | None = None,
        max_retries: int = 2,
    ) -> None:
        self._api_key = api_key
        self._base_url = base_url.rstrip("/")
        self._model = model
        self._transport = transport
        self._sleep = sleep or asyncio.sleep
        self._max_retries = max_retries
        self._client: httpx.AsyncClient | None = None

    def _http(self) -> httpx.AsyncClient:
        if self._client is None:
            self._client = (
                httpx.AsyncClient(transport=self._transport, timeout=30.0)
                if self._transport is not None
                else httpx.AsyncClient(timeout=30.0)
            )
        return self._client

    async def aclose(self) -> None:
        if self._client is not None:
            await self._client.aclose()
            self._client = None

    async def analyze_regime(self, context: dict[str, Any]) -> MarketRegimeAnalysis:
        body = {
            "model": self._model,
            "messages": [
                {"role": "system", "content": _SYSTEM_PROMPT},
                {"role": "user", "content": json.dumps(context, default=str)},
            ],
            "temperature": 0.2,
        }
        headers = {"Authorization": f"Bearer {self._api_key}"}
        last_error: GLMAnalysisError | None = None

        for attempt in range(self._max_retries + 1):
            if attempt > 0:
                await self._sleep(0.25 * (2 ** (attempt - 1)))
            try:
                response = await self._http().post(
                    f"{self._base_url}/chat/completions",
                    json=body,
                    headers=headers,
                )
            except httpx.HTTPError as exc:
                last_error = GLMAnalysisError(GLMErrorKind.NETWORK, f"request failed: {exc}")
                continue

            if response.status_code in (401, 403):
                raise GLMAnalysisError(GLMErrorKind.AUTH, f"status {response.status_code}")
            if response.status_code >= 500:
                last_error = GLMAnalysisError(GLMErrorKind.SERVER, f"status {response.status_code}")
                continue

            try:
                payload = response.json()
                content = payload["choices"][0]["message"]["content"]
            except (ValueError, KeyError, IndexError, TypeError) as exc:
                raise GLMAnalysisError(
                    GLMErrorKind.EMPTY_RESPONSE, f"unusable response: {exc}"
                ) from exc

            try:
                parsed = _extract_json(str(content))
            except json.JSONDecodeError as exc:
                raise GLMAnalysisError(
                    GLMErrorKind.SCHEMA_VIOLATION, f"reply is not JSON: {exc}"
                ) from exc
            try:
                return MarketRegimeAnalysis.model_validate(parsed)
            except ValidationError as exc:
                raise GLMAnalysisError(
                    GLMErrorKind.SCHEMA_VIOLATION, f"reply violates schema: {exc}"
                ) from exc

        assert last_error is not None
        raise last_error


class RegimeAnalysisService:
    """Analysis with fallback and result persistence.

    `on_result` receives every analysis (real or fallback) — wire it to the
    storage layer (Repository.record_analysis) so results are auditable.
    """

    def __init__(
        self,
        client: GLMAnalysisClient | None,
        on_result: Callable[[str, dict[str, Any]], None] | None = None,
    ) -> None:
        self._client = client
        self._on_result = on_result

    async def analyze_regime(self, context: dict[str, Any]) -> AnalysisFallback:
        if self._client is None:
            return self._finish(self._fallback(None), context)

        try:
            analysis = await self._client.analyze_regime(context)
        except GLMAnalysisError as exc:
            logger.warning("GLM analysis failed (%s); using fallback", exc)
            return self._finish(self._fallback(exc.kind), context)
        return self._finish(AnalysisFallback(analysis=analysis, used_fallback=False), context)

    @staticmethod
    def _fallback(kind: GLMErrorKind | None) -> AnalysisFallback:
        return AnalysisFallback(
            analysis=MarketRegimeAnalysis(
                regime="unknown",
                confidence=0.0,
                summary="GLM analysis unavailable; trading continues without it",
                risk_notes=[],
            ),
            used_fallback=True,
            error_kind=kind.value if kind else "not_configured",
        )

    def _finish(self, result: AnalysisFallback, context: dict[str, Any]) -> AnalysisFallback:
        if self._on_result is not None:
            self._on_result(
                "regime",
                {
                    **json.loads(result.analysis.model_dump_json()),
                    "used_fallback": result.used_fallback,
                    "error_kind": result.error_kind,
                    "context_symbols": sorted(context.get("symbols", [])),
                    "recorded_at": datetime.now(tz=UTC).isoformat(),
                },
            )
        return result
