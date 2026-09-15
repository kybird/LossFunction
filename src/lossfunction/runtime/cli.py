"""Container entrypoint — health endpoint, status page, runtime skeleton.

Runs the process-level concerns of 24/7 operation:
- an HTTP endpoint serving `/healthz` (JSON) and `/` (lightweight HTML
  status page rendered from SQLite),
- settings load + broker assembly + database open/migrate at startup
  (live without the double confirmation refuses to boot — a crash by
  design that the container restart policy then surfaces),
- graceful shutdown on SIGINT/SIGTERM (so restarts are clean).

The trading loop itself activates when market-data credentials are wired;
until then the process stays healthy and idle.
"""

import asyncio
import contextlib
import json
import logging
import os
import signal
import sys

from lossfunction.broker.base import Broker
from lossfunction.broker.factory import build_broker
from lossfunction.config import Settings, load_settings
from lossfunction.runtime.web import render_status_page
from lossfunction.storage import Repository

logger = logging.getLogger("lossfunction.runtime")

_SHUTDOWN = asyncio.Event()

_STATE: dict[str, object] = {
    "trading_mode": "paper",
    "broker": "memory",
    "database_path": "",
    "started_at": 0.0,
}


def _response(body: bytes, content_type: str, status: str = "200 OK") -> bytes:
    head = (
        f"HTTP/1.0 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {len(body)}\r\n\r\n"
    ).encode()
    return head + body


async def _render_page(repository: Repository) -> bytes:
    page = render_status_page(
        trading_mode=str(_STATE["trading_mode"]),
        broker=str(_STATE["broker"]),
        database_path=str(_STATE["database_path"]),
        positions=await repository.get_positions(),
        orders=await repository.list_recent_orders(limit=50),
        fills=await repository.list_recent_fills(limit=50),
        audit=await repository.latest_audit(limit=20),
        latest_prices=await repository.latest_quotes(),
    )
    return _response(page.encode("utf-8"), "text/html; charset=utf-8")


def _health_body() -> bytes:
    body = json.dumps(
        {
            "status": "ok",
            "service": "lossfunction-runtime",
            "trading_mode": _STATE["trading_mode"],
            "broker": _STATE["broker"],
            "uptime_seconds": round(
                asyncio.get_running_loop().time() - float(_STATE["started_at"])
            ),
        }
    ).encode()
    return _response(body, "application/json")


def _make_handler(repository: Repository):
    async def handle(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        """Minimal HTTP/1.0 responder: GET /healthz and GET /."""
        try:
            request_line = await asyncio.wait_for(reader.readline(), timeout=5.0)
            parts = request_line.decode("latin-1").split()
            method, path = (parts + ["", ""])[:2]
            while (await reader.readline()) not in (b"\r\n", b"\n", b""):
                pass  # drain headers

            route = path.split("?")[0]
            if method != "GET":
                writer.write(_response(b"method not allowed", "text/plain", "405"))
            elif route == "/healthz":
                writer.write(_health_body())
            elif route in ("/", "/index.html"):
                try:
                    writer.write(await _render_page(repository))
                except Exception:
                    logger.exception("status page rendering failed")
                    writer.write(_response(b"status page error", "text/plain", "500"))
            else:
                writer.write(_response(b"not found", "text/plain", "404"))
            await writer.drain()
        except (TimeoutError, ConnectionError):
            pass
        finally:
            writer.close()

    return handle


def _request_shutdown(*_: object) -> None:
    _SHUTDOWN.set()


def main(argv: list[str] | None = None) -> int:
    logging.basicConfig(level=logging.INFO)
    port = int(os.environ.get("HEALTH_PORT", "8080"))

    settings: Settings = load_settings()
    _STATE["trading_mode"] = settings.trading_mode.value
    _STATE["database_path"] = settings.database_path
    broker: Broker = build_broker(settings)
    _STATE["broker"] = type(broker).__name__
    logger.info("runtime starting: mode=%s broker=%s", settings.trading_mode, _STATE["broker"])

    async def serve() -> None:
        loop = asyncio.get_running_loop()
        _STATE["started_at"] = loop.time()

        repository = await Repository.connect(settings.database_path)
        applied = await repository.migrate()
        if applied:
            logger.info("database migrations applied: %s", applied)

        server = await asyncio.start_server(_make_handler(repository), "0.0.0.0", port)
        for sig in (signal.SIGINT, signal.SIGTERM):
            try:
                loop.add_signal_handler(sig, _request_shutdown)
            except NotImplementedError:  # Windows
                signal.signal(sig, _request_shutdown)
        logger.info("http listening on 0.0.0.0:%s (/, /healthz)", port)
        try:
            async with server:
                await _SHUTDOWN.wait()
        finally:
            await repository.close()

    with contextlib.suppress(KeyboardInterrupt):
        asyncio.run(serve())
    logger.info("runtime stopped cleanly")
    return 0


if __name__ == "__main__":
    sys.exit(main())
