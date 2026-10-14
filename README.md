[English](README.md) | [日本語](README.ja.md)

<p align="center">
<a href="https://github.com/Jomvet-lovers/SCD-Direct/releases/latest">
<img src=".github/assets/logo.png" width="180px" alt="SCD-Direct" />
</a>
</p>

<h1 align="center"><a href="https://github.com/Jomvet-lovers/SCD-Direct">SCD-Direct</a></h1>

---

<p align="center">
<b>[<a href="https://github.com/zxcloli666/SoundCloud-Desktop">Original project</a>]</b><br>
<b>Unofficial SoundCloud desktop client — backend-free direct mode</b><br>
No ads · No captcha · No third-party backend · Download tracks & listen offline
</p>

<p align="center">
<a href="https://github.com/Jomvet-lovers/SCD-Direct/releases/latest">
<img src="https://img.shields.io/github/v/release/Jomvet-lovers/SCD-Direct?style=for-the-badge&logo=github&color=FF5500&label=VERSION" alt="Version"/>
</a>
<a href="https://github.com/Jomvet-lovers/SCD-Direct/releases">
<img src="https://img.shields.io/github/downloads/Jomvet-lovers/SCD-Direct/total?style=for-the-badge&logo=download&color=FF5500&label=Downloads" alt="Downloads"/>
</a>
<a href="https://github.com/Jomvet-lovers/SCD-Direct/stargazers">
<img src="https://img.shields.io/github/stars/Jomvet-lovers/SCD-Direct?style=for-the-badge&logo=github&color=FF5500&label=Stars" alt="Stars"/>
</a>
<a href="LICENSE">
<img src="https://img.shields.io/badge/License-MIT-FF5500?style=for-the-badge" alt="License"/>
</a>
</p>

<p align="center">
<a href="https://github.com/Jomvet-lovers/SCD-Direct/releases/latest">
<img src="https://img.shields.io/badge/Download-Latest_Version-FF5500?style=for-the-badge" alt="Download"/>
</a>
<a href="README.ja.md">
<img src="https://img.shields.io/badge/日本語-README-0066FF?style=for-the-badge" alt="Japanese README"/>
</a>
</p>

> **Unofficial third-party SoundCloud desktop client.** This is a standalone,
> modified copy of [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)
> by [@zxcloli666](https://github.com/zxcloli666), used under the MIT License.
> It is not affiliated with or endorsed by SoundCloud. All original credit belongs
> to the upstream project.
>
> **Intended for personal use only — provided as-is, at your own risk.** This is not
> a registered or authorized SoundCloud API client; it talks to SoundCloud the same
> way the web player does. Using it — including syncing your actions back to
> SoundCloud — is your own decision and may affect your account. See SoundCloud's
> [terms of use](https://soundcloud.com/terms-of-use). No warranty, no liability.

A backend-free **direct mode** build of this SoundCloud desktop app (Tauri v2 + React + Rust):
listen to SoundCloud on your desktop without a browser.
Instead of relying on the original developer backend (`api.scnative.space`), the app
talks to SoundCloud itself: search and playback go straight to the public api-v2, and
your actions (likes, follows, playlists, comments, history) are kept in a local store
and written back to SoundCloud where possible — see [Limitations](#limitations).

## Screenshots

![Home — greeting, liked tracks and the now-playing bar](screenshots/home.png)
![Search — genre wall](screenshots/search.png)
![Library — soundprint and following feed](screenshots/library.png)
![History — playback history](screenshots/history.png)
![Settings — startup options](screenshots/settings.png)
![Audio settings — playback toggles and output devices](screenshots/audio.png)

## Features

- **Feed** — what the artists you follow uploaded and reposted, newest first
  (SoundCloud's own stream, tracks and playlists in one list).
- **Search** — tracks / playlists / users / albums via the public SoundCloud api-v2
  (`client_id` extracted from the SoundCloud homepage).
- **Playback** — streamed and cached directly from SoundCloud, no relay infrastructure.
- **Library** — liked tracks, playlists, followings and the following feed
  (read-only import from your SoundCloud account).
- **Downloads** — save individual tracks to a file as **M4A (AAC)**, **MP3 (320 kbps)**,
  **FLAC** or **WAV**, with cover art embedded. MP3 / FLAC / WAV are transcoded by a
  managed ffmpeg binary (system ffmpeg or a one-time auto-download).
- **Offline library** — bulk-cache your liked tracks ("Download all likes") or single
  tracks and play them with no network connection. Manage them on the **Local library**
  page; a configurable audio-cache limit keeps the regular cache in check, while
  bulk-liked downloads live in a separate folder that bypasses the limit.
- **Browsing** — tracks, playlists, users, albums, comments (read).
- **Actions** — persisted in `direct_store.json` (app data dir); most are also written
  back to SoundCloud (see Limitations): like / unlike tracks and playlists, follow /
  unfollow users, create / edit / delete playlists, comments, playback history, dislikes.
- **Flat UI** — no gradients / glow / backdrop blur, compact artwork, Spotify-style
  playing indicator.

## Limitations

- Write actions are applied to the local store (`direct_store.json`) immediately and are
  **also written to soundcloud.com** through a hidden writer WebView
  (`src-tauri/src/direct/webview.rs`): the write runs as a `fetch()` on a real
  soundcloud.com page, so it carries the signed-in web session and passes DataDome.
  Synced: likes (tracks & playlists), follows, comments (post & delete), playlist
  creation, playlist track add / reorder / delete and play history.
  Local-only: playlist metadata edits, sharing toggles (playlist & track) and dislikes.
- If SoundCloud answers with a DataDome challenge, a verification window may open;
  the local state always stands (`direct:sync-error`).
- Downloads: SoundCloud's source audio is AAC — M4A exports copy it as-is, while
  MP3 / FLAC / WAV are transcoded from that source (FLAC / WAV cannot restore quality
  beyond the original AAC).

## Differences from upstream

Backend-only features were removed together with the decorative layer:

- Discover catalog, Star / premium pages and pay flows
- SoundWave / recommendations / vibe search / clusters
- Aura palettes and decorative star fields (neutralised stubs remain so the API
  surface still compiles)
- Lyrics panel, Yandex Music import, QR session transfer, P2P call network,
  host-status banners
- Wallhaven online wallpaper search (custom image / URL wallpaper remains)
- Gradients, glow shadows, backdrop blur, decorative badges and the
  Fraunces / Unbounded display fonts (Inter + JetBrains Mono only)

## Signing in

Press "Sign in with SoundCloud" on the login screen: an in-app window opens
soundcloud.com, and once you sign in there, the session is picked up automatically
and stored locally in the app data directory (Rust session store). It is only ever
sent to SoundCloud.

## Build

Requirements (Windows): Rust (MSVC), VS Build Tools, CMake, NASM, LLVM (libclang),
Node 20+.

```sh
cd desktop
corepack pnpm install
corepack pnpm tauri dev     # development (Vite HMR)
corepack pnpm tauri build --bundles nsis   # release + NSIS installer
```

## Credits & License

Based on [zxcloli666/SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop).
MIT License — see [LICENSE](LICENSE).
