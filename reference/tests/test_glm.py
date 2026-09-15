"""GLM analysis integration tests — schema, persistence, fallback."""

from typing import Any

import httpx
import pytest

from lossfunction.analysis import (
    GLMAnalysisClient,
    GLMAnalysisError,
    GLMErrorKind,
    RegimeAnalysisService,
)


def _reply(content: str) -> httpx.Response:
    return httpx.Response(200, json={"choices": [{"message": {"content": content}}]})


def _valid_content() -> str:
    return (
        '{"regime": "trending_up", "confidence": 0.72, '
        '"summary": "index above MA20 with rising volume", '
        '"risk_notes": ["concentration in semis"]}'
    )


def _client(handler, **overrides: Any) -> GLMAnalysisClient:
    defaults: dict[str, Any] = {
        "api_key": "glm-key",
        "transport": httpx.MockTransport(handler),
        "sleep": _no_sleep,
    }
    defaults.update(overrides)
    return GLMAnalysisClient(**defaults)


async def _no_sleep(seconds: float) -> None:
    return None


CONTEXT = {"symbols": ["005930"], "recent_returns": [0.01, -0.002, 0.013]}


# ── AC1: responses are schema-validated; violations rejected ───────


async def test_valid_response_parsed() -> None:
    client = _client(lambda request: _reply(_valid_content()))
    analysis = await client.analyze_regime(CONTEXT)
    assert analysis.regime == "trending_up"
    assert analysis.confidence == pytest.approx(0.72)
    await client.aclose()


async def test_fenced_json_accepted() -> None:
    fenced = f"```json\n{_valid_content()}\n```"
    client = _client(lambda request: _reply(fenced))
    analysis = await client.analyze_regime(CONTEXT)
    assert analysis.regime == "trending_up"
    await client.aclose()


async def test_schema_violation_rejected_without_retry() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return _reply('{"regime": "moon_mode", "confidence": 9}')

    client = _client(handler)
    with pytest.raises(GLMAnalysisError) as excinfo:
        await client.analyze_regime(CONTEXT)
    assert excinfo.value.kind is GLMErrorKind.SCHEMA_VIOLATION
    assert len(calls) == 1  # junk is rejected, not retried
    await client.aclose()


async def test_non_json_reply_rejected() -> None:
    client = _client(lambda request: _reply("I think the market looks fine."))
    with pytest.raises(GLMAnalysisError) as excinfo:
        await client.analyze_regime(CONTEXT)
    assert excinfo.value.kind is GLMErrorKind.SCHEMA_VIOLATION
    await client.aclose()


async def test_empty_choices_rejected() -> None:
    client = _client(lambda request: httpx.Response(200, json={"choices": []}))
    with pytest.raises(GLMAnalysisError) as excinfo:
        await client.analyze_regime(CONTEXT)
    assert excinfo.value.kind is GLMErrorKind.EMPTY_RESPONSE
    await client.aclose()


# ── AC3: failures fall back without stopping the system ────────────


async def test_network_failure_uses_fallback_after_retries() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        raise httpx.ConnectError("glitch")

    service = RegimeAnalysisService(_client(handler))
    result = await service.analyze_regime(CONTEXT)
    assert result.used_fallback is True
    assert result.analysis.regime == "unknown"
    assert result.error_kind == "network"
    assert len(calls) == 3  # 1 + 2 retries, then fallback
    assert result.analysis.confidence == 0.0


async def test_server_error_uses_fallback() -> None:
    service = RegimeAnalysisService(_client(lambda request: httpx.Response(503, text="down")))
    result = await service.analyze_regime(CONTEXT)
    assert result.used_fallback
    assert result.error_kind == "server"


async def test_auth_failure_uses_fallback_immediately() -> None:
    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(401, text="bad key")

    service = RegimeAnalysisService(_client(handler))
    result = await service.analyze_regime(CONTEXT)
    assert result.used_fallback
    assert result.error_kind == "auth"
    assert len(calls) == 1  # auth errors are not retried


async def test_unconfigured_client_is_pure_fallback() -> None:
    service = RegimeAnalysisService(None)
    result = await service.analyze_regime(CONTEXT)
    assert result.used_fallback
    assert result.error_kind == "not_configured"


# ── AC2: results reach the storage layer ───────────────────────────


async def test_results_persisted_to_storage() -> None:
    captured: list[tuple[str, dict]] = []

    def on_result(kind: str, payload: dict) -> None:
        captured.append((kind, payload))

    service = RegimeAnalysisService(
        _client(lambda request: _reply(_valid_content())), on_result=on_result
    )
    result = await service.analyze_regime(CONTEXT)
    assert result.used_fallback is False

    kind, payload = captured[0]
    assert kind == "regime"
    assert payload["regime"] == "trending_up"
    assert payload["used_fallback"] is False
    assert payload["context_symbols"] == ["005930"]
    assert "recorded_at" in payload

    # Fallbacks are recorded too — the outage itself is auditable.
    captured.clear()
    down = RegimeAnalysisService(
        _client(lambda request: httpx.Response(500, text="boom")),
        on_result=lambda kind, payload: captured.append((kind, payload)),
    )
    await down.analyze_regime(CONTEXT)
    assert captured[0][1]["used_fallback"] is True
    assert captured[0][1]["error_kind"] == "server"


@pytest.mark.integration
async def test_repository_records_analysis(tmp_path) -> None:
    from lossfunction.storage import Repository

    repo = await Repository.connect(tmp_path / "glm.db")
    try:
        await repo.migrate()
        await repo.record_analysis(
            "regime",
            {"regime": "volatile", "confidence": 0.5, "used_fallback": False},
        )
        events = await repo.get_audit_log(subject="analysis:regime")
        assert events
        assert events[-1]["event_type"] == "analysis.regime"
        assert events[-1]["payload"]["regime"] == "volatile"
    finally:
        await repo.close()
