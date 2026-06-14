# Multi-stage build for the detachable cloud services (sentinel-control / -gateway /
# -flow). Parameterized by PACKAGE so one Dockerfile builds any of them:
#   docker build --build-arg PACKAGE=sentinel-control -t sentinel-control .
#
# cargo-chef caches the dependency compile (the heavy app-core/tract layer) so source
# changes don't recompile the world. Pure-Rust + rustls throughout → no OpenSSL; the
# runtime image only needs CA certs.
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG PACKAGE
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release -p "${PACKAGE}" \
    && cp "target/release/${PACKAGE}" /app/service

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/service /usr/local/bin/service
ENTRYPOINT ["/usr/local/bin/service"]
