# ---- Rust builder для cert-runner (sidecar для ACME issue/renew) ----
FROM rust:1-bookworm AS cert-builder
WORKDIR /build/cert-runner

# Кэш зависимостей.
COPY cert-runner/Cargo.toml cert-runner/Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release --locked && rm -rf src

COPY cert-runner/src ./src
RUN touch src/main.rs && cargo build --release --locked

# ---- Node builder ----
FROM node:22-bookworm-slim AS base
RUN corepack enable && corepack prepare pnpm@latest --activate
WORKDIR /app

FROM base AS deps
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

FROM base AS build
COPY --from=deps /app/node_modules ./node_modules
COPY . .
RUN pnpm build

# ---- Runtime ----
FROM node:22-bookworm-slim AS production
ENV NODE_ENV=production
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/cache/acme

COPY --from=deps /app/node_modules ./node_modules
COPY --from=build /app/dist ./dist
COPY package.json ./
COPY --from=cert-builder /build/cert-runner/target/release/cert-runner /usr/local/bin/cert-runner

EXPOSE 3000 80 443

# cert-runner — pid 1: выпустит/обновит cert, прокинет TLS_CERT_FILE/TLS_KEY_FILE
# в child и будет следить за expiry (renew + restart child автоматически).
# Если TLS_ENABLED не выставлен — просто прозрачно exec'ает child.
ENTRYPOINT ["/usr/local/bin/cert-runner"]
CMD ["node", "dist/main.js"]
