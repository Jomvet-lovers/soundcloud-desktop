FROM rust:bookworm AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Shared utils-крейты приходят отдельными named build context'ами.
# `additional_contexts:` в compose или `--build-context name=../utils/<name>` в docker build.
COPY --from=tls-common . /app/utils/tls-common
COPY --from=call . /app/utils/call
COPY --from=decrypt . /app/utils/decrypt
COPY --from=decrypt-client . /app/utils/decrypt-client

WORKDIR /app/streaming

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Build actual binary
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/streaming/target/release/streaming /usr/local/bin/streaming

RUN mkdir -p /var/cache/acme

ENV PORT=8080
EXPOSE 8080 80 443

VOLUME ["/data"]

CMD ["streaming"]
