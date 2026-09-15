"""ML signal schema — the contract between models and the decision layer.

Model outputs are advisory inputs: they ride on `MarketSnapshot.signals` and
strategies choose how (or whether) to use them. The schema is validated at
the boundary, so a malformed model output can never enter a decision.
"""

from pydantic import BaseModel, ConfigDict, Field


class ModelSignal(BaseModel):
    """One model's scored opinion about one symbol at one point in time."""

    model_config = ConfigDict(frozen=True)

    model_name: str
    model_version: str
    symbol: str
    as_of: str  # ISO timestamp of the last bar the features used
    probability_up: float = Field(ge=0.0, le=1.0)
    features: dict[str, str]
