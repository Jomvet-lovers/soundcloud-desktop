# Direct Mode fork

This fork removes the hard dependency on the developer backend
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
  protects write endpoints with DataDome bot protection; only registered API
  apps (currently Artist Pro) can write. The app therefore stores them
  locally.
- Premium / ML features from the original backend: Discover catalog, Aura,
  SoundWave recommendations, vibe search, Yandex Music import. These show
  empty states.

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
