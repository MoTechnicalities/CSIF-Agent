# syntax=docker/dockerfile:1.7

FROM rust:1.80-alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY grammar.toml ./
COPY crates ./crates
COPY apps ./apps

ARG TARGETARCH
RUN case "${TARGETARCH}" in \
      amd64) echo "x86_64-unknown-linux-musl" > /tmp/rust_target ;; \
      arm64) echo "aarch64-unknown-linux-musl" > /tmp/rust_target ;; \
      *) echo "Unsupported TARGETARCH: ${TARGETARCH}" && exit 1 ;; \
    esac

RUN rustup target add "$(cat /tmp/rust_target)"
RUN cargo build --release --bin agent_demo --target "$(cat /tmp/rust_target)"
RUN mkdir -p /out && cp \
  "/app/target/$(cat /tmp/rust_target)/release/agent_demo" \
  /out/agent_demo

FROM alpine:3.20 AS runtime

RUN addgroup -S csif && adduser -S -G csif -u 10001 csif

WORKDIR /app
RUN mkdir -p /data && chown -R csif:csif /data /app

ARG TARGETARCH
COPY --from=builder /out/agent_demo /app/agent_demo
COPY grammar.toml /app/grammar.toml
RUN chmod +x /app/agent_demo

ENV CSIF_BANK_PATH=/data/my_brain.rwif
ENV CSIF_GRAMMAR_PATH=/app/grammar.toml
EXPOSE 8080
VOLUME ["/data"]

HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 \
  CMD wget -q -O - http://127.0.0.1:8080/health || exit 1

USER csif
ENTRYPOINT ["/app/agent_demo"]
