# Multi-stage Rust build: static-ish release binary in a distroless runtime.
FROM rust:1-slim AS builder
WORKDIR /build
# Dependency layer first for build-cache reuse.
COPY Cargo.toml Cargo.lock ./
COPY lossfunction/Cargo.toml lossfunction/Cargo.toml
RUN mkdir -p lossfunction/src && echo "" > lossfunction/src/lib.rs \
    && echo "fn main() {}" > lossfunction/src/main.rs \
    && cargo build --release --manifest-path lossfunction/Cargo.toml \
    && rm -rf lossfunction/src
COPY lossfunction/src lossfunction/src
RUN touch lossfunction/src/lib.rs lossfunction/src/main.rs \
    && cargo build --release --manifest-path lossfunction/Cargo.toml

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=builder /build/target/release/lossfunction /usr/local/bin/lossfunction
# Paper by default; live requires the settings double confirmation.
ENV TRADING_MODE=paper \
    DATABASE_PATH=/data/lossfunction.db \
    HEALTH_PORT=8080
VOLUME ["/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/usr/local/bin/lossfunction", "--healthcheck"]
USER nonroot
ENTRYPOINT ["/usr/local/bin/lossfunction"]
