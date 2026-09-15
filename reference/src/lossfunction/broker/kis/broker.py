"""KIS implementation of the `Broker` interface.

Wraps `KISRestClient` operations behind the venue-agnostic interface so
strategy/risk/order code never touches KIS specifics.

Cancellation spec (order-rvsecncl, official sample): POST
/uapi/domestic-stock/v1/trading/order-rvsecncl, tr_id TTTC0013U (real) /
VTTC0013U (mock), RVSE_CNCL_DVSN_CD "02" for cancel, QTY_ALL_ORD_YN "Y"
cancels the full remainder (quantity/price are then placeholders).
"""

import httpx

from lossfunction.broker.base import (
    Broker,
    ExecutionReport,
    OrderAck,
    OrderRequest,
    Position,
    Quote,
)
from lossfunction.broker.kis.rest import AuditCallback, KISRestClient
from lossfunction.domain.types import OrderSide, OrderType

_ORD_DVSN = {OrderType.LIMIT: "00", OrderType.MARKET: "01"}


class KISBroker(Broker):
    """`Broker` implementation over the KIS Open API (live or mock domain)."""

    def __init__(
        self,
        rest: KISRestClient,
        *,
        trading_mode: str = "paper",
    ) -> None:
        self._rest = rest
        self._trading_mode = trading_mode
        # In-session cancel context: broker_order_id -> (orgno, ord_dvsn).
        # Cross-restart context is the reconciliation layer's job.
        self._order_context: dict[str, tuple[str, str]] = {}

    @property
    def trading_mode(self) -> str:
        return self._trading_mode

    async def submit_order(self, request: OrderRequest) -> OrderAck:
        ack, output = await self._rest.submit_cash_order(request)
        orgno = output.get("KRX_FWDG_ORD_ORGNO", "")
        self._order_context[ack.broker_order_id] = (
            str(orgno),
            _ORD_DVSN[request.order_type],
        )
        return ack

    async def cancel_order(self, broker_order_id: str) -> None:
        context = self._order_context.get(broker_order_id)
        if context is None:
            msg = (
                f"no in-session context for order {broker_order_id}; "
                "restart recovery must reconcile before cancelling"
            )
            raise LookupError(msg)
        orgno, ord_dvsn = context
        await self._rest.cancel_cash_order(
            broker_order_id=broker_order_id,
            krx_fwdg_ord_orgno=orgno,
            ord_dvsn=ord_dvsn,
        )

    async def get_execution_report(self, broker_order_id: str) -> ExecutionReport:
        """Terminal/current execution state via inquire-daily-ccld.

        Field mapping follows the documented response shape; any missing
        field fails loudly (MALFORMED) instead of guessing. Orders absent
        from today's response raise LookupError — the reconciliation layer
        decides what that means.
        """
        from datetime import UTC, datetime
        from decimal import Decimal

        from lossfunction.broker.kis.rest import APIErrorKind, KISAPIError

        row = await self._rest.fetch_order_ccld_row(broker_order_id)
        try:
            symbol = str(row["pdno"])
            order_quantity = int(str(row["ord_qty"]))
            filled_quantity = int(str(row.get("tot_ccld_qty") or "0"))
            side = OrderSide.BUY if str(row["sll_buy_dvsn_cd"]) == "02" else OrderSide.SELL
            cancelled = str(row.get("cncl_yn", "N")) == "Y"
            ord_dvsn = str(row.get("ord_dvsn_cd", "00"))
        except (KeyError, ValueError) as exc:
            msg = f"inquire-daily-ccld row missing/invalid fields: {row!r:.200}"
            raise KISAPIError(APIErrorKind.MALFORMED, msg) from exc

        average_fill_price = None
        if filled_quantity > 0:
            amount_raw = row.get("tot_ccld_amt")
            if amount_raw in (None, "", "0"):
                msg = f"ccld row has fills but no tot_ccld_amt: {row!r:.200}"
                raise KISAPIError(APIErrorKind.MALFORMED, msg)
            average_fill_price = Decimal(str(amount_raw)) / filled_quantity

        return ExecutionReport(
            broker_order_id=broker_order_id,
            client_order_id="",
            symbol=symbol,
            side=side,
            order_type=OrderType.LIMIT if ord_dvsn == "00" else OrderType.MARKET,
            order_quantity=order_quantity,
            filled_quantity=filled_quantity,
            average_fill_price=average_fill_price,
            open=not cancelled and filled_quantity < order_quantity,
            timestamp=datetime.now(tz=UTC),
        )

    async def get_positions(self) -> list[Position]:
        return await self._rest.fetch_positions()

    async def get_quote(self, symbol: str) -> Quote:
        return await self._rest.fetch_quote(symbol)


async def _default_sleep(seconds: float) -> None:
    import asyncio

    await asyncio.sleep(seconds)


def build_kis_broker(
    *,
    app_key: str,
    app_secret: str,
    account_number: str,
    environment: str,
    trading_mode: str = "paper",
    transport: httpx.AsyncBaseTransport | None = None,
    audit: AuditCallback | None = None,
) -> KISBroker:
    """Assemble a KISBroker from explicit parameters (factory shortcut)."""
    from lossfunction.broker.kis.auth import KISAuthClient
    from lossfunction.broker.kis.env import kis_base_url

    base_url = kis_base_url(environment)
    auth = KISAuthClient(app_key, app_secret, base_url, transport=transport, sleep=_default_sleep)
    rest = KISRestClient(
        auth=auth,
        account_number=account_number,
        environment=environment,
        base_url=base_url,
        transport=transport,
        sleep=_default_sleep,
        audit=audit,
        trading_mode=trading_mode,
    )
    return KISBroker(rest, trading_mode=trading_mode)
