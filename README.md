<p align="center">
<a href="https://github.com/Jomvet-lovers/soundcloud-desktop/releases/latest">
<img src=".github/assets/logo.png" width="180px" alt="SCD-Direct" />
</a>
</p>
<h1 align="center">SoundCloud Dekstop</h1>

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
> This is not a registered or authorized SoundCloud API client; it communicates with
> SoundCloud in the same way as the web player. Use it
> at your own discretion. The authors and contributors accept no responsibility for any
> issues arising from the use of this client.

> This repository is a standalone modified version based on
> [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)
> by [@zxcloli666](https://github.com/zxcloli666), used under the MIT License.

## Screenshots

![Home — greeting, liked tracks and the now-playing bar](screenshots/home.png)
![Search — genre wall](screenshots/search.png)
![Library — soundprint and following feed](screenshots/library.png)
![History — playback history](screenshots/history.png)
![Settings — startup options](screenshots/settings.png)
![Audio settings — playback toggles and output devices](screenshots/audio.png)

## Features

It inherits features from the official service and the upstream project while providing
a more refined UI and UX.

- Downloads
  - Save tracks as M4A (AAC), MP3 (320 kbps), FLAC, or WAV files.
- Offline playback
  - Cache all your liked tracks at once ("Download all likes") or save tracks
    individually and play them without a network connection.
  - Cached tracks have a storage limit, so no manual management is required.
    - The default limit is 1 GB and can be changed under Settings > Storage.
- Feed
  - View new uploads and reposts from artists you follow, newest first.
- Search
  - Tracks
  - Playlists
  - Users
  - Albums
- Library
  - Liked tracks
  - Playlists
  - Following
  - Following feed

## Signing in

Press "Sign in with SoundCloud" on the login screen to open soundcloud.com in an
in-app window.

Once you sign in, the session is retrieved automatically and stored locally in the
app data directory.

It is never sent anywhere other than SoundCloud.

## App data directory (Storage location)

- Windows
  - `%LOCALAPPDATA%\fun.natsumi.scd.direct`

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
