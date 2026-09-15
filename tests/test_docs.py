"""Public documentation tests — completeness and secret hygiene."""

import re
from pathlib import Path

ROOT = Path(__file__).parent.parent
README = (ROOT / "README.md").read_text(encoding="utf-8")

DOCUMENTED_FILES = [
    ROOT / "README.md",
    ROOT / "docs" / "architecture.md",
    ROOT / "docs" / "recovery.md",
    ROOT / "docs" / "deployment.md",
]


# ── AC1: install / configure / run / paper guides present ──────────


def test_readme_covers_setup_configuration_running_and_paper() -> None:
    for section_marker in (
        "빠른 시작",
        "pip install -e",
        ".env.example",
        "설정",
        "paper trading",
        "lossfunction.runtime.cli",
    ):
        assert section_marker in README, f"README missing: {section_marker}"


def test_readme_links_core_documents() -> None:
    for doc in ("docs/architecture.md", "docs/recovery.md", "docs/deployment.md"):
        assert doc in README


# ── AC2: clone-to-tests path is documented and truthful ────────────


def test_readme_clone_to_test_commands() -> None:
    for command in (
        "python -m venv .venv",
        'pip install -e ".[dev]"',
        "pytest",
        "ruff check",
    ):
        assert command in README


def test_example_env_has_no_filled_secrets() -> None:
    example = (ROOT / ".env.example").read_text(encoding="utf-8")
    for line in example.splitlines():
        if "=" in line and not line.strip().startswith("#"):
            key, _, value = line.partition("=")
            if key.upper().startswith(("KIS_APP", "KIS_ACCOUNT", "ALERT_WEBHOOK", "GLM")):
                assert value.strip() == "", f".env.example has a filled secret: {key}"


# ── AC3: no secrets or personal data in tracked docs ───────────────


def test_documents_contain_no_secret_like_material() -> None:
    suspicious = re.compile(
        r"[A-Za-z0-9+/]{40,}={0,2}"  # long base64/hex blobs (keys, tokens)
        r"|[0-9a-f]{32,}"  # hex digests beyond example sha lengths
    )
    for path in DOCUMENTED_FILES:
        text = path.read_text(encoding="utf-8")
        for match in suspicious.finditer(text):
            context = text[max(0, match.start() - 20) : match.end() + 10]
            # Allow documented placeholder shapes only.
            assert "<owner>" in context or "sha256" in context.lower(), (
                f"suspicious token in {path.name}: ...{context}..."
            )


def test_documents_contain_no_personal_identifiers() -> None:
    email = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
    for path in DOCUMENTED_FILES:
        text = path.read_text(encoding="utf-8")
        assert not email.search(text), f"email address in {path.name}"
