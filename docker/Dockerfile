FROM rust:1.82-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY examples ./examples

RUN cargo build --release -p rspark-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/rspark-cli /usr/local/bin/rspark-cli
COPY examples /app/examples
ENV RSPARK_LOG=info
ENTRYPOINT ["/usr/local/bin/rspark-cli"]
CMD ["--help"]