"""Order state machine tests — table coverage, UNKNOWN path, audit events."""

import pytest

from lossfunction.domain.order import OrderStatus
from lossfunction.execution import (
    TRANSITIONS,
    IllegalTransitionError,
    OrderStateMachine,
    OrderTransitionEvent,
)

ALL = set(OrderStatus)


def test_transition_table_covers_every_status() -> None:
    assert set(TRANSITIONS) == ALL


def test_terminal_statuses_accept_nothing() -> None:
    for status in (OrderStatus.FILLED, OrderStatus.CANCELLED, OrderStatus.REJECTED):
        assert TRANSITIONS[status] == frozenset()


def test_all_legal_transitions_execute() -> None:
    machine = OrderStateMachine()
    for from_status, targets in TRANSITIONS.items():
        for to_status in targets:
            machine = OrderStateMachine()
            machine.register("o-1", from_status)
            assert machine.transition("o-1", to_status, reason="test") == to_status


def test_every_illegal_transition_rejected() -> None:
    """Exhaustive: anything not in the table must raise."""
    for from_status, targets in TRANSITIONS.items():
        legal = set(targets)
        for to_status in ALL - legal - {from_status}:
            machine = OrderStateMachine()
            machine.register("o-1", from_status)
            with pytest.raises(IllegalTransitionError, match="illegal transition"):
                machine.transition("o-1", to_status, reason="test")


def test_partial_fill_self_transition_allowed() -> None:
    machine = OrderStateMachine()
    machine.register("o-1", OrderStatus.PARTIALLY_FILLED)
    machine.transition("o-1", OrderStatus.PARTIALLY_FILLED, reason="more fills")
    machine.transition("o-1", OrderStatus.FILLED, reason="final fill")


def test_unknown_entry_paths() -> None:
    for source in (OrderStatus.PENDING, OrderStatus.SUBMITTED):
        machine = OrderStateMachine()
        machine.register("o-1", source)
        machine.mark_unknown("o-1", reason="submit timeout")
        assert machine.status("o-1") is OrderStatus.UNKNOWN


def test_unknown_entry_rejected_from_other_statuses() -> None:
    for source in (
        OrderStatus.PARTIALLY_FILLED,
        OrderStatus.FILLED,
        OrderStatus.CANCELLED,
        OrderStatus.REJECTED,
        OrderStatus.UNKNOWN,
    ):
        machine = OrderStateMachine()
        machine.register("o-1", source)
        with pytest.raises(IllegalTransitionError, match="outstanding submission"):
            machine.mark_unknown("o-1", reason="late timeout")


def test_unknown_resolves_via_reconciliation() -> None:
    machine = OrderStateMachine()
    machine.register("o-1", OrderStatus.PENDING)
    machine.mark_unknown("o-1", reason="timeout")
    for resolved in (OrderStatus.FILLED, OrderStatus.CANCELLED, OrderStatus.SUBMITTED):
        machine = OrderStateMachine()
        machine.register("o-1", OrderStatus.PENDING)
        machine.mark_unknown("o-1", reason="timeout")
        machine.transition("o-1", resolved, reason="broker reconciliation")


def test_every_transition_emits_exactly_one_audit_event() -> None:
    events: list[OrderTransitionEvent] = []
    machine = OrderStateMachine(on_event=events.append)
    machine.register("o-1", OrderStatus.PENDING)
    machine.transition("o-1", OrderStatus.SUBMITTED, reason="broker ack")
    machine.transition("o-1", OrderStatus.PARTIALLY_FILLED, reason="fill 10/20")
    machine.transition("o-1", OrderStatus.FILLED, reason="fill 20/20")

    assert [(e.from_status, e.to_status) for e in events] == [
        (OrderStatus.PENDING, OrderStatus.SUBMITTED),
        (OrderStatus.SUBMITTED, OrderStatus.PARTIALLY_FILLED),
        (OrderStatus.PARTIALLY_FILLED, OrderStatus.FILLED),
    ]
    assert all(e.client_order_id == "o-1" for e in events)
    assert all(e.occurred_at.tzinfo is not None for e in events)
    assert [e.reason for e in events] == ["broker ack", "fill 10/20", "fill 20/20"]
    assert machine.history("o-1") == events


def test_illegal_transition_emits_nothing() -> None:
    events: list[OrderTransitionEvent] = []
    machine = OrderStateMachine(on_event=events.append)
    machine.register("o-1", OrderStatus.FILLED)
    with pytest.raises(IllegalTransitionError):
        machine.transition("o-1", OrderStatus.SUBMITTED, reason="late ack")
    assert events == []
    assert machine.status("o-1") is OrderStatus.FILLED


def test_unknown_order_must_be_registered_first() -> None:
    machine = OrderStateMachine()
    with pytest.raises(IllegalTransitionError, match="register"):
        machine.transition("o-404", OrderStatus.SUBMITTED, reason="?")


def test_duplicate_registration_rejected() -> None:
    machine = OrderStateMachine()
    machine.register("o-1", OrderStatus.PENDING)
    with pytest.raises(IllegalTransitionError, match="already registered"):
        machine.register("o-1", OrderStatus.PENDING)
