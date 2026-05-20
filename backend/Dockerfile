FROM rust:1-bookworm AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build

COPY --from=tls-common . /build/utils/tls-common
COPY --from=call . /build/utils/call

WORKDIR /build/backend
COPY Cargo.toml ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src target/release/backend target/release/deps/backend*

COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/cache/acme

COPY --from=builder /build/backend/target/release/backend /usr/local/bin/backend

EXPOSE 80 443 3000

ENTRYPOINT ["/usr/local/bin/backend"]
