FROM rust:1-bookworm AS builder

RUN apt-get update \
  && apt-get install -y --no-install-recommends cmake clang build-essential pkg-config libssl-dev \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

# The server embeds the browser RandomX module, so it must exist before the
# release build starts.
RUN rustup target add wasm32-unknown-unknown \
  && cargo xtask build-wasm \
  && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates libssl3 \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/rilgate /app/rilgate
COPY --from=builder /app/LICENSE /app/LICENSE
COPY config /app/config

EXPOSE 8080
ENTRYPOINT ["/app/rilgate"]
