"""Strategy interface and the deterministic decision layer.

A strategy is a pure function of the market snapshot: same snapshot in,
same decision out — no clocks, no randomness, no I/O. The decision layer
wraps a strategy and records every decision (inputs digest + signals) so
any signal can be reproduced and audited after the fact.
"""

from abc import ABC, abstractmethod
from collections.abc import Callable
from typing import Any

from pydantic import BaseModel, ConfigDict

from lossfunction.broker.base import Quote
from lossfunction.domain.order import OrderSide, OrderType
from lossfunction.domain.portfolio import PositionState


class MarketSnapshot(BaseModel):
    """Everything a strategy is allowed to see for one decision."""

    model_config = ConfigDict(frozen=True)

    quotes: dict[str, Quote]
    positions: dict[str, PositionState]
    as_of: str  # opaque session label; ordering handled upstream


class OrderIntent(BaseModel):
    """A desired trade, before risk checks and id assignment."""

    model_config = ConfigDict(frozen=True)

    symbol: str
    side: OrderSide
    order_type: OrderType
    quantity: int
    limit_price: str | None = None


class StrategyDecision(BaseModel):
    """The output of one strategy invocation, fully self-describing."""

    model_config = ConfigDict(frozen=True)

    strategy_name: str
    strategy_version: str
    intents: tuple[OrderIntent, ...]
    features: dict[str, Any]  # the exact inputs the signals derive from
    rationale: str = ""


DecisionCallback = Callable[[StrategyDecision, MarketSnapshot], None]


class Strategy(ABC):
    """Deterministic signal generator."""

    name: str = "unnamed"
    version: str = "0"

    @abstractmethod
    def decide(self, snapshot: MarketSnapshot) -> StrategyDecision:
        raise NotImplementedError


class DecisionLayer:
    """Runs a strategy and records every decision for audit/replay."""

    def __init__(
        self,
        strategy: Strategy,
        on_decision: DecisionCallback | None = None,
    ) -> None:
        self._strategy = strategy
        self._on_decision = on_decision
        self._decisions: list[StrategyDecision] = []

    @property
    def strategy_name(self) -> str:
        return self._strategy.name

    def decide(self, snapshot: MarketSnapshot) -> StrategyDecision:
        decision = self._strategy.decide(snapshot)
        self._decisions.append(decision)
        if self._on_decision is not None:
            self._on_decision(decision, snapshot)
        return decision

    def decisions(self) -> list[StrategyDecision]:
        return list(self._decisions)
