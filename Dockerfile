# syntax=docker/dockerfile:1
FROM rust:slim-bookworm AS builder
WORKDIR /usr/src/app

# Install build dependencies for openssl/sqlite
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Use BuildKit cache mounts to prevent re-downloading and re-compiling crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/app/target \
    cargo build --release && \
    cp ./target/release/agent-book-translate /tmp/agent-book-translate

# Runtime Stage
FROM debian:bookworm-slim

# Install SSL certificates and SQLite runtime dependency
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends \
    ca-certificates \
    libsqlite3-0 \
    openssl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/agent-book-translate /usr/local/bin/agent-book-translate

# Set default working directory for ebook inputs/outputs
WORKDIR /workspace

ENTRYPOINT ["agent-book-translate"]
