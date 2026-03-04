# SoundCloud Desktop — Backend Implementation Plan

## Overview
NestJS backend acting as a proxy/BFF (Backend-For-Frontend) between the Tauri desktop app and the SoundCloud API. Handles OAuth 2.1 with PKCE, token management, and proxies all SC API endpoints. Generates its own OpenAPI schema at `/openapi.json`.

## Tech Stack
- **Runtime:** Node.js + NestJS 11
- **Package Manager:** pnpm
- **Linter:** Biome (replacing ESLint + Prettier)
- **Database:** PostgreSQL (via TypeORM) — stores user sessions, tokens
- **HTTP Client:** built-in fetch / `@nestjs/axios`
- **OpenAPI:** `@nestjs/swagger`
- **Docker:** Dockerfile + docker-compose (dev + prod)
- **Testing:** Jest (unit) + Supertest (e2e)

## Architecture

```
Desktop App (Tauri)
       │
       ▼
  Our NestJS Backend (localhost:3000)
       │
       ▼
  SoundCloud API (api.soundcloud.com)
```

### Module Structure
```
src/
├── main.ts
├── app.module.ts
├── config/
│   └── configuration.ts            # env-based config (SC client_id, secret, etc.)
├── common/
│   ├── guards/
│   │   └── auth.guard.ts           # checks valid session before proxying
│   ├── interceptors/
│   │   └── soundcloud-api.interceptor.ts
│   ├── decorators/
│   │   └── current-user.decorator.ts
│   └── dto/
│       └── pagination.dto.ts       # linked_partitioning support
├── auth/
│   ├── auth.module.ts
│   ├── auth.controller.ts          # /auth/login, /auth/callback, /auth/refresh, /auth/logout
│   ├── auth.service.ts             # OAuth 2.1 + PKCE logic, token exchange
│   └── entities/
│       └── session.entity.ts       # PG: stores access_token, refresh_token, expires_in, user info
├── soundcloud/
│   ├── soundcloud.module.ts
│   ├── soundcloud.service.ts       # HTTP client wrapper for SC API, auto-refresh tokens
│   └── soundcloud.types.ts         # TypeScript interfaces matching SC API schemas
├── me/
│   ├── me.module.ts
│   ├── me.controller.ts            # /me, /me/feed, /me/likes/*, /me/followings/*, /me/followers, /me/playlists, /me/tracks
│   └── me.service.ts
├── tracks/
│   ├── tracks.module.ts
│   ├── tracks.controller.ts        # /tracks (search+upload), /tracks/:id, /tracks/:id/streams, comments, related, etc.
│   └── tracks.service.ts
├── playlists/
│   ├── playlists.module.ts
│   ├── playlists.controller.ts     # /playlists (search+create), /playlists/:id (CRUD), tracks, reposters
│   └── playlists.service.ts
├── users/
│   ├── users.module.ts
│   ├── users.controller.ts         # /users (search), /users/:id, followers, followings, tracks, playlists, likes, web-profiles
│   └── users.service.ts
├── likes/
│   ├── likes.module.ts
│   ├── likes.controller.ts         # /likes/tracks/:id (POST/DELETE), /likes/playlists/:id (POST/DELETE)
│   └── likes.service.ts
├── reposts/
│   ├── reposts.module.ts
│   ├── reposts.controller.ts       # /reposts/tracks/:id, /reposts/playlists/:id
│   └── reposts.service.ts
├── resolve/
│   ├── resolve.module.ts
│   ├── resolve.controller.ts       # /resolve?url=...
│   └── resolve.service.ts
└── health/
    └── health.controller.ts        # /health
```

## Implementation Phases

### Phase 0: Cleanup & Setup
- [x] Remove ESLint, Prettier, their configs and deps
- [x] Install & configure Biome
- [x] Install core deps: `@nestjs/swagger`, `@nestjs/config`, `@nestjs/typeorm`, `typeorm`, `pg`, `@nestjs/axios`, `axios`
- [x] Configure `@nestjs/swagger` to serve OpenAPI at `/openapi.json`
- [x] Setup env-based config (`SOUNDCLOUD_CLIENT_ID`, `SOUNDCLOUD_CLIENT_SECRET`, `SOUNDCLOUD_REDIRECT_URI`, `DATABASE_URL`, etc.)
- [x] Setup TypeORM with PG connection

### Phase 1: Auth (OAuth 2.1 + PKCE)
- [x] `GET /auth/login` — generates PKCE code_verifier/challenge, state, returns SC authorize URL
- [x] `GET /auth/callback?code=...&state=...` — exchanges code for tokens, stores session, renders success/error HTML page (glass UI)
- [x] `POST /auth/refresh` — refreshes access_token using refresh_token
- [x] `POST /auth/logout` — invalidates session, calls SC sign-out
- [x] AuthGuard — extracts session from request, validates token, attaches user to request

