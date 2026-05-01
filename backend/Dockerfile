# ---- Rust builder для gateway (TLS-terminator + reverse proxy + ACME) ----
FROM rust:1-bookworm AS gateway-builder
WORKDIR /build/gateway

COPY gateway/Cargo.toml gateway/Cargo.lock* ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src

COPY gateway/src ./src
RUN touch src/main.rs && cargo build --release

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
    && mkdir -p /var/cache/acme /run/scd

COPY --from=deps /app/node_modules ./node_modules
COPY --from=build /app/dist ./dist
COPY package.json ./
COPY --from=gateway-builder /build/gateway/target/release/gateway /usr/local/bin/gateway

EXPOSE 80 443

ENTRYPOINT ["/usr/local/bin/gateway"]
CMD ["node", "dist/main.js"]
