# SoundCloud networking — how to make requests & write SC methods

How the backend talks to SoundCloud, and the rules for adding a new SC call. Read this
before touching anything that fetches from SC.

## The model

Two SC APIs:
- **apiv1** — `api.soundcloud.com`, needs an OAuth token.
- **apiv2** — `api-v2.soundcloud.com`, needs only a `client_id` (no user token) for public
  data; accepts an OAuth token for private/`/me` data.

All **public** reads go through one facade, `ScReadService` (`src/sc/read.rs`), which runs a
3-tier state chain, moving to the next tier on failure:

```
CHAIN = A) apiv2 via the relay (signed Lua method)
        B) apiv2 via proxy&relay
        C) apiv1 via an OAuth token (direct → proxy&relay)   ← legacy, terminal fallback
```

- **A** and **(B→C)** are combined as a **hedge** by default (`CALL_FETCH_STRATEGY` =
  `hedge` | `race` | `fallback`); a per-A circuit breaker routes straight to B+C while the
  relay is failing.
- Every tier returns the **apiv1 JSON shape** (mapping in `src/sc/mapping.rs`:
  `normalize_v2_to_v1` + like-feed unwrap + playlist hydration), so persistence
  (`upsert_from_sc` / `ingest` / mirror) is identical regardless of which tier answered.
- The apiv1 tier is the existing `ScClient::api_get_value` → `with_fallback`
  (`direct → race(relay, proxy)`); it needs a token, resolved **lazily** so the happy path
  (A/B) never touches the token pool.

Variants of the chain a method may use:
- **CHAIN** — full A→B→C (single-entity reads; search; generic lists).
- **apiv2-only** (`A` hedged `B`, no C) — when an apiv2 cursor can't be honored on apiv1, so
  a paginated sequence stays in one cursor space (e.g. `playlist_tracks`, `collection_all`).
  The caller starts an apiv1 sequence itself if apiv2 can't even begin.

Private/owner `/me/*` and all **writes** do NOT use the facade — they stay on **apiv1 + the
user's own OAuth token** (apiv2 anon can't see private content).

## Decision table — which transport for which request

| Request | Transport |
|---|---|
| Public single entity (track/user/playlist/resolve by id) | `ScReadService` op → **CHAIN** |
| Public playlist track list | `read.playlist_tracks` (**apiv2-only**) → caller's apiv1 `/tracks` pagination |
| Public per-user collection (likes/playlists/followings/owned of **another** user) | `read.collection_all` (**apiv2-only**, paged) → apiv1 fallback |
| Public typed search (plain `q`) | `read.search_page` / `sc_search_page` → **CHAIN** |
| Public paginated list (comments/reposters/related/followers) | `read.list_page` / `sc_list_page { apiv2: true }` → **CHAIN** |
| Owner `/me/*` private (own likes/playlists/tracks/profile/feed) | **apiv1 + user token** (`sc.api_get_value`) |
| `secret_token` private resource | **apiv1 + token** |
| Write (like/follow/playlist CRUD/sharing/comment POST/track update·delete) | optimistic DB + `sync_queue` → **apiv1 + user token** (background) |
| Cron that reads public SC (discovery/enrich/walkers/search) | `ScReadService` op → **CHAIN** |
| Local data (recommendations/wave/discover/db-search/albums/artists/dislikes/history/aura/subscriptions) | **DB (+Qdrant/Redis)** — no SC |
| Lyrics | DB + external aggregators via proxy — **not SC** |

A full per-endpoint map lives at the bottom of this doc.

> **apiv2 endpoints are NOT all anon-accessible. Before adding one, prove it with curl.**
> Some return 401/404 for an anon `client_id` (e.g. `favoriters` 404, `web-profiles` 401,
> `/users/{id}/followings/{fid}` 404) — those MUST stay apiv1. Scrape a `client_id` and test:
> ```
> CID=$(curl -s -A "Mozilla/5.0" https://soundcloud.com/ | grep -oP '"hydratable":"apiClient","data":\{"id":"\K[^"]+' | head -1)
> curl -s "https://api-v2.soundcloud.com/<path>?client_id=$CID&limit=2" | jq .
> ```
> Add the apiv2 path **only if your curl returns 200** with the data you expect.

## Making a request (consumer side)

The services already hold `Arc<ScReadService>` as `read` (and `cold_refresh` for the
DB-backed cold pattern). Use these — do NOT call `sc.api_get_value` directly for a public
read.

- **Public single entity:** `self.read.track_by_id(kind, id)` / `user_by_id(kind, id)` /
  `playlist_meta(kind, id)` / `resolve(kind, url)`. `id` is the **bare** numeric id
  (`extract_sc_id`). `kind: TokenKind` only feeds the apiv1 tier (lazy).
- **Public playlist tracks:** `self.read.playlist_tracks(id)` (one-shot, full ordered list).
  In `cold_refresh::refresh_playlist_tracks` it falls back to apiv1 `/tracks` pagination.
- **Public collection (non-owner):** go through `cold_refresh` (`ensure_collection` →
  `read.collection_all`) so it mirrors to the DB; owner `/me/*` stays apiv1 in the same fn.
- **Public search:** `sc_search_page(ScSearchArgs { read, kind, ty, q, … })`. Only for a
  plain `q` (use `cache::plain_query`); `ids`/`genres`/`tags` stay apiv1 via `sc_list_page`.
- **Public paginated list:** `sc_list_page(ScListPageArgs { read, apiv2: true, path, … })`.
  Put the apiv1 path (e.g. `/tracks/{id}/comments`) in `path` with a **bare** id; the facade
  builds the api-v2 URL from it and host-routes the cursor. apiv2-only quirks go in
  `extra_params` (e.g. comments need `("threaded","0")`).
