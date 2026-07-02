FROM rust:1.86-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Pre-build dependency layer for better Docker layer caching.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN mkdir -p /app/examples \
    && touch /app/examples/.gitkeep
RUN cargo build --release -p rspark-cli --bin rspark-cli || true

# Build the actual binary on top of the cached deps.
COPY examples ./examples
RUN cargo build --release -p rspark-cli --bin rspark-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/rspark-cli /usr/local/bin/rspark-cli
COPY examples /app/examples
ENV RSPARK_LOG=info
# Container is a multi-purpose image; the orchestrator (k3s Deployment,
# docker compose, etc.) provides the subcommand via args.
ENTRYPOINT ["/usr/local/bin/rspark-cli"]
CMD ["--help"]