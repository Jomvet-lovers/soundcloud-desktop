FROM rust:bookworm AS builder

WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Build actual binary
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/streaming /usr/local/bin/streaming

ENV PORT=8080
EXPOSE 8080

VOLUME ["/data"]

CMD ["streaming"]
