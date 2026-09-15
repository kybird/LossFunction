"""Container entrypoint — health, status page, minimal controls, demo loop.

Runs the process-level concerns of 24/7 operation:
- an HTTP endpoint serving `/healthz` (JSON), `/` (status page rendered from
  SQLite), and `POST /control/kill-switch` (the one manual control),
- settings load + broker/risk/database assembly at startup (live without the
  double confirmation refuses to boot — crash by design),
- optional demo loop (paper+memory) that keeps the pipeline visibly alive,
- graceful shutdown on SIGINT/SIGTERM.

Controls are bound to the same listener as the page: loopback-only by
deployment default — do not expose the port publicly.
"""

import asyncio
import contextlib
import json
import logging
import os
import signal
import sys
from datetime import timedelta
from decimal import Decimal

from lossfunction.broker.base import Broker
from lossfunction.broker.factory import build_broker
from lossfunction.config import Settings, load_settings
from lossfunction.risk import RiskLimits, RiskManager
from lossfunction.runtime.web import render_status_page
from lossfunction.storage import Repository

logger = logging.getLogger("lossfunction.runtime")

_SHUTDOWN = asyncio.Event()

_STATE: dict[str, object] = {
    "trading_mode": "paper",
    "broker": "memory",
    "database_path": "",
    "started_at": 0.0,
    "risk": None,
    "repository": None,
}

_MAX_BODY_BYTES = 4096


def _response(body: bytes, content_type: str, status: str = "200 OK") -> bytes:
    head = (
        f"HTTP/1.0 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {len(body)}\r\n\r\n"
    ).encode()
    return head + body


async def _render_page(repository: Repository) -> bytes:
    risk: RiskManager = _STATE["risk"]  # type: ignore[assignment]
    page = render_status_page(
        trading_mode=str(_STATE["trading_mode"]),
        broker=str(_STATE["broker"]),
        database_path=str(_STATE["database_path"]),
        kill_switch=risk.kill_switch_active,
        kill_reason=risk.kill_reason,
        positions=await repository.get_positions(),
        orders=await repository.list_recent_orders(limit=50),
        fills=await repository.list_recent_fills(limit=50),
        audit=await repository.latest_audit(limit=20),
        latest_prices=await repository.latest_quotes(),
    )
    return _response(page.encode("utf-8"), "text/html; charset=utf-8")


def _health_body() -> bytes:
    risk: RiskManager = _STATE["risk"]  # type: ignore[assignment]
    body = json.dumps(
        {
            "status": "ok",
            "service": "lossfunction-runtime",
            "trading_mode": _STATE["trading_mode"],
            "broker": _STATE["broker"],
            "kill_switch": risk.kill_switch_active,
            "kill_reason": risk.kill_reason,
            "uptime_seconds": round(
                asyncio.get_running_loop().time() - float(_STATE["started_at"])
            ),
        }
    ).encode()
    return _response(body, "application/json")


async def _handle_kill_switch(body: bytes) -> bytes:
    """POST /control/kill-switch  {"activate": bool, "reason": str}."""
    risk: RiskManager = _STATE["risk"]  # type: ignore[assignment]
    repository: Repository = _STATE["repository"]  # type: ignore[assignment]
    try:
        command = json.loads(body or b"{}")
        activate = bool(command["activate"])
        reason = str(command.get("reason") or "manual (web)")[:200]
    except (ValueError, KeyError, TypeError):
        return _response(b'{"error": "bad request"}', "application/json", "400")

    if activate:
        risk.activate_kill_switch(reason)
    else:
        risk.deactivate_kill_switch()
    await repository.record_event(
        "control.kill_switch",
        "operator",
        {"activate": activate, "reason": reason, "source": "web"},
    )
    logger.warning(
        "kill switch %s via web (reason: %s)",
        "ACTIVATED" if activate else "released",
        reason,
    )
    payload = json.dumps(
        {
            "ok": True,
            "kill_switch": risk.kill_switch_active,
            "reason": risk.kill_reason,
        }
    ).encode()
    return _response(payload, "application/json")


