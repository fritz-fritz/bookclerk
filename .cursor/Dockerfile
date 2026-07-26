# Cloud Agent base image for bookclerk (Rust workspace).
# Do not COPY the repo — Cursor checks out the correct commit into the VM.
FROM rust:bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        jq \
        pkg-config \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Match rust-toolchain.toml: stable + rustfmt + clippy
RUN rustup component add rustfmt clippy

WORKDIR /workspace
