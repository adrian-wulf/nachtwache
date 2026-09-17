FROM rust:slim AS builder

WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev cmake build-essential && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web

RUN cargo build --release

FROM debian:trixie-slim

WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/nachtwache /app/nachtwache

ENV PORT=8080
ENV HOST=0.0.0.0
ENV NACHTWACHE_DB=/data/nachtwache.db

VOLUME ["/data"]
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s \
  CMD curl -f http://localhost:8080/api/stats || exit 1

CMD ["/app/nachtwache"]
