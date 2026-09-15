"""Configuration loading, validation, and secret-separation tests."""

import subprocess
from pathlib import Path

import pytest
from pydantic import ValidationError

from lossfunction.config import Settings, TradingMode, load_settings


def test_default_mode_is_paper(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("TRADING_MODE", raising=False)
    monkeypatch.delenv("LIVE_TRADING_CONFIRMED", raising=False)
    assert load_settings().trading_mode is TradingMode.PAPER


def test_env_vars_are_loaded_and_validated(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("TRADING_MODE", "paper")
    monkeypatch.setenv("DATABASE_PATH", "data/custom.db")
    settings = load_settings()
    assert settings.trading_mode is TradingMode.PAPER
    assert settings.database_path == "data/custom.db"


def test_invalid_mode_is_rejected(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("TRADING_MODE", "moon")
    with pytest.raises(ValidationError):
        load_settings()


def test_live_mode_requires_confirmation(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("TRADING_MODE", "live")
    monkeypatch.delenv("LIVE_TRADING_CONFIRMED", raising=False)
    with pytest.raises(ValidationError, match="live_trading_confirmed"):
        load_settings()


def test_confirmed_live_mode_is_allowed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("TRADING_MODE", "live")
    monkeypatch.setenv("LIVE_TRADING_CONFIRMED", "true")
    monkeypatch.setenv("KIS_ENVIRONMENT", "real")
    assert load_settings().trading_mode is TradingMode.LIVE


def test_kis_environment_defaults_to_mock(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("KIS_ENVIRONMENT", raising=False)
    assert load_settings().kis_environment == "mock"


def test_live_mode_requires_real_kis_environment(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("TRADING_MODE", "live")
    monkeypatch.setenv("LIVE_TRADING_CONFIRMED", "true")
    monkeypatch.setenv("KIS_ENVIRONMENT", "mock")
    with pytest.raises(ValidationError, match="kis_environment=real"):
        load_settings()


def test_dotenv_settings_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    for var in ("TRADING_MODE", "DATABASE_PATH", "LIVE_TRADING_CONFIRMED"):
        monkeypatch.delenv(var, raising=False)
    env_file = tmp_path / ".env"
    env_file.write_text("DATABASE_PATH=data/from-file.db\n", encoding="utf-8")
    settings = Settings(_env_file=env_file)  # type: ignore[call-arg]
    assert settings.database_path == "data/from-file.db"


def test_secrets_are_masked_in_repr(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("KIS_APP_KEY", "very-secret-key")
    settings = load_settings()
    assert "very-secret-key" not in repr(settings)
    assert "very-secret-key" not in str(settings.model_dump())


def test_env_files_are_gitignored() -> None:
    """`.env` files must be blocked by the repo root .gitignore (archive lives
    one level down, so run git from the repository root)."""
    root = Path(__file__).resolve().parents[2]
    result = subprocess.run(
        ["git", "-C", str(root), "check-ignore", ".env", ".env.local", ".env.production"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, f"not all .env variants ignored: {result.stderr}"
    ignore_rules = (root / ".gitignore").read_text(encoding="utf-8")
    assert ".env" in ignore_rules and ".env.*" in ignore_rules
