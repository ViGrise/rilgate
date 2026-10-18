FROM rust:1-bookworm@sha256:e12566d5abc17a0da741628782cdb504b68b8961f5bfcf677936bf3e8b32de29 AS builder

ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS

RUN apt-get update \
  && apt-get install -y --no-install-recommends cmake clang build-essential pkg-config libssl-dev \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY rust-toolchain.toml ./
RUN rustup target add wasm32-unknown-unknown
COPY . .

# Cache mounts are build-only; the runtime binary must also exist in the image layer.
RUN --mount=type=cache,id=rilgate-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=rilgate-target,target=/app/target,sharing=locked \
    cargo xtask build-wasm \
    && cargo build --release --locked \
    && cp target/release/rilgate /app/rilgate-release

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
RUN apt-get update \
  && apt-get upgrade -y \
  && apt-get install -y --no-install-recommends ca-certificates libssl3 curl \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/rilgate-release /app/rilgate
COPY --from=builder /app/LICENSE /app/LICENSE
COPY config/rilgate.production.yml /app/config/rilgate.yml

ENV ENV=production SERVE_HOST=0.0.0.0
USER 10001:10001
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl --fail --silent --max-time 4 http://127.0.0.1:8080/.well-known/rilgate/api/ready || exit 1

EXPOSE 8080
ENTRYPOINT ["/app/rilgate"]
CMD ["--config", "/app/config/rilgate.yml"]
