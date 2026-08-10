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

# Cursor agent shells often start with a minimal PATH that omits
# /usr/local/cargo/bin (even when Dockerfile ENV PATH includes it). Symlink the
# toolchain into /usr/local/bin so `cargo`/`rustc` remain usable without a
# manual PATH export.
RUN for b in cargo rustc rustup rustfmt cargo-clippy clippy-driver cargo-fmt; do \
      ln -sf "/usr/local/cargo/bin/$b" "/usr/local/bin/$b"; \
    done

WORKDIR /workspace

# Keep rustup/cargo on PATH for processes that inherit the image env.
# Workspace-local Cargo cache + Bookclerk data (bind-mounted checkout).
ENV PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    CARGO_HOME=/workspace/.cargo-home \
    CARGO_TARGET_DIR=/workspace/target \
    TMPDIR=/workspace/.tmp \
    BOOKCLERK_FILES_DIR=/workspace/BookclerkFiles

