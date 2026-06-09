# SoundCloud-Desktop — Backend

Rust (axum) API + background pipelines for a SoundCloud desktop client. Mirrors SoundCloud content into our own catalog,
enriches it (artists/albums/lyrics), embeds it for a vector-based recommendation "wave", and streams audio. Built for *
*high load**: ~1.5M tracks, many concurrent users + background jobs.

## Stack & data stores

- **Postgres** (sqlx, queries are compile-time-checked `query_file!` macros — see **Database queries** below) — source
  of truth (`tracks`, `artists`, `albums`, `users`, likes/history/events, `wanted_tracks`,
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
  `../SoundCloud-Internal`), **proxy-systems** (`../Proxy-Systems`: intermediate + simple + ipv6 rotating
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
- **Every `/admin/*` route is gated per-handler by the `AdminAuth` extractor** (`common/admin.rs`: constant-time
  `x-admin-token` vs `config.admin.token`, fail-closed). There is **no** global auth layer in `router.rs` — a new admin
  handler MUST take `_: AdminAuth` as its **first** argument or the route is open to the world (this is how
  `/admin/collab/*` leaked). Body extractors (`Json`, `Option<Json<…>>`) are `FromRequest` and must stay **last**.
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

## Database queries (sqlx — FOLLOW THESE)

SQL is **checked against a real Postgres schema at compile time**: a query that selects a dropped/renamed column, or
binds a wrong-typed param, fails `cargo build` — not at 3am in prod. (This is the whole point — it once caught a live bug
where `sync_queue` inserted a `payload` column that migration `0019` had dropped.)

- SQL lives in **`backend/queries/<module>/<name>.sql`** (one query per file), **not** inline strings in Rust.
- Call it with a `query_file*!` macro:
    - `sqlx::query_file_scalar!("queries/<m>/x.sql", arg1, …)` — SELECT of **one column** → `T` / `Option<T>` / `Vec<T>`.
    - `sqlx::query_file_as!(MyRow, "queries/<m>/x.sql", …)` — SELECT into a `#[derive(sqlx::FromRow)]` struct; the `.sql`
      column list must match the struct fields **by name AND order** (expand `SELECT *` to an explicit list).
    - `sqlx::query_file!("queries/<m>/x.sql", …)` — INSERT/UPDATE/DELETE (+`RETURNING`; read `row.<col>`).
- At compile time the macro connects to `DATABASE_URL` and asks Postgres for the query's types — that connection **is**
  the check. **There is NO `.sqlx` offline cache** (deliberately `.gitignore`d — we don't commit that). → **you must
  have a Postgres with migrations applied to build.**

**Build/test locally** — bring up a dev DB once, then `cargo check` validates every macro against it:

```
podman run -d --name scd-dev-pg -e POSTGRES_USER=scd -e POSTGRES_PASSWORD=dev -e POSTGRES_DB=soundcloud_desktop \
  -p 127.0.0.1:55432:5432 docker.io/library/postgres:17-alpine
for f in migrations/*.sql; do podman exec -i scd-dev-pg psql -U scd -d soundcloud_desktop < "$f"; done
export DATABASE_URL=postgres://scd:dev@127.0.0.1:55432/soundcloud_desktop
cargo check --all-targets        # macros are validated against this schema
```

CI and `docker build` do the same automatically (spin up an ephemeral Postgres → apply migrations → build online).
Nothing to commit, nothing to keep in sync.

**Keep on runtime `sqlx::query(...)`** — these CANNOT be macros; leave a one-line comment saying which case:

- **Dynamic SQL** — built with `format!` / conditional `WHERE` / `QueryBuilder` (the string isn't static).
- **`INSERT … VALUES` binding `Option<…>`** — Postgres `DESCRIBE` doesn't report parameter nullability, so the macro
  infers params **NON-NULL** and rejects `Option`. (An UPDATE `SET c = COALESCE($n, c)` IS macro-able — `COALESCE` makes
  `$n` nullable.) The big upserts (`tracks`/`playlists`/`artists`/`albums`) stay runtime for this.
- **`UNNEST($1::int8[], …)` with `Vec<Option<T>>`** (array of nullable elements — macro wants `&[T]`).
- **`pg_stat_*` / system-catalog** queries.

**Nullability hints in the `.sql`:** struct field is `Option<T>` but the column is `NOT NULL` and comes via a
`LEFT JOIN` (so actually nullable) → alias it `AS "col?"`; a non-null expression the macro thinks is nullable
(`COUNT(*)`, `EXISTS(…)`, `(xmax = 0)`) → `AS "col!"`.

**⚠ Formatter trap:** JetBrains "Reformat Code" / Actions-on-Save splits `=>` — `make_interval(days => $n)` becomes
`days = > $n` → SQL syntax error → red CI/Docker. Either disable auto-reformat for `backend/queries/**`, or avoid `=>`
(write `$n::int * INTERVAL '1 day'`).

## Migrations (FOLLOW THESE)

`migrations/NNNN_*.sql`, sqlx, **embedded at compile time** (`sqlx::migrate!()` in `db/mod.rs`). Applied on boot under an
advisory lock when `MIGRATE_ON_BOOT` ≠ `false`; otherwise the standalone `migrate` bin (`src/bin/migrate.rs`) runs them
as a discrete pre-start deploy step (a failed migration then fails the deploy, not the running app).

- **A `.sql` edit needs a rebuild+redeploy** to take effect — patching the file and restarting the old binary changes
  nothing. **Never edit an already-applied migration:** the checksum (SHA-384 of the file) lives in `_sqlx_migrations`,
  and any mismatch aborts startup with `VersionMismatch`. Fix forward with a new `NNNN_*.sql`. (Editing a *pending*
  one — not yet in `_sqlx_migrations` — is safe; it applies fresh.) **Enforced** by `scripts/check-migrations.sh`
  (pre-commit `.githooks/pre-commit` + CI `migrations-guard.yml`): rejects edits to a committed migration and
  duplicate/out-of-order numbers; **eugene** lints new migrations for dangerous locks (advisory).
- **One file = one simple-query message = one implicit transaction.** Postgres runs every `;`-separated statement in the
  file as a single transaction, so a migration with ≥2 statements is *always* transactional — `-- no-transaction` does
  **not** change that (it only drops sqlx's own `BEGIN/COMMIT` wrapper).
- **Non-transactional DDL** (`CREATE`/`DROP INDEX CONCURRENTLY`, `REINDEX CONCURRENTLY`, `ALTER TYPE … ADD VALUE`,
  `VACUUM`) cannot run in a transaction. Two ways to ship it:
  - **Default (what every index migration here does):** plain `CREATE INDEX IF NOT EXISTS` in the normal migration, and
    **pre-create the big index `CONCURRENTLY` by hand on prod before deploy** so the migration no-ops via `IF NOT
    EXISTS`. Keep everything idempotent (`IF NOT EXISTS`, `ADD COLUMN IF NOT EXISTS`).
  - **Only when the migration itself must build concurrently:** `-- no-transaction` as the **first line** *and* the file
    holds **exactly one statement** — no `DO` block, no second DDL, nothing else. (A `DO` self-heal block +
    `CREATE INDEX CONCURRENTLY` = two statements = implicit tx = the error that took prod down — `0029`.)
- **A failed `CREATE INDEX CONCURRENTLY` leaves an INVALID index** that `IF NOT EXISTS` then silently skips (planner
  ignores it → seq scans). Drop invalid leftovers by hand; self-heal can't share the file with the concurrent build.
- **Keep boot migrations cheap.** They run while every starting instance blocks on the migration advisory lock; a heavy
  in-transaction index build or backfill stalls the whole fleet — pre-create or backfill on prod instead.

## Commands

- Build/check: needs a migrated Postgres (see **Database queries**) — `DATABASE_URL=… cargo check --all-targets` (in
  `backend/`). Lint: `cargo clippy --all-targets -- -D warnings` — CI runs latest stable, so keep your toolchain current
  (`rustup update`) or you'll miss newer lints it rejects. Migrations: see **Migrations** above.
- Query plans: `scripts/check-query-plans.sh` (CI `query-plans.yml`) flags `Seq Scan` on big tables (advisory; warms
  prod-like sizes via `scripts/load-approx-stats.sql`). Prod slow queries / unused indexes: `GET /admin/slow-queries`,
  `GET /admin/index-usage` (header `x-admin-token`).
- Key env: `PG_POOL_MAX`, `ENRICH_CONSUMER_CONCURRENCY`, `LYRICS_INDEXING_CONCURRENCY`, `GENIUS_MAX_CONCURRENT_SCRAPES`,
  `GENIUS_ACCESS_TOKEN`, `MAX_TRACK_DURATION_SEC`, `ENRICH_*`, `SC_PROXY_URL`, `CALL_*`, `TLS_PROXY_TRUSTED_HOSTS`.
- Prod: compose on dedic `ssh dedic-ru:/root/docker-compose.yml`; DB/qdrant/minio creds in
  `../SoundCloud-Desktop-Infra/_main-host-now/docker-compose.yml`. Query prod DB from PC via
  `podman run ... postgres:17-alpine psql -h <dedic> ...`.
