FROM rust:1-bookworm AS builder

# Shared tls-common crate is provided as a named build context.
# `additional_contexts: tls-common: ../tls-common` in compose,
# or `docker build --build-context tls-common=../tls-common ./storage`.
COPY --from=tls-common . /build/tls-common

WORKDIR /build/storage
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/storage/target/release/storage /usr/local/bin/storage

ENV PORT=3000 \
    ADMIN_TOKEN="" \
    STORAGE_BACKEND=local \
    STORAGE_PATH=/data/storage \
    TMP_PATH=/tmp/storage \
    MAX_TRANSCODES=2 \
    TRANSCODE_BATCH_SIZE=8 \
    TRANSCODE_BATCH_WAIT_MS=50 \
    UPLOAD_CONCURRENCY=64 \
    UPLOAD_RETRIES=4 \
    UPLOAD_RETRY_BASE_MS=250 \
    S3_REGION=us-east-1 \
    S3_FORCE_PATH_STYLE=true

RUN mkdir -p /data/storage/hq /data/storage/sq /var/cache/acme

EXPOSE 3000 80 443

CMD ["storage"]
