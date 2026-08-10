# Cloud Agent base image for bookclerk (Rust workspace).
# Do not COPY the repo — Cursor checks out the correct commit into the VM.
# Pin to the same Rust minor as packaging/docker/Dockerfile.
FROM rust:1.85-bookworm

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

# Match rust-toolchain.toml components: rustfmt + clippy
RUN rustup component add rustfmt clippy

WORKDIR /workspace

# Keep rustup/cargo on PATH even when CARGO_HOME is remapped (Cursor agent
# shells sometimes start with a minimal PATH that omits /usr/local/cargo/bin).
# Workspace-local Cargo cache + Bookclerk data (bind-mounted checkout).
ENV PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    CARGO_HOME=/workspace/.cargo-home \
    CARGO_TARGET_DIR=/workspace/target \
    TMPDIR=/workspace/.tmp \
    BOOKCLERK_FILES_DIR=/workspace/BookclerkFiles

