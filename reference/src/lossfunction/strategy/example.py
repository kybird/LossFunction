"""Example deterministic strategy — entry-price starter.

Buys a fixed quantity the first time a symbol trades at or below its
configured entry price (no position yet). Pure function of the snapshot:
no randomness, no clocks, no state outside the snapshot.
"""

from lossfunction.domain.order import OrderSide, OrderType
from lossfunction.strategy.base import (
    MarketSnapshot,
    OrderIntent,
    Strategy,
    StrategyDecision,
)


class EntryPriceStrategy(Strategy):
    name = "entry-price"
    version = "1"

    def __init__(self, entry_prices: dict[str, str], quantity: int = 10) -> None:
        from decimal import Decimal

        self._entry_prices = {symbol: Decimal(p) for symbol, p in entry_prices.items()}
        self._quantity = quantity

    def decide(self, snapshot: MarketSnapshot) -> StrategyDecision:
        intents: list[OrderIntent] = []
        features: dict[str, str] = {}
        for symbol, entry in sorted(self._entry_prices.items()):
            quote = snapshot.quotes.get(symbol)
            if quote is None:
                features[symbol] = "no-quote"
                continue
            features[f"{symbol}:price"] = str(quote.last_price)
            features[f"{symbol}:entry"] = str(entry)
            held = snapshot.positions.get(symbol)
            held_qty = held.quantity if held is not None else 0
            features[f"{symbol}:held"] = str(held_qty)
            if held_qty == 0 and quote.last_price <= entry:
                intents.append(
                    OrderIntent(
                        symbol=symbol,
                        side=OrderSide.BUY,
                        order_type=OrderType.LIMIT,
                        quantity=self._quantity,
                        limit_price=str(quote.last_price),
                    )
                )
        rationale = (
            "buy symbols at/below entry with no existing position"
            if intents
            else "no entry signals"
        )
        return StrategyDecision(
            strategy_name=self.name,
            strategy_version=self.version,
            intents=tuple(intents),
            features=features,
            rationale=rationale,
        )