- **Owner `/me/*` private:** `sc.api_get_value("/me/…", token, params)` with the user token.
- **Write:** enqueue a `sync_queue` action; never block the request on SC.

The DB-backed cold pattern (most read endpoints): project from our tables first; on miss do
a synchronous fetch + persist; on stale spawn a background refresh. The fetch goes through
the facade. See `cold_refresh::service` and `playlists::get_tracks` for the canonical shape.

## Writing a new SC Lua method (channel A)

The relay is a generic executor: it signs and runs a Lua script we author and returns the
JSON. The script + business logic live HERE, not in the relay. Steps:

1. **Write `backend/sc_methods/<name>.lua`.** Conventions:
   - Globals from the relay: `inputs` (the method's JSON input), `client_id()`, `http(req)`,
     `json_decode`/`json_encode`, `urlencode`, `b64encode`, `log`. Stdlib `string`/`table`/
     `math` only — `os`/`io`/`require`/`load*`/`_G`/metatable/raw* are removed.
   - Append the `client_id` to every apiv2 URL (SC's `next_href` omits it → (re)append it).
   - **Failure convention:** `error("…")` → the relay fails over and retries
     (401/403/429/5xx); `return { ok = false, reason = "…" }` → terminal not-found;
     `return { ok = true, … }` → success.
2. **Embed + validate** with a const in `src/sc/lua_methods.rs`:
   ```rust
   pub const MY_METHOD: &str = call_lua_macros::lua_script!("sc_methods/my_method.lua");
   ```
   `lua_script!` parses (full_moon) + lint-checks the denylist at `cargo check`; editing the
   `.lua` rebuilds. **No relay/mock change is needed** — the script crosses the wire.
3. **Add a thin `ScClient` accessor** (`src/sc/client.rs`) that runs it and returns
   `Option<Value>` (None ⇒ caller falls back). Reuse `call_relay_method`:
   ```rust
   pub async fn my_method_via_relay(&self, arg: &str) -> Option<Value> {
       let inputs = serde_json::to_vec(&serde_json::json!({ "arg": arg })).ok()?;
       let v = self.call_relay_method("sc.my_method", lua_methods::MY_METHOD, inputs).await?;
       (v.get("ok").and_then(Value::as_bool) == Some(true)).then(|| v.get("…").cloned()).flatten()
   }
   ```
4. **Wire it into `ScReadService`** as channel A of an op (hedged with the apiv2-proxy backup
   in `Apiv2Proxy`, then apiv1). Implement the matching apiv2-proxy flow in `src/sc/apiv2.rs`
   so the host still works when the relay can't serve (channel B). Apply
   `mapping::normalize_v2_to_v1` so the output is apiv1-shaped.

Generic GET (`sc.apiv2_get` / `ScClient::apiv2_get_via_relay` / `read.list_page`) already
exists for simple paginated lists — prefer it over a bespoke method when you just need
"GET this api-v2 URL".

## Invariants & conventions

- **Mock-sync** (`utils/call/relay` is a mock returning `Disabled`; CI swaps the real crate):
  rely only on `Error::is_disabled()`; every `*_via_relay` returns `None` in the mock → caller
  falls back → OSS behavior unchanged. Do NOT add new public methods to `call_relay::Client`
  (`call_method` already exists on both); add accessors on `ScClient`.
- **Hedge, not blind race** — channel A is the primary; the backup starts only when A is
  slow/failed. (`CALL_FETCH_STRATEGY`, default `hedge`.)
- **Lazy tokens** — never compute the token chain before the facade call for a public read;
  the apiv1 tier resolves it only if reached. (This is why public reads work where the
  app-token pool is empty.)
- **Bare ids in paths** — apiv2 needs the bare numeric id (`extract_sc_id`); apiv1 accepts it
  too, so use bare ids for paths shared by both tiers.
- **No `.unwrap()/.expect()` in prod** code paths; terse, current-state comments.
- Relay/Lua authoring details: the call relay docs (`lua-methods.md`, `relay-usage.md`,
  `relay-apiv2-lua-rework.md` in the `call` crate) and `AGENTS.md`.

## Per-endpoint transport map

**CHAIN (apiv2 lua → apiv2 proxy&relay → apiv1):** `resolve`; `tracks.get_by_id`;
`users.get_by_id`; `playlists.get_by_id`; `tracks/users/playlists.search` (plain `q`);
`tracks.get_comments` (GET, `threaded=0`); `tracks.get_reposters`; `tracks.get_related`;
`users.get_followers`; `playlists.get_reposters`; `featured.pick`; crons `track_discovery`,
`duration_resolver`, `wanted_resolver` search, `sc_account_scan`, `artist_account_walker`,
`artist_crawl` resolve.

**apiv2-only → apiv1 pagination fallback:** `playlists.get_tracks` (`playlist_tracks`);
non-owner per-user collections (`collection_all`).

**apiv1 + user token:** owner `/me/*` (likes/playlists/tracks/profile/followings/feed/
followers); `secret_token` track/playlist; `tracks.get_streams`; all writes via `sync_queue`
(like/unlike, follow/unfollow, playlist CRUD/sharing, comment POST, track update/delete);
search with `ids`/`genres`/`tags`; `tracks.get_favoriters`; `users.get_web_profiles`;
`users.get_is_following`; `artist_crawl` web-profiles; auth (`/auth/*` → `secure.soundcloud.com`).

**Local only (no SC):** recommendations/wave/similar; `search/db/*`, `search/vibe`,
`search/lyrics`; `discover/*`; albums; artists; dislikes; events; history; aura;
subscriptions; admin; oauth-apps; stats. Lyrics use external aggregators via the proxy.
`/tracks/{urn}/stream` proxies to the **streaming** service (its own download flow).
