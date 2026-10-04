# SCD-Direct

> **Unofficial personal build.** This is a standalone, modified copy of
> [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop) by
> [@zxcloli666](https://github.com/zxcloli666), used under the MIT License.
> All original credit belongs to the upstream project.

A backend-free **direct mode** build of SoundCloud Desktop (Tauri v2 + React + Rust).
Instead of relying on the original developer backend (`api.scnative.space`), the app
talks to SoundCloud itself: search and playback go straight to the public api-v2, and
your actions (likes, follows, playlists, comments, history) are stored locally.

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

- Write actions are **not synced** back to soundcloud.com. SoundCloud protects its
  write endpoints with DataDome bot protection; every non-trusted client is rejected.
  Everything stays local in `direct_store.json`. The experimental writer lives in
  `src-tauri/src/direct/webview.rs` behind `SYNC_ENABLED = false`.

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

Direct mode uses the `oauth_token` cookie of your SoundCloud web session:

1. Log in at https://soundcloud.com in your browser.
2. Open DevTools (F12) → Application → Storage → Cookies → https://soundcloud.com
3. Copy the value of the `oauth_token` cookie.
4. Paste it into the login screen and press "Sign in with token".

The token is stored locally in the app data directory (Rust session store) and is
only sent to SoundCloud.

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

- 書き込み操作は **soundcloud.com に同期されません**。SoundCloud は書き込み
  エンドポイントを DataDome の bot 保護で守っており、非信頼クライアントは拒否されます。
  すべて `direct_store.json` にローカル保存されます。実験的な同期実装は
  `src-tauri/src/direct/webview.rs` に `SYNC_ENABLED = false` で残しています。

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

direct モードでは SoundCloud Web セッションの `oauth_token` Cookie を使用します:

1. ブラウザで https://soundcloud.com にログイン
2. DevTools (F12) → Application → Storage → Cookies → https://soundcloud.com
3. `oauth_token` の値をコピー
4. ログイン画面に貼り付けて「Sign in with token」

トークンはアプリのデータディレクトリ（Rust セッションストア）にローカル保存され、
SoundCloud にのみ送信されます。

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
