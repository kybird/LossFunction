"""Monitoring and alerting tests."""

import pytest

from lossfunction.config import Settings
from lossfunction.ops import (
    AlertManager,
    ConsoleChannel,
    MetricsRegistry,
    WebhookChannel,
    build_channels,
)

# ── AC1: metrics are recorded and snapshot-able ────────────────────


def test_metrics_counters_and_gauges() -> None:
    metrics = MetricsRegistry()
    metrics.inc("orders.submitted")
    metrics.inc("orders.submitted")
    metrics.inc("orders.rejected")
    metrics.gauge("positions.open", 3)

    assert metrics.counter("orders.submitted") == 2
    snapshot = metrics.snapshot()
    assert snapshot == {
        "counters": {"orders.rejected": 1, "orders.submitted": 2},
        "gauges": {"positions.open": 3.0},
    }


# ── AC2: failure scenarios raise alerts ────────────────────────────


class RecordingChannel:
    name = "recording"

    def __init__(self) -> None:
        self.events: list[tuple[str, dict]] = []

    async def send(self, event_type: str, detail: dict) -> None:
        self.events.append((event_type, detail))


async def test_kill_switch_scenario_alerts() -> None:
    channel = RecordingChannel()
    manager = AlertManager([channel], min_interval_seconds=30.0)
    sent = await manager.emit("risk.kill_switch", {"reason": "manual halt", "operator": "test"})
    assert sent is True
    assert channel.events == [("risk.kill_switch", {"reason": "manual halt", "operator": "test"})]


async def test_order_timeout_scenario_alerts() -> None:
    channel = RecordingChannel()
    manager = AlertManager([channel])
    await manager.emit("order.timeout", {"client_order_id": "ord-0001"})
    assert channel.events[0][0] == "order.timeout"


async def test_duplicate_alerts_are_deduped() -> None:
    channel = RecordingChannel()
    now = {"t": 100.0}
    manager = AlertManager([channel], min_interval_seconds=30.0, clock=lambda: now["t"])
    assert await manager.emit("ws.reconnect", {}) is True
    now["t"] = 110.0
    assert await manager.emit("ws.reconnect", {}) is False  # inside window
    now["t"] = 140.0
    assert await manager.emit("ws.reconnect", {}) is True  # window elapsed
    assert len(channel.events) == 2


async def test_channel_failure_does_not_propagate() -> None:
    class BrokenChannel:
        name = "broken"

        async def send(self, event_type: str, detail: dict) -> None:
            msg = "boom"
            raise RuntimeError(msg)

    healthy = RecordingChannel()
    manager = AlertManager([BrokenChannel(), healthy])
    assert await manager.emit("broker.error", {}) is True
    assert healthy.events  # broken channel didn't stop the others


async def test_webhook_channel_posts_payload() -> None:
    import json as jsonlib

    import httpx

    calls: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return httpx.Response(200, json={"ok": True})

    channel = WebhookChannel("https://hooks.example/alert", transport=httpx.MockTransport(handler))
    await channel.send("risk.kill_switch", {"reason": "daily loss limit breached"})
    assert len(calls) == 1
    payload = jsonlib.loads(calls[0].read())
    assert payload == {
        "event_type": "risk.kill_switch",
        "reason": "daily loss limit breached",
    }
    await channel.aclose()


# ── AC3: channels come from settings ───────────────────────────────


def _settings(**overrides: object) -> Settings:
    return Settings(_env_file=None, **overrides)  # type: ignore[call-arg]


def test_default_settings_use_console_only() -> None:
    channels = build_channels(_settings())
    assert len(channels) == 1
    assert isinstance(channels[0], ConsoleChannel)


def test_configured_webhook_is_added() -> None:
    channels = build_channels(_settings(alert_webhook_url="https://hooks.example/alert"))
    assert [type(channel).__name__ for channel in channels] == [
        "ConsoleChannel",
        "WebhookChannel",
    ]
    # A webhook failure (e.g. 500) is surfaced by the channel itself.
    with pytest.raises(RuntimeError):
        import asyncio

        import httpx

        channel = WebhookChannel(
            "https://hooks.example/alert",
            transport=httpx.MockTransport(lambda request: httpx.Response(500, text="down")),
        )
        asyncio.run(channel.send("x", {}))
