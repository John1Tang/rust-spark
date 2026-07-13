FROM rust:1.97-slim AS builder
WORKDIR /app

# Force apt + cargo to ignore any inherited proxy env. Some corporate
# networks set http_proxy=https://... which Docker Desktop forwards
# into the build, but the host already has VPN tunnel access and that
# proxy is unreachable from inside the build sandbox.
RUN echo 'Acquire::http::Proxy "false";' > /etc/apt/apt.conf.d/00no-proxy \
 && echo 'Acquire::https::Proxy "false";' >> /etc/apt/apt.conf.d/00no-proxy \
 && mkdir -p /root/.cargo \
 && printf '[http]\n  proxy = ""\n[https]\n  proxy = ""\n' > /root/.cargo/config.toml

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates librdkafka-dev \
    && rm -rf /var/lib/apt/lists/*

# Pre-build dependency layer for better Docker layer caching.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN mkdir -p /app/examples \
    && touch /app/examples/.gitkeep
# Populate the registry & target dir caches before we touch the source
# tree below. `cargo fetch` only resolves deps; it does not compile.
# This layer survives as long as Cargo.toml/Cargo.lock are unchanged,
# so the next build won't re-download the whole world.
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    cargo fetch
# Touch a placeholder so the first build succeeds without a real
# copy of the examples directory (otherwise cargo treats it as
# missing and the COPY examples below re-invalidates the cache).
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p rspark-cli --bin rspark-cli || true

# Build the real binaries. The cache mounts read from the populated
# registry/target dirs, but the FINAL step writes into the layer's
# normal filesystem so the `COPY --from=builder` below can find them.
COPY examples ./examples
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p rspark-cli --bin rspark-cli && \
    cargo build --release -p rspark-operator --bin rspark-operator && \
    cp /app/target/release/rspark-cli      /app/rspark-cli && \
    cp /app/target/release/rspark-operator /app/rspark-operator

FROM debian:trixie-slim
RUN echo 'Acquire::http::Proxy "false";' > /etc/apt/apt.conf.d/00no-proxy \
 && echo 'Acquire::https::Proxy "false";' >> /etc/apt/apt.conf.d/00no-proxy \
 && apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl librdkafka1 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/rspark-cli      /usr/local/bin/rspark-cli
COPY --from=builder /app/rspark-operator /usr/local/bin/rspark-operator
COPY examples /app/examples
ENV RSPARK_LOG=info
# Default entrypoint is the CLI; the operator Deployment overrides
# this with `/usr/local/bin/rspark-operator`.
ENTRYPOINT ["/usr/local/bin/rspark-cli"]
CMD ["--help"]
