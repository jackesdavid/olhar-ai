# Multi-stage build for the detachable cloud services (sentinel-control / -gateway / -flow).
# Parameterized by PACKAGE so one Dockerfile builds any of them:
#   docker build --build-arg PACKAGE=sentinel-control -t sentinel-control .
#
# ## Why the builder compiles ALL THREE binaries
#
# It used to compile only `${PACKAGE}`, so three CI jobs each compiled the same dependency tree
# and the same app-core: 18.5 + 18.3 + 16.2 = 53 minutes of compute to produce three binaries
# that share almost every object file.
#
# The builder stage is now PACKAGE-independent — one cache key for all three images. Build them
# in a single job and BuildKit computes it once and reuses it twice, so the second and third
# images cost only their runtime layer. `ARG PACKAGE` appears ONLY in the final stage; declaring
# it earlier forks the builder cache per service and undoes the whole point.
#
# cargo-chef caches the dependency compile (the heavy app-core/tract layer) so source changes do
# not recompile the world. Pure-Rust + rustls throughout → no OpenSSL; runtime needs only CA certs.

# Prebuilt cargo-chef image. `cargo install cargo-chef` compiled the tool ITSELF on every cold
# build — minutes spent building a build tool, once per service.
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Dependencies only. Reused until Cargo.lock changes, which is what makes a source-only change cheap.
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
# All three in ONE cargo invocation: they share app-core and nearly every dependency, so each
# binary links from objects cargo has already built instead of starting over per package.
RUN cargo build --release \
      -p sentinel-control \
      -p sentinel-flow \
      -p sentinel-gateway \
      -p sentinel-notification

FROM debian:bookworm-slim AS runtime
ARG PACKAGE
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/${PACKAGE} /usr/local/bin/service
ENTRYPOINT ["/usr/local/bin/service"]
