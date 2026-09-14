"""Smoke tests for the package scaffold."""

import lossfunction


def test_package_importable() -> None:
    assert lossfunction.__doc__ is not None


def test_version_is_semver() -> None:
    parts = lossfunction.__version__.split(".")
    assert len(parts) == 3
    assert all(p.isdigit() for p in parts)
