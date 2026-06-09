FROM rust:1-bookworm AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler postgresql postgresql-client \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build

COPY --from=tls-common . /build/utils/tls-common
COPY --from=call . /build/utils/call

WORKDIR /build/backend
COPY Cargo.toml ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src target/release/backend target/release/deps/backend*

COPY src ./src
COPY migrations ./migrations
COPY queries ./queries
# query_file! макросы сверяются на компиляции с РЕАЛЬНОЙ схемой: поднимаем эфемерный
# postgres прямо в билд-стадии, катим миграции, собираемся online. Никакого .sqlx.
RUN pg_ctlcluster "$(ls /etc/postgresql)" main start \
    && su postgres -c "psql -v ON_ERROR_STOP=1 --command \"CREATE ROLE scd LOGIN SUPERUSER PASSWORD 'dev';\"" \
    && su postgres -c "createdb -O scd scd" \
    && for f in migrations/*.sql; do PGPASSWORD=dev psql -h 127.0.0.1 -U scd -d scd -v ON_ERROR_STOP=1 -q -f "$f"; done \
    && touch src/main.rs \
    && DATABASE_URL="postgres://scd:dev@127.0.0.1:5432/scd" cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/cache/acme

COPY --from=builder /build/backend/target/release/backend /usr/local/bin/backend

EXPOSE 80 443 3000

ENTRYPOINT ["/usr/local/bin/backend"]
