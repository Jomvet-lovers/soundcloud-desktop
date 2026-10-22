<p align="center">
<a href="https://github.com/Jomvet-lovers/soundcloud-desktop/releases/latest">
<img src=".github/assets/logo.png" width="180px" alt="SCD-Direct" />
</a>
</p>
<h1 align="center">SoundCloud Desktop</h1>

<p align="center">
<a href="README.md">English</a> | <a href="README.ja.md">日本語</a>
</p>

<p align="center">
<a href="https://github.com/Jomvet-lovers/soundcloud-desktop/releases/latest">
<img src="https://img.shields.io/badge/Download-Latest_Version-FF5500?style=for-the-badge" alt="Download"/>
</a>
<a href="https://github.com/Jomvet-lovers/soundcloud-desktop/releases">
<img src="https://img.shields.io/github/downloads/Jomvet-lovers/soundcloud-desktop/total?style=for-the-badge&logo=download&color=FF5500&label=Downloads" alt="Downloads"/>
</a>
</p>

> [!WARNING]
> **This is an unofficial client intended for personal use only. Use it at your own risk.**
> It is not a registered or authorized SoundCloud API client; it talks to SoundCloud
> the same way the web player does. The authors and contributors accept no
> responsibility for any issues arising from the use of this client.

> This repository is a standalone modified version based on
> [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)
> by [@zxcloli666](https://github.com/zxcloli666), used under the MIT License.

## What this build is

This is a **direct-mode** build ("SCD-Direct"): it talks to SoundCloud itself
and does not need the vendor backend (`api.scnative.space`) of the upstream
project.

- **Reads** use the public `api-v2` endpoints the web player uses.
- **Writes** (likes, follows, comments, playlists, sharing, playback history)
  go through a hidden `soundcloud.com` page and land on your real account.
- **Sign-in** happens in an in-app window; the session is stored locally and
  never sent to a server other than SoundCloud.

The result is a self-contained desktop client: no account on a third-party
service, no proxy, no relay.

## Screenshots

Screenshots are from an earlier build; the layout has since been simplified.

![Home](screenshots/home.png)
![Search](screenshots/search.png)
![Library](screenshots/library.png)
![History](screenshots/history.png)
![Settings](screenshots/settings.png)
![Audio settings](screenshots/audio.png)

## Features

### Playback

- Streaming playback with queue, shuffle and repeat
- Seek everywhere (waveform, slider, comment timestamps, A-B loop)
- Playback speed and pitch control
- 10-band equalizer with presets; changing a preset switches the EQ on
- Volume normalization
- Output device picker in the player bar and in Settings: follow the system
  default or pin a specific device (persisted across restarts)
- Media keys, tray controls and Discord rich presence

### Library and discovery

- Liked tracks, playlists, following and the following feed
- Playback history: the History tab keeps a local log, and plays are reported
  to SoundCloud the way the web player does (after 15 seconds), so they also
  show up on soundcloud.com
- Search for tracks, users, playlists and albums; tag pages
- Track pages with waveform, comments (read and post) and related tracks
- Playlist editing: create, add and remove tracks, drag to reorder, delete,
  pin to the sidebar
- Artist and album pages

### Sync

- Likes, follows, comments, playlist changes and sharing states are applied to
  SoundCloud **first**; the local copy is updated only after SoundCloud accepts
- If SoundCloud refuses (for example a bot challenge) or is unreachable, the
  change is not applied locally and the UI reports the reason instead of
  showing a change that did not happen
- Likes are cached per account, so switching accounts does not mix state

### Offline and downloads

- Cache all liked tracks or single tracks and play them without a network
- Configurable storage limit (1 GB by default, Settings > Storage)
- Export tracks as M4A (AAC), MP3 (320 kbps), FLAC or WAV

### Upload

- Upload audio files with artwork, title, artist, genre, tags and sharing choice

### Customization

- Theme and accent presets
- Audio, storage, startup and Discord settings
- Optional on-screen keyboard for touch devices

## Signing in

Press **Sign in with SoundCloud** on the login screen. An in-app window opens
on soundcloud.com; once you sign in there, the session is picked up
automatically and stored in the app data directory.

The session is only ever sent to SoundCloud.

## Storage locations

- **Windows**
  - session and settings: `%APPDATA%\fun.natsumi.scd.direct`
  - caches and logs: `%LOCALAPPDATA%\fun.natsumi.scd.direct`
- **Linux**
  - session and settings: `~/.local/share/fun.natsumi.scd.direct`
  - caches and logs: `~/.cache/fun.natsumi.scd.direct`

Deleting these directories signs you out and clears the offline cache.

## Build

Requirements on Windows: Rust (MSVC toolchain), Visual Studio Build Tools,
CMake, NASM, LLVM (libclang) and Node.js 20+ (Vite 8 needs 20.19 or newer).

```sh
cd desktop
corepack pnpm install
corepack pnpm tauri dev                     # development (Vite HMR)
corepack pnpm tauri build --bundles nsis    # release + NSIS installer
```

On Linux, install the WebKitGTK build dependencies first
(`libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`,
`libasound2-dev`) and then run `corepack pnpm tauri build`.

Checks (run from `desktop/`):

```sh
npx tsc --noEmit
npx biome check src
cd src-tauri && cargo check && cargo test --lib
```

## Limitations

- Unofficial and personal-use only; SoundCloud can change its API at any time.
- Writes can be rejected by SoundCloud (bot challenge, private or removed
  content). The app reports the error and changes nothing.
- The UI is English only.
- Windows is the primary target; Linux bundles are built by CI and tested
  lightly.

## Credits & License

Based on [zxcloli666/SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop).
MIT License, see [LICENSE](LICENSE).
