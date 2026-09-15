"""In-process metrics registry.

Counters and gauges with label-free flat names — deliberately simple,
snapshot-able to JSON, and cheap enough to call from every code path that
matters (orders, risk rejections, timeouts, reconnects, fallbacks).
"""

from typing import Any


class MetricsRegistry:
    def __init__(self) -> None:
        self._counters: dict[str, int] = {}
        self._gauges: dict[str, float] = {}

    def inc(self, name: str, amount: int = 1) -> None:
        self._counters[name] = self._counters.get(name, 0) + amount

    def gauge(self, name: str, value: float) -> None:
        self._gauges[name] = value

    def counter(self, name: str) -> int:
        return self._counters.get(name, 0)

    def snapshot(self) -> dict[str, Any]:
        return {
            "counters": dict(sorted(self._counters.items())),
            "gauges": dict(sorted(self._gauges.items())),
        }
