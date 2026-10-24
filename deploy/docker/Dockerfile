FROM rust:1-bookworm@sha256:e12566d5abc17a0da741628782cdb504b68b8961f5bfcf677936bf3e8b32de29 AS chef

ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS

RUN apt-get update \
  && apt-get install -y --no-install-recommends cmake clang build-essential pkg-config libssl-dev \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY rust-toolchain.toml ./
RUN rustup target add wasm32-unknown-unknown wasm32v1-none \
  && cargo install cargo-chef --version 0.1.78 --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Dependency layers depend only on the recipe, so they stay cached until manifests or Cargo.lock change.
FROM chef AS builder
COPY .cargo .cargo
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --package rilgate --recipe-path recipe.json \
  && cargo chef cook --locked --package xtask --recipe-path recipe.json
COPY . .
RUN cargo xtask build-wasm \
  && cargo xtask build-web \
  && cargo build --release --locked \
  && cp target/release/rilgate /app/rilgate-release

# The release binary alone, for packaging with
# `docker buildx build --target binary --output type=local,dest=<dir>`.
FROM scratch AS binary
COPY --from=builder /app/rilgate-release /rilgate

# The builder uses bookworm, so released binaries need glibc 2.36. The runtime uses the current Debian stable release.
FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad AS runtime
RUN apt-get update \
  && apt-get upgrade -y \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/rilgate-release /app/rilgate
COPY --from=builder /app/LICENSE /app/LICENSE
COPY config/rilgate.production.yml /app/config/rilgate.yml

USER 10001:10001
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl --fail --silent --max-time 4 http://127.0.0.1:9090/.well-known/rilgate/api/ready || exit 1

EXPOSE 8080 9090
ENTRYPOINT ["/app/rilgate"]
CMD ["--config", "/app/config/rilgate.yml"]