async def _handle(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    """Minimal HTTP/1.0 responder: /healthz, /, POST /control/kill-switch."""
    try:
        request_line = await asyncio.wait_for(reader.readline(), timeout=5.0)
        parts = request_line.decode("latin-1").split()
        method, path = (parts + ["", ""])[:2]
        content_length = 0
        while True:
            header = await reader.readline()
            if header in (b"\r\n", b"\n", b""):
                break
            name, _, value = header.partition(b":")
            if name.strip().lower() == b"content-length":
                content_length = int(value.strip() or b"0")

        route = path.split("?")[0]
        if route == "/healthz" and method == "GET":
            writer.write(_health_body())
        elif route in ("/", "/index.html") and method == "GET":
            try:
                writer.write(
                    await _render_page(_STATE["repository"])  # type: ignore[arg-type]
                )
            except Exception:
                logger.exception("status page rendering failed")
                writer.write(_response(b"status page error", "text/plain", "500"))
        elif route == "/control/kill-switch" and method == "POST":
            if content_length > _MAX_BODY_BYTES:
                writer.write(_response(b"body too large", "text/plain", "413"))
            else:
                body = await reader.readexactly(content_length) if content_length else b""
                writer.write(await _handle_kill_switch(body))
        elif route in ("/healthz", "/control/kill-switch"):
            writer.write(_response(b"method not allowed", "text/plain", "405"))
        else:
            writer.write(_response(b"not found", "text/plain", "404"))
        await writer.drain()
    except (TimeoutError, ConnectionError, asyncio.IncompleteReadError):
        pass
    finally:
        writer.close()


def _request_shutdown(*_: object) -> None:
    _SHUTDOWN.set()


def _build_risk(settings: Settings) -> RiskManager:
    limits = RiskLimits(
        max_order_notional=Decimal(settings.risk_max_order_notional),
        max_position_quantity=settings.risk_max_position_quantity,
        max_gross_exposure=Decimal(settings.risk_gross_exposure),
        daily_loss_limit=Decimal(settings.risk_daily_loss_limit),
        stale_quote_max_age=timedelta(seconds=settings.risk_stale_quote_seconds),
    )
    return RiskManager(limits)


def _maybe_start_demo(
    settings: Settings, broker: Broker, repository: Repository
) -> asyncio.Task | None:
    """Start the demo loop only on the safe in-memory paper backend."""
    from lossfunction.broker.mock import MockBroker

    if os.environ.get("DEMO_LOOP", "").lower() not in ("1", "true", "yes"):
        return None
    if not isinstance(broker, MockBroker):
        logger.warning("DEMO_LOOP ignored: only valid with paper+memory backend")
        return None
    from lossfunction.runtime.demo import DemoLoop

    demo = DemoLoop(
        broker=broker,
        risk=_STATE["risk"],
        repository=repository,  # type: ignore[arg-type]
    )
    task = asyncio.get_running_loop().create_task(demo.run())
    logger.info("demo loop enabled (synthetic paper market)")
    return task


def main(argv: list[str] | None = None) -> int:
    logging.basicConfig(level=logging.INFO)
    port = int(os.environ.get("HEALTH_PORT", "8080"))

    settings: Settings = load_settings()
    _STATE["trading_mode"] = settings.trading_mode.value
    _STATE["database_path"] = settings.database_path
    broker: Broker = build_broker(settings)
    _STATE["broker"] = type(broker).__name__
    _STATE["risk"] = _build_risk(settings)
    logger.info("runtime starting: mode=%s broker=%s", settings.trading_mode, _STATE["broker"])

    async def serve() -> None:
        loop = asyncio.get_running_loop()
        _STATE["started_at"] = loop.time()

        repository = await Repository.connect(settings.database_path)
        applied = await repository.migrate()
        if applied:
            logger.info("database migrations applied: %s", applied)
        _STATE["repository"] = repository

        demo_task = _maybe_start_demo(settings, broker, repository)

        server = await asyncio.start_server(_handle, "0.0.0.0", port)
        for sig in (signal.SIGINT, signal.SIGTERM):
            try:
                loop.add_signal_handler(sig, _request_shutdown)
            except NotImplementedError:  # Windows
                signal.signal(sig, _request_shutdown)
        logger.info("http listening on 0.0.0.0:%s (/, /healthz, controls)", port)

        try:
            async with server:
                await _SHUTDOWN.wait()
        finally:
            if demo_task is not None:
                demo_task.cancel()
            await repository.close()

    with contextlib.suppress(KeyboardInterrupt):
        asyncio.run(serve())
    logger.info("runtime stopped cleanly")
    return 0


if __name__ == "__main__":
    sys.exit(main())
