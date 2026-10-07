# SCD-Direct

> **Unofficial personal build.** This is a standalone, modified copy of
> [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop) by
> [@zxcloli666](https://github.com/zxcloli666), used under the MIT License.
> All original credit belongs to the upstream project.

A backend-free **direct mode** build of SoundCloud Desktop (Tauri v2 + React + Rust).
Instead of relying on the original developer backend (`api.scnative.space`), the app
talks to SoundCloud itself: search and playback go straight to the public api-v2, and
your actions (likes, follows, playlists, comments, history) are stored locally.

## Screenshots

![Home — greeting, liked tracks and the now-playing bar](screenshots/home.png)
![Search — genre wall](screenshots/search.png)
![Library — soundprint and following feed](screenshots/library.png)
![History — playback history](screenshots/history.png)
![Settings — startup options](screenshots/settings.png)
![Audio settings — playback toggles and output devices](screenshots/audio.png)

## Features

- **Search** — tracks / playlists / users / albums via the public SoundCloud api-v2
  (`client_id` extracted from the SoundCloud homepage).
- **Playback** — streamed and cached directly from SoundCloud, no relay infrastructure.
- **Library** — liked tracks, playlists, followings and the following feed
  (read-only import from your SoundCloud account).
- **Browsing** — tracks, playlists, users, albums, comments (read).
- **Local actions**, persisted in `direct_store.json` (app data dir):
  like / unlike tracks and playlists, follow / unfollow users, create / edit / delete
  playlists, local comments, playback history, dislikes.
- **Flat UI** — no gradients / glow / backdrop blur, compact artwork, Spotify-style
  playing indicator.

## Limitations

- Write actions apply instantly to the local store (`direct_store.json`) and are then
  synced to soundcloud.com on a best-effort basis through a hidden writer webview
  (`src-tauri/src/direct/webview.rs`): likes, follows, playlist edits and play history
  are attempted; comments, new playlists, sharing toggles and dislikes stay local.
  SoundCloud guards its write endpoints with DataDome bot protection, so sync can be
  challenged — the local state always stands.

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
corepack pnpm tauri build   # release + installer
```

## Credits & License

Based on [zxcloli666/SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop).
MIT License — see [LICENSE](LICENSE).

---

# SCD-Direct（日本語）

> **非公式の個人ビルドです。** 本リポジトリは
> [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)
> （[@zxcloli666](https://github.com/zxcloli666) 氏、MIT ライセンス）を元にした
> 独立した改造版です。オリジナルのクレジットはすべて上流プロジェクトに帰属します。

開発元バックエンド（`api.scnative.space`）に依存しない **direct モード** ビルドです
（Tauri v2 + React + Rust）。検索・再生は公開 api-v2 に直接アクセスし、いいね・
フォロー・プレイリストなどの操作はすべてローカルに保存されます。

## スクリーンショット

![ホーム — 挨拶、いいねしたトラック、再生バー](screenshots/home.png)
![検索 — ジャンルウォール](screenshots/search.png)
![ライブラリ — サウンドプリントとフォローフィード](screenshots/library.png)
![履歴 — 再生履歴](screenshots/history.png)
![設定 — 起動オプション](screenshots/settings.png)
![オーディオ設定 — 再生トグルと出力デバイス](screenshots/audio.png)

## 機能

- **検索** — トラック / プレイリスト / ユーザー / アルバム（公開 api-v2、`client_id` は
  SoundCloud トップページから取得）
- **再生** — SoundCloud から直接ストリーミング＋キャッシュ（中継サーバー不要）
- **ライブラリ** — いいねしたトラック、プレイリスト、フォロー一覧、フォローフィード
  （SoundCloud アカウントからの読み取りインポート）
- **ページ閲覧** — トラック / プレイリスト / ユーザー / アルバム / コメント（読み取り）
- **ローカル操作**（`direct_store.json` に保存）— いいね/解除、フォロー/解除、
  プレイリスト作成・編集・削除、ローカルコメント、再生履歴、低評価
- **フラット UI** — グラデーション・グロー・ぼかしなし、コンパクトなサムネイル、
  Spotify 風の再生インジケーター

## 制限事項

- 書き込み操作はローカルストア（`direct_store.json`）に即時反映され、その後
  隠し writer WebView（`src-tauri/src/direct/webview.rs`）経由で soundcloud.com への
  同期がベストエフォートで試みられます。同期対象: いいね、フォロー、
  プレイリスト編集、再生履歴。コメント、新規プレイリスト、公開範囲切替、
  低評価はローカルのみです。SoundCloud の書き込みエンドポイントは DataDome の
  bot 保護で守られているため、同期が拒否される場合がありますが、
  ローカルの状態は常に維持されます。

## 上流からの変更（削除された機能）

- Discover カタログ、Star / プレミアムページ、課金フロー
- SoundWave / レコメンド / バイブ検索 / クラスタ
- Aura パレット、星の装飾（API 互換のためスタブは残存）
- 歌詞パネル、Yandex Music インポート、QR セッション移行、P2P 通話、
  ホスト状態バナー
- Wallhaven オンライン壁紙検索（画像 / URL 壁紙は残存）
- グラデーション、グロー、背景ぼかし、装飾バッジ、Fraunces / Unbounded フォント
  （Inter + JetBrains Mono のみ）

## サインイン

ログイン画面で「Sign in with SoundCloud」を押すと、アプリ内ウィンドウで
soundcloud.com が開きます。そこでサインインするとセッションが自動的に取得され、
アプリのデータディレクトリ（Rust セッションストア）にローカル保存されます。
SoundCloud 以外に送信されることはありません。

## ビルド

必要環境（Windows）: Rust (MSVC)、VS Build Tools、CMake、NASM、LLVM (libclang)、
Node 20+

```sh
cd desktop
corepack pnpm install
corepack pnpm tauri dev     # 開発（Vite HMR）
corepack pnpm tauri build   # リリース＋インストーラー
```

## クレジット / ライセンス

[zxcloli666/SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop) を
元にしています。MIT ライセンス — [LICENSE](LICENSE) を参照。
