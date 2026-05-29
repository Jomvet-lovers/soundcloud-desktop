# SoundCloud-Desktop — Backend

Rust (axum) API + background pipelines for a SoundCloud desktop client. Mirrors SoundCloud content into our own catalog,
enriches it (artists/albums/lyrics), embeds it for a vector-based recommendation "wave", and streams audio. Built for *
*high load**: ~1.5M tracks, many concurrent users + background jobs.

## Stack & data stores

- **Postgres** (sqlx) — source of truth (`tracks`, `artists`, `albums`, `users`, likes/history/events, `wanted_tracks`,
  `lyrics_cache`). Pool is small (`PG_POOL_MAX`, ~20–50); `max_connections=200` shared across services. **Connections
  are precious — never hold one across network/RPC work.**
- **Qdrant** — vector search. Collections: `tracks_mert` (1024d audio), `tracks_clap` (512d audio), `tracks_lyrics` (
  1024d), `tracks_collab` (128d, item2vec). Point id = `sc_track_id`.
- **NATS JetStream** — work queues between backend ↔ worker (enrich, embed, transcribe, index, storage events). See
  `bus/`.
- **Redis** — caches, wave cursors, single-flight locks, rate budgets.
- **MinIO/S3** — transcoded audio (`soundcloud_tracks_<id>.m4a`).
- Sibling services (separate repos/images): **streaming** (SC→S3 download/transcode), **worker** (Python:
  whisper/demucs/embeddings/LLM RPC over NATS), **call** (relay through user clients to dodge SC bans —
  `../SoundCloud-Desktop-Internal`), **proxy-systems** (`../Proxy-Systems`: intermediate + simple + ipv6 rotating
  proxies for external APIs), **tls-common** (`utils/tls-common`, shared TLS/ACME/PROXY-protocol).

## Track lifecycle (the spine)

`ingest` (like/playlist/discovery → `indexing::ingest_track_from_sc`, UPSERT `tracks`, priority set) → **storage** (
`streaming` downloads from SC → S3; `storage_state`) → **index** (worker embeds audio+lyrics → Qdrant; `index_state`) →
**enrich** (link artists/albums; `enrich_state`) → **lyrics** (aggregators + self-gen whisper). Each stage has its own
state column + pickup. The bottleneck in prod is **SC download** (rates/bans) — mitigated by the `call` relay + rotating
proxies.

The **wave** (`recommendations::smart_wave`) blends 3 arms — track-arm (clap+mert+lyrics NN from seed likes),
artist-arm (affinity graph), collab-arm (item2vec) — and degrades to popularity when a user's taste isn't indexed yet.
So wave quality depends on the user's liked tracks being **indexed** (vectors), and artist pages on tracks being *
*enriched** (linked).

## High-load conventions (FOLLOW THESE)

- **NATS consumers run concurrently, bounded.** `bus/nats.rs::consume(stream, durable, filter, concurrency, handler)`
  spawns handlers up to `concurrency` (permit-before-pull backpressure, ack on completion, ack_wait 120s). Pick
  concurrency per consumer (enrich = `ENRICH_CONSUMER_CONCURRENCY`; quick done-handlers ~16). Never go back to awaiting
  handlers serially.
- **Never hold a pooled DB connection across `.await` on network/RPC/slow work.** (This was the enrich
  `pg_advisory_lock` bug → pool exhaustion at 1 track/min.) Dedup via in-memory `mini_moka` cache + **idempotent UPSERT
  ** (`ON CONFLICT`) + freshness checks, not session locks. Acquire a connection only for the query, release
  immediately.
- **Parallelize fan-out with `futures::future::join_all` + a `Semaphore` cap**, not serial `for x { ...await... }` and
  not a global `Throttle` (a `Throttle` serializes a hot path). Ban-resistance comes from the rotating ipv6 proxies, not
  from app-side throttling.
