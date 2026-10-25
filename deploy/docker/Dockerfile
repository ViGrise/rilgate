# Runtime base images. Each runtime stage builds from these values and records them as OCI labels.
ARG DEBIAN_RUNTIME_IMAGE=docker.io/library/debian:trixie-slim
ARG DEBIAN_RUNTIME_DIGEST=sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
ARG ALPINE_RUNTIME_IMAGE=docker.io/library/alpine:3.22
ARG ALPINE_RUNTIME_DIGEST=sha256:2e0b81d60597f449baa56dc25d355367869d2dca4cd57e866f1e2b52af3eb55f

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

# Browser solvers and generated web resources, embedded by both release builds.
FROM chef AS assets
COPY .cargo .cargo
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --locked --package xtask --recipe-path recipe.json
COPY . .
RUN cargo xtask build-wasm \
  && cargo xtask build-web

# Dependency layers depend only on the recipe, so they stay cached until manifests or Cargo.lock change.
FROM chef AS builder
COPY .cargo .cargo
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --package rilgate --recipe-path recipe.json
COPY . .
COPY --from=assets /app/crates/rilgate-web/generated crates/rilgate-web/generated
COPY --from=assets /app/web/static/js/solvers web/static/js/solvers
RUN cargo build --release --locked \
  && cp target/release/rilgate /app/rilgate-release

# A musl target links statically, so this binary runs on any Linux distribution of its architecture.
FROM rust:1-alpine@sha256:b1d5dd5a22a09404d3e798c0cf051b689077360374eb505fa5bb93c479693d87 AS musl-chef

ARG CARGO_BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS

RUN apk add --no-cache gcc musl-dev linux-headers make cmake perl

WORKDIR /app
COPY rust-toolchain.toml ./
RUN cargo install cargo-chef --version 0.1.78 --locked

FROM musl-chef AS musl-builder
COPY .cargo .cargo
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --package rilgate --recipe-path recipe.json
COPY . .
COPY --from=assets /app/crates/rilgate-web/generated crates/rilgate-web/generated
COPY --from=assets /app/web/static/js/solvers web/static/js/solvers
RUN cargo build --release --locked -p rilgate \
  && cp target/release/rilgate /app/rilgate-release

# Release binaries alone, for packaging with
# `docker buildx build --target binary --output type=local,dest=<dir>`
# (dynamic, glibc) or `--target binary-musl` (static).
FROM scratch AS binary
COPY --from=builder /app/rilgate-release /rilgate

FROM scratch AS binary-musl
COPY --from=musl-builder /app/rilgate-release /rilgate

# Debian image with the dynamic glibc binary, published with the -debian tag suffix.
# The builder uses bookworm, so this binary needs glibc 2.36 or newer.
FROM ${DEBIAN_RUNTIME_IMAGE}@${DEBIAN_RUNTIME_DIGEST} AS runtime-debian
ARG DEBIAN_RUNTIME_IMAGE
ARG DEBIAN_RUNTIME_DIGEST
LABEL org.opencontainers.image.base.name="${DEBIAN_RUNTIME_IMAGE}" \
  org.opencontainers.image.base.digest="${DEBIAN_RUNTIME_DIGEST}"
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

# Default image with the static musl binary, published with the unsuffixed and latest tags.
# It is the last stage, so a build without --target produces it.
FROM ${ALPINE_RUNTIME_IMAGE}@${ALPINE_RUNTIME_DIGEST} AS runtime-alpine
ARG ALPINE_RUNTIME_IMAGE
ARG ALPINE_RUNTIME_DIGEST
LABEL org.opencontainers.image.base.name="${ALPINE_RUNTIME_IMAGE}" \
  org.opencontainers.image.base.digest="${ALPINE_RUNTIME_DIGEST}"
RUN apk upgrade --no-cache \
  && apk add --no-cache ca-certificates curl

WORKDIR /app
COPY --from=musl-builder /app/rilgate-release /app/rilgate
COPY --from=musl-builder /app/LICENSE /app/LICENSE
COPY config/rilgate.yml /app/config/rilgate.yml

USER 10001:10001
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl --fail --silent --max-time 4 http://127.0.0.1:9090/.well-known/rilgate/api/ready || exit 1

EXPOSE 8080 9090
ENTRYPOINT ["/app/rilgate"]
CMD ["--config", "/app/config/rilgate.yml"]
