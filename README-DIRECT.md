# Direct Mode build

Personal, modified build based on
[SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop) by
[@zxcloli666](https://github.com/zxcloli666) — MIT License, all original
credit belongs to the upstream project.

This build removes the hard dependency on the developer backend
(`api.scnative.space`). The app talks to SoundCloud itself and keeps user
actions locally.

## What works

- Search (tracks / playlists / users / albums), via the public SoundCloud
  api-v2 with a `client_id` extracted from the SoundCloud homepage.
- Playback: resolved and downloaded straight from SoundCloud
  (`soundcloud.com` -> `api-v2` transcoding -> audio cache). No rela
  infrastructure needed.
- Library: your liked tracks, playlists, followings and the following feed
  (read-only import from your SoundCloud account).
- Page browsing: tracks, playlists, users, albums, comments (read).
- Local actions, persisted in `direct_store.json` (app data dir), not synced
  to soundcloud.com:
  - like / unlike tracks and playlists
  - follow / unfollow users
  - create / edit / delete playlists
  - post comments (stored locally)
  - playback history
  - dislikes

## What does not work (yet)

- Syncing likes / follows / playlist edits back to SoundCloud. SoundCloud
  protects write endpoints with DataDome bot protection. Every non-trusted
  client is rejected (plain HTTP, TLS-impersonated HTTP, replayed datadome
  cookie, and an embedded WebView2 whose challenge never finishes). Writes
  stay local in `direct_store.json`; the experimental writer lives in
  `src-tauri/src/direct/webview.rs` behind `SYNC_ENABLED = false`.

## Removed in this fork

Backend-only features were removed together with the "vibe" decoration layer:

- Discover catalog, Star/premium pages and pay flows
- SoundWave / recommendations / vibe search / clusters
- Aura palettes and decorative star fields (neutralised stubs remain so the
  API surface still compiles)
- Lyrics panel, Yandex Music import, QR session transfer, P2P call network,
  host-status banners
- Wallhaven online wallpaper search (custom image / URL wallpaper remains)
- Gradients, glow shadows, backdrop blur, decorative badges and the
  Fraunces/Unbounded display fonts (Inter + JetBrains Mono only)

## Signing in

Direct mode uses the `oauth_token` cookie of your SoundCloud web session:

1. Log in at https://soundcloud.com in your browser.
2. Open DevTools (F12) -> Application -> Storage -> Cookies -> https://soundcloud.com
3. Copy the value of the `oauth_token` cookie.
4. Paste it into the login screen and press "Sign in with token".

The token is stored locally in the app data directory (Rust session store)
and is only sent to SoundCloud.

## Build

```
cd desktop
corepack pnpm install
corepack pnpm tauri build     # or: corepack pnpm tauri dev
```

Requirements on Windows: Rust (MSVC), VS Build Tools, CMake, NASM, LLVM
(libclang), Node 20+.
