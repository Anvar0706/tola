# syntax=docker/dockerfile:1
# ------------------------------------------------------------------------------
# Build Stage: Compile high-performance release binaries
# ------------------------------------------------------------------------------
FROM rust:1-bullseye AS builder

WORKDIR /usr/src/tola

# Copy root manifest and crate definitions
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates

# Build release binaries for node daemon and operational CLI
RUN cargo build --release --bin tola-node --bin tola-cli

# ------------------------------------------------------------------------------
# Runtime Stage: Minimal, hardened, unprivileged production container
# ------------------------------------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create dedicated non-root system user and persistent directories
RUN useradd -m -u 1000 -U -s /bin/sh tola && \
    mkdir -p /data && \
    chown -R tola:tola /data

# Copy artifacts from builder
COPY --from=builder /usr/src/tola/target/release/tola-node /usr/local/bin/tola-node
COPY --from=builder /usr/src/tola/target/release/tola-cli /usr/local/bin/tola-cli

USER tola
WORKDIR /home/tola

VOLUME ["/data"]

# P2P wire port (30303) and JSON-RPC / Prometheus HTTP port (8545)
EXPOSE 30303 8545

# Healthcheck monitoring the HTTP /health endpoint
HEALTHCHECK --interval=5s --timeout=3s --start-period=3s --retries=5 \
    CMD curl -f http://127.0.0.1:8545/health || exit 1

ENTRYPOINT ["/usr/local/bin/tola-node"]
CMD ["--chain-id", "102", "--data-dir", "/data", "--p2p-addr", "0.0.0.0:30303", "--rpc-addr", "0.0.0.0:8545"]
