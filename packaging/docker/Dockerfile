# Multi-stage Dockerfile for libation CLI + daemon.
FROM rust:1.85-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release -p libation-cli -p libationd \
    && strip target/release/libation target/release/libationd

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /var/lib/libation --create-home --shell /usr/sbin/nologin libation
COPY --from=builder /src/target/release/libation /usr/local/bin/libation
COPY --from=builder /src/target/release/libationd /usr/local/bin/libationd
COPY config/config.example.toml /etc/libation/config.example.toml
ENV LIBATION_FILES_DIR=/config \
    LIBATION_CONFIG=/config/config.toml \
    LIBATION_DAEMON_LISTEN=0.0.0.0:8787
VOLUME ["/config", "/data"]
EXPOSE 8787
USER libation
WORKDIR /config
ENTRYPOINT ["/usr/local/bin/libationd"]
