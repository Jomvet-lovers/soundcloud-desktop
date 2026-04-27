FROM rust:bookworm AS builder

# Shared tls-common crate is provided as a named build context.
# `additional_contexts: tls-common: ../tls-common` in compose,
# or `docker build --build-context tls-common=../tls-common ./streaming`.
COPY --from=tls-common . /app/tls-common

WORKDIR /app/streaming

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Build actual binary
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/streaming/target/release/streaming /usr/local/bin/streaming

RUN mkdir -p /var/cache/acme

ENV PORT=8080
EXPOSE 8080 80 443

VOLUME ["/data"]

CMD ["streaming"]
