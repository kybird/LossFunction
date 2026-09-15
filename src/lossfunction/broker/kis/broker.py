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
from lossfunction.domain.types import OrderType

_ORD_DVSN = {OrderType.LIMIT: "00", OrderType.MARKET: "01"}


class PendingReconciliationError(NotImplementedError):
    """Raised by operations that require the reconciliation layer.

    The broker keeps only in-session order context; cross-restart order
    state and terminal-state lookup (inquire-daily-ccld) belong to the
    reconciliation layer built on top of this broker.
    """


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
        msg = (
            "terminal-state lookup (inquire-daily-ccld) is provided by the "
            "reconciliation layer; this broker reports only via open-order "
            "queries once that lands"
        )
        raise PendingReconciliationError(msg)

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
