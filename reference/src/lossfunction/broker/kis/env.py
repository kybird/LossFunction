"""KIS environment and domain mapping."""

from enum import StrEnum

KIS_BASE_URLS: dict[str, str] = {
    "real": "https://openapi.koreainvestment.com:9443",
    "mock": "https://openapivts.koreainvestment.com:9443",
}


class KISEnvironment(StrEnum):
    REAL = "real"
    MOCK = "mock"


def kis_base_url(environment: str) -> str:
    """Return the REST base URL for a KIS environment."""
    try:
        return KIS_BASE_URLS[environment]
    except KeyError:
        known = ", ".join(sorted(KIS_BASE_URLS))
        msg = f"unknown KIS environment {environment!r}; expected one of: {known}"
        raise ValueError(msg) from None
