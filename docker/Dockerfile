FROM rust:1.89-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Pre-build dependency layer for better Docker layer caching.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN mkdir -p /app/examples \
    && touch /app/examples/.gitkeep
# Touch a placeholder so the first build succeeds without a real
# copy of the examples directory (otherwise cargo treats it as
# missing and the COPY examples below re-invalidates the cache).
RUN cargo build --release -p rspark-cli --bin rspark-cli || true

# Build the real binaries.
COPY examples ./examples
RUN cargo build --release -p rspark-cli --bin rspark-cli && \
    cargo build --release -p rspark-operator --bin rspark-operator

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/rspark-cli      /usr/local/bin/rspark-cli
COPY --from=builder /app/target/release/rspark-operator /usr/local/bin/rspark-operator
COPY examples /app/examples
ENV RSPARK_LOG=info
# Default entrypoint is the CLI; the operator Deployment overrides
# this with `/usr/local/bin/rspark-operator`.
ENTRYPOINT ["/usr/local/bin/rspark-cli"]
CMD ["--help"]