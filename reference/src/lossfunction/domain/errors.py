"""Domain error hierarchy — no infrastructure types allowed here."""


class DomainError(Exception):
    """Base class for business-rule violations."""


class InvalidOrderError(DomainError):
    """An order creation/amendment/cancellation violated a domain rule."""


class InsufficientPositionError(DomainError):
    """A sell exceeded the quantity held."""
