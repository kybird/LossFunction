"""Foundational shared-kernel types used across domain, broker, and strategy.

Prices are `Decimal` (KIS reports KRW as integers; exact arithmetic avoids float
drift in P&L). Quantities are `int` — Korean equities trade in whole shares.
Symbols are 6-character domestic tickers (e.g. "005930").
"""

from decimal import Decimal
from enum import StrEnum
from typing import Annotated

from pydantic import StringConstraints

Symbol = Annotated[str, StringConstraints(pattern=r"^\d{6}$")]

Quantity = int

Price = Decimal


class OrderSide(StrEnum):
    BUY = "buy"
    SELL = "sell"


class OrderType(StrEnum):
    MARKET = "market"
    LIMIT = "limit"


__all__ = ["OrderSide", "OrderType", "Price", "Quantity", "Symbol"]