### Phase 2: SoundCloud API Proxy Service
- [x] `SoundcloudService` — wraps SC API calls with auto token injection and refresh
- [x] TypeScript types for all SC API responses (User, Me, Track, Playlist, Comment, etc.)

### Phase 3: API Modules
- [x] **Me** — all /me/* endpoints (profile, feed, likes, followings, followers, playlists, tracks)
- [x] **Tracks** — search, CRUD, streams, preview, comments, favoriters, reposters, related
- [x] **Playlists** — search, CRUD, tracks, reposters
- [x] **Users** — search, get, followers, followings, tracks, playlists, likes, web-profiles
- [x] **Likes** — like/unlike tracks & playlists
- [x] **Reposts** — repost/unrepost tracks & playlists
- [x] **Resolve** — URL resolution

### Phase 4: Infra
- [x] Dockerfile (multi-stage: build + prod)
- [x] docker-compose.yml (dev: app + PG; prod: app + PG)
- [x] Health endpoint

### Phase 5: Auth Callback UI
- [x] Glass-morphism Vision Pro style HTML page for auth success/error
- [x] Auto-redirect to desktop app via custom protocol or close instruction

## OAuth Flow Details

```
1. App calls: GET /auth/login
   → Backend generates: code_verifier, code_challenge (S256), state
   → Stores in session/memory
   → Returns: { url: "https://secure.soundcloud.com/authorize?client_id=...&redirect_uri=...&response_type=code&code_challenge=...&code_challenge_method=S256&state=..." }

2. User opens URL in browser → SoundCloud auth page

3. SC redirects to: GET /auth/callback?code=XXXX&state=YYYY
   → Backend verifies state
   → POST https://secure.soundcloud.com/oauth/token
     { grant_type: "authorization_code", client_id, client_secret, code, redirect_uri, code_verifier }
   → Stores access_token, refresh_token, expires_in in PG
   → Returns pretty HTML page (success or error)

4. App polls or uses session ID to get token status
```

## SoundCloud API Endpoints Coverage (non-deprecated only)

| Our Endpoint | SC Endpoint | Method |
|---|---|---|
| `/me` | `/me` | GET |
| `/me/feed` | `/me/feed` | GET |
| `/me/feed/tracks` | `/me/feed/tracks` | GET |
| `/me/likes/tracks` | `/me/likes/tracks` | GET |
| `/me/likes/playlists` | `/me/likes/playlists` | GET |
| `/me/followings` | `/me/followings` | GET |
| `/me/followings/tracks` | `/me/followings/tracks` | GET |
| `/me/followings/:user_urn` | `/me/followings/{user_urn}` | PUT/DELETE |
| `/me/followers` | `/me/followers` | GET |
| `/me/playlists` | `/me/playlists` | GET |
| `/me/tracks` | `/me/tracks` | GET |
| `/tracks` | `/tracks` | GET/POST |
| `/tracks/:id` | `/tracks/{track_urn}` | GET/PUT/DELETE |
| `/tracks/:id/streams` | `/tracks/{track_urn}/streams` | GET |
| `/tracks/:id/comments` | `/tracks/{track_urn}/comments` | GET/POST |
| `/tracks/:id/favoriters` | `/tracks/{track_urn}/favoriters` | GET |
| `/tracks/:id/reposters` | `/tracks/{track_urn}/reposters` | GET |
| `/tracks/:id/related` | `/tracks/{track_urn}/related` | GET |
| `/playlists` | `/playlists` | GET/POST |
| `/playlists/:id` | `/playlists/{playlist_urn}` | GET/PUT/DELETE |
| `/playlists/:id/tracks` | `/playlists/{playlist_urn}/tracks` | GET |
| `/playlists/:id/reposters` | `/playlists/{playlist_urn}/reposters` | GET |
| `/users` | `/users` | GET |
| `/users/:id` | `/users/{user_urn}` | GET |
| `/users/:id/followers` | `/users/{user_urn}/followers` | GET |
| `/users/:id/followings` | `/users/{user_urn}/followings` | GET |
| `/users/:id/tracks` | `/users/{user_urn}/tracks` | GET |
| `/users/:id/playlists` | `/users/{user_urn}/playlists` | GET |
| `/users/:id/likes/tracks` | `/users/{user_urn}/likes/tracks` | GET |
| `/users/:id/likes/playlists` | `/users/{user_urn}/likes/playlists` | GET |
| `/users/:id/web-profiles` | `/users/{user_urn}/web-profiles` | GET |
| `/likes/tracks/:id` | `/likes/tracks/{track_urn}` | POST/DELETE |
| `/likes/playlists/:id` | `/likes/playlists/{playlist_urn}` | POST/DELETE |
| `/reposts/tracks/:id` | `/reposts/tracks/{track_urn}` | POST/DELETE |
| `/reposts/playlists/:id` | `/reposts/playlists/{playlist_urn}` | POST/DELETE |
| `/resolve` | `/resolve` | GET |
