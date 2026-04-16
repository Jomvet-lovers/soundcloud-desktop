FROM rust:1-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/storage /usr/local/bin/storage

ENV PORT=3000 \
    ADMIN_TOKEN="" \
    STORAGE_BACKEND=local \
    STORAGE_PATH=/data/storage \
    TMP_PATH=/tmp/transcode \
    MAX_TRANSCODES=2 \
    S3_REGION=us-east-1 \
    S3_FORCE_PATH_STYLE=true

RUN mkdir -p /data/storage/hq /data/storage/sq

EXPOSE 3000

CMD ["storage"]