- **External APIs** (Genius/MB/lrclib) go through the proxy via `common/external_fetch.rs`. Force
  `Accept-Encoding: identity` (the proxy strips `content-encoding` without decompressing — see [proxy bug] below).
  `get_api` = direct-first (token APIs), `get_scrape` = proxy-first (web). Genius concurrency =
  `GENIUS_MAX_CONCURRENT_SCRAPES`.
- **Prioritize user-relevant work.** `TrackPriority` (Like=1 … Discovery=5) → `tracks.{index,storage}_priority`; enrich
  backfill orders by `index_priority` too. Likes/owned must beat the discovery firehose for SC-download/index/enrich.
- **Skip pointless external work.** MusicBrainz only for ISRC/`metadata_artist` (label) tracks — it never matches
  underground SC uploads and its throttle serializes enrich.
- **Tracks > `MAX_TRACK_DURATION_SEC` (7 min)** are terminal `too_long` (storage/index = `too_long`, transcribe
  `disabled`) — not downloaded/indexed (DJ sets/podcasts bloat S3, useless for the wave). Frontend shows an `F` badge.
- **Comment style:** terse, current-state only. No narrative-of-the-change comments, no rationale paragraphs.

## Gotchas (verified in prod)

- **Proxy strips `Content-Encoding` without decompressing** → gzip/br bodies arrive as garbage; logged only at `debug`.
  Always send `Accept-Encoding: identity` for proxied fetches. Fixed in `proxy-common/headers.rs` (forces identity) +
  backend `external_fetch`.
- **`call` relay** (SC-ban bypass) must reach `control.scdinternal.site`; the call server expects PROXY-protocol only
  from haproxy. Internal services connect direct (docker alias, bypassing haproxy) → tls-common does optional
  PROXY-detect + trusts only `TLS_PROXY_TRUSTED_HOSTS=haproxy` (auto-resolved). Port `:444` is the desktop's direct
  gRPC (DNAT preserves client IP).

## Module map (`src/modules/`)

`indexing` (ingest + pipeline kick + reaps), `tracks` (repository/UPSERT/projection), `enrich` (`resolver` artist/album
resolution: ISRC→MB→Genius→AI→heuristic; `artist_crawl` Genius/MB catalog → `wanted_tracks`; `persist`), `lyrics` (
aggregators lrclib/mxm/genius/netease + self-gen transcribe), `recommendations` (`smart_wave`, arms, blender, cursors,
clusters, bandits, trainer), `collab`/`centroids` (vectors), `cold_refresh` (TTL-based SC re-sync), `auth`/
`oauth_apps` (SC token chains + proxy), `sync_queue` (write-back to SC), `resolve` (SC resolve API), read-path:
`search discover albums artists playlists users me likes dislikes history auras featured subscriptions`. Infra: `bus/` (
nats), `cache/`, `db/`, `qdrant/`, `redis/`, `sc/` (ScClient), `common/` (`external_fetch`, `throttle`), `config.rs`,
`main.rs`.

## Commands

- Build/check: `cargo check` / `cargo check --all-targets` (in `backend/`). Migrations: `migrations/NNNN_*.sql` (sqlx,
  applied at startup; use `CREATE INDEX IF NOT EXISTS`, and create big indexes `CONCURRENTLY` on prod first).
- Key env: `PG_POOL_MAX`, `ENRICH_CONSUMER_CONCURRENCY`, `LYRICS_INDEXING_CONCURRENCY`, `GENIUS_MAX_CONCURRENT_SCRAPES`,
  `GENIUS_ACCESS_TOKEN`, `MAX_TRACK_DURATION_SEC`, `ENRICH_*`, `SC_PROXY_URL`, `CALL_*`, `TLS_PROXY_TRUSTED_HOSTS`.
- Prod: compose on dedic `ssh dedic-ru:/root/docker-compose.yml`; DB/qdrant/minio creds in
  `../SoundCloud-Desktop-Infra/_main-host-now/docker-compose.yml`. Query prod DB from PC via
  `podman run ... postgres:17-alpine psql -h <dedic> ...`.
