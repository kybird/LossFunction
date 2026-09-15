"""Runtime entrypoint tests — health endpoint in a real subprocess."""

import json
import os
import socket
import subprocess
import sys
import time
import urllib.request

import pytest

PYTHON = os.environ.get("LOSSFUNCTION_TEST_PYTHON", sys.executable)


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _wait_for_health(port: int, timeout: float = 15.0) -> dict:
    deadline = time.time() + timeout
    last_error: Exception | None = None
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/healthz", timeout=2) as response:
                assert response.status == 200
                return json.loads(response.read().decode())
        except Exception as exc:  # noqa: BLE001 - startup polling
            last_error = exc
            time.sleep(0.2)
    pytest.fail(f"health endpoint never came up: {last_error}")


def test_container_entrypoint_serves_paper_health() -> None:
    port = _free_port()
    env = {**os.environ, "HEALTH_PORT": str(port), "TRADING_MODE": "paper"}
    env.pop("LIVE_TRADING_CONFIRMED", None)
    process = subprocess.Popen(
        [PYTHON, "-m", "lossfunction.runtime.cli"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        payload = _wait_for_health(port)
        assert payload["status"] == "ok"
        assert payload["trading_mode"] == "paper"
        assert payload["broker"] == "MockBroker"
        assert payload["uptime_seconds"] >= 0

        # Unknown paths are a plain 404.
        with pytest.raises(urllib.error.HTTPError) as excinfo:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/nope", timeout=2)
        assert excinfo.value.code == 404
    finally:
        process.terminate()
        process.wait(timeout=10)


def test_live_without_confirmation_refuses_to_boot() -> None:
    port = _free_port()
    env = {
        **os.environ,
        "HEALTH_PORT": str(port),
        "TRADING_MODE": "live",
    }  # no LIVE_TRADING_CONFIRMED → settings validator must refuse
    process = subprocess.run(
        [PYTHON, "-m", "lossfunction.runtime.cli"],
        env=env,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert process.returncode != 0
    assert "live_trading_confirmed" in process.stderr
