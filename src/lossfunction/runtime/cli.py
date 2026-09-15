"""Container entrypoint — health endpoint and runtime skeleton.

Runs the process-level concerns of 24/7 operation:
- an HTTP health endpoint (`/healthz`) reporting mode and wiring facts,
- graceful shutdown on SIGINT/SIGTERM (so restarts are clean),
- settings load + broker assembly at startup (paper default; live refuses
  to boot without the double confirmation — that failure is a crash by
  design, which the container restart policy then surfaces).

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

logger = logging.getLogger("lossfunction.runtime")

_SHUTDOWN = asyncio.Event()


async def _handle_health(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    """Minimal HTTP/1.0 responder for GET /healthz."""
    try:
        request_line = await asyncio.wait_for(reader.readline(), timeout=5.0)
        parts = request_line.decode("latin-1").split()
        method, path = (parts + ["", ""])[:2]
        while (await reader.readline()) not in (b"\r\n", b"\n", b""):
            pass  # drain headers

        if method == "GET" and path.split("?")[0] == "/healthz":
            body = json.dumps(
                {
                    "status": "ok",
                    "service": "lossfunction-runtime",
                    "trading_mode": _STATE["trading_mode"],
                    "broker": _STATE["broker"],
                    "uptime_seconds": round(
                        asyncio.get_running_loop().time() - _STATE["started_at"]
                    ),
                }
            ).encode()
            head = (
                "HTTP/1.0 200 OK\r\n"
                "Content-Type: application/json\r\n"
                f"Content-Length: {len(body)}\r\n\r\n"
            ).encode()
            writer.write(head + body)
        else:
            writer.write(b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n")
        await writer.drain()
    except (TimeoutError, ConnectionError):
        pass
    finally:
        writer.close()


_STATE: dict[str, object] = {"trading_mode": "paper", "broker": "memory", "started_at": 0.0}


async def _idle() -> None:
    """Keep the process alive until shutdown."""
    await _SHUTDOWN.wait()


def _request_shutdown(*_: object) -> None:
    _SHUTDOWN.set()


def main(argv: list[str] | None = None) -> int:
    logging.basicConfig(level=logging.INFO)
    port = int(os.environ.get("HEALTH_PORT", "8080"))

    settings: Settings = load_settings()
    _STATE["trading_mode"] = settings.trading_mode.value
    broker: Broker = build_broker(settings)
    _STATE["broker"] = type(broker).__name__
    logger.info("runtime starting: mode=%s broker=%s", settings.trading_mode, _STATE["broker"])

    async def serve() -> None:
        loop = asyncio.get_running_loop()
        _STATE["started_at"] = loop.time()
        server = await asyncio.start_server(_handle_health, "0.0.0.0", port)
        for sig in (signal.SIGINT, signal.SIGTERM):
            try:
                loop.add_signal_handler(sig, _request_shutdown)
            except NotImplementedError:  # Windows
                signal.signal(sig, _request_shutdown)
        logger.info("health endpoint listening on 0.0.0.0:%s", port)
        async with server:
            await _idle()

    with contextlib.suppress(KeyboardInterrupt):
        asyncio.run(serve())
    logger.info("runtime stopped cleanly")
    return 0


if __name__ == "__main__":
    sys.exit(main())
