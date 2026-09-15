"""Alerting — channels chosen by settings, deduped, and testable.

Channels: webhook (HTTP POST JSON) and console (log). The console channel is
always present so an unconfigured deployment still surfaces events in logs;
a configured webhook is added on top. Every emit is deduped per event type
within a configurable window — a flapping condition must not spam.
"""

import asyncio
import json
import logging
import time
from typing import Protocol

import httpx

from lossfunction.config import Settings

logger = logging.getLogger("lossfunction.alerts")


class AlertChannel(Protocol):
    name: str

    async def send(self, event_type: str, detail: dict) -> None: ...


class ConsoleChannel:
    name = "console"

    async def send(self, event_type: str, detail: dict) -> None:
        logger.warning("ALERT [%s] %s", event_type, json.dumps(detail, default=str))


class WebhookChannel:
    name = "webhook"

    def __init__(
        self,
        url: str,
        *,
        transport: httpx.AsyncBaseTransport | None = None,
        timeout: float = 10.0,
    ) -> None:
        self._url = url
        self._transport = transport
        self._timeout = timeout
        self._client: httpx.AsyncClient | None = None
        self.sent: list[dict] = []  # test observability

    async def _http(self) -> httpx.AsyncClient:
        if self._client is None:
            self._client = (
                httpx.AsyncClient(transport=self._transport, timeout=self._timeout)
                if self._transport is not None
                else httpx.AsyncClient(timeout=self._timeout)
            )
        return self._client

    async def send(self, event_type: str, detail: dict) -> None:
        payload = {"event_type": event_type, **detail}
        self.sent.append(payload)
        client = await self._http()
        response = await client.post(self._url, json=payload)
        if response.status_code >= 300:
            msg = f"webhook returned {response.status_code}"
            raise RuntimeError(msg)

    async def aclose(self) -> None:
        if self._client is not None:
            await self._client.aclose()
            self._client = None


def build_channels(
    settings: Settings, *, transport: httpx.AsyncBaseTransport | None = None
) -> list[AlertChannel]:
    """Console always; webhook only when ALERT_WEBHOOK_URL is configured."""
    channels: list[AlertChannel] = [ConsoleChannel()]
    if settings.alert_webhook_url:
        channels.append(WebhookChannel(settings.alert_webhook_url, transport=transport))
    return channels


class AlertManager:
    """Emits deduped alerts to all channels; never raises to the caller."""

    def __init__(
        self,
        channels: list[AlertChannel],
        *,
        min_interval_seconds: float = 30.0,
        clock=time.monotonic,
        on_emit=None,
    ) -> None:
        self._channels = channels
        self._min_interval = min_interval_seconds
        self._clock = clock
        self._last_emitted: dict[str, float] = {}
        self._on_emit = on_emit
        self._lock = asyncio.Lock()

    async def emit(self, event_type: str, detail: dict) -> bool:
        """Send to channels unless deduped; returns True when sent."""
        now = self._clock()
        async with self._lock:
            last = self._last_emitted.get(event_type)
            if last is not None and (now - last) < self._min_interval:
                return False
            self._last_emitted[event_type] = now
        if self._on_emit is not None:
            self._on_emit(event_type, detail)
        for channel in self._channels:
            try:
                await channel.send(event_type, detail)
            except Exception:  # noqa: BLE001 - alerting must never break trading
                logger.exception("alert channel %s failed", channel.name)
        return True
