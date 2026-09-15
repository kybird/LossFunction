FROM python:3.13-slim

WORKDIR /app

# Install the package (dependencies pulled from PyPI at build time).
COPY pyproject.toml README.md ./
COPY src ./src
RUN pip install --no-cache-dir .

# Paper by default; live requires TRADING_MODE=live + LIVE_TRADING_CONFIRMED=true
# + KIS_ENVIRONMENT=real and will refuse to boot otherwise (settings validator).
ENV TRADING_MODE=paper \
    HEALTH_PORT=8080

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8080/healthz', timeout=3)"

CMD ["python", "-m", "lossfunction.runtime.cli"]
