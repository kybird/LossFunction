"""Configuration loading and validation.

Non-secret settings come from environment variables or a `.env` settings file.
Secrets (KIS credentials) are read from the environment only and are represented
as `SecretStr` so they never leak into logs or repr.
"""

from lossfunction.config.settings import Settings, TradingMode, load_settings

__all__ = ["Settings", "TradingMode", "load_settings"]
