# SCD-Direct — エージェント向けガイド（Tauri + React + Rust）

このリポジトリは [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)（MIT）の
非公式改造版で、開発元バックエンド（`api.scnative.space`）に依存しない **direct モード** ビルドです。
アプリは SoundCloud 本体と直接通信し、読み取りは公開 API、書き込みはローカル保存＋
隠し WebView 経由で SoundCloud にも反映します。

- **リポジトリ**: `Jomvet-lovers/SCD-Direct`（デフォルトブランチ `direct-mode`）
- **上流**: `zxcloli666/SoundCloud-Desktop`（remote `upstream`）
- **引継ぎ資料**: リポジトリ直下の [`../HANDOVER.md`](../HANDOVER.md)（現在の状態・既知の問題・次の作業）

## リポジトリとリモート

| remote | URL | 用途 |
|---|---|---|
| `origin` | `https://github.com/Jomvet-lovers/SCD-Direct.git` | 作業の push 先 |
| `upstream` | `https://github.com/zxcloli666/SoundCloud-Desktop.git` | 上流の取り込み（`git fetch upstream`） |

- ブランチ: `direct-mode`（作業・デフォルト）/ `main`（上流ベースライン）
- 作業後は必ず `git push`（`origin` へ）

## 開発ワークフロー

- **UI 確認は dev で**: `desktop/` で `corepack pnpm tauri dev`（Vite HMR → `http://localhost:1420`）。
  リリースビルドはインストーラーが必要なときだけ。
- **単一インスタンス制限**: dev を起動する前に、起動中の `soundcloud-desktop.exe`
  （インストール版含む）をすべて閉じること。閉じないと新しいインスタンスが即終了する。
- **リリースビルド**: `corepack pnpm tauri build --bundles nsis`。MSI はファイルロックで
  失敗しやすいため NSIS のみを使う。
  - **注意**: `target\release\soundcloud-desktop.exe` を起動したままにするとビルドが失敗する。
    動作確認はインストール版アプリで行う。
- **Rust のビルド環境（Windows）**: `LIBCLANG_PATH=C:\Program Files\LLVM\bin` を設定し、
  PATH を再取得してから `cargo` を実行する。
- ユーザーデータ: `%APPDATA%\fun.natsumi.scd.direct\`（`auth_session.json` / `direct_store.json` /
  `sc-settings.json` など）。消すと再ログインが必要になる。

## チェック（変更後に必ず）

- `npx tsc --noEmit` — TypeScript 型チェック
- `npx biome check --write src` — リント＋フォーマット（Biome。ESLint / Prettier は使わない）
- `cargo check`（`src-tauri/`）— Rust コンパイル確認

## 構成（このフォーク時点）

```
desktop/
  src/
    pages/        Home, Library, LibraryCollection, Search, Login, Settings,
                  OfflinePage, TrackPage, UserPage, PlaylistPage, AlbumPage, ArtistPage
    components/   layout (AppShell / Sidebar / Titlebar / NowPlayingBar / GlobalSearch),
                  ui (VirtualList / VirtualGrid / Avatar / Skeleton / CopyLinkButton …),
                  music (TrackCard / PlaylistCard / LikeButton / PlayingBars /
                         AddToPlaylistDialog / TrackStatusBadges / TrackTitleArtist …),
                  album, artist, user, library, playlist, search, settings, track,
                  auth, offline
    stores/       player, auth, auth-recovery, settings, app-status, news,
                  searchHistory, searchPrefs
    lib/          api / api-client（direct バックエンド）, audio, cache, likes, dislikes,
                  track-display, scproxy, tauri-storage, diagnostics, icons, formatters …
    i18n/locales/ en.json, ru.json, tr.json
  src-tauri/src/
    direct/       mod, routes/（HTTP ルート: mod=振り分け、me/tracks/playlists/
                  catalog/discover/local=ドメイン、common/normalize=共通処理）,
                  sc（api-v2 クライアント）, store（direct_store.json）,
                  webview（同期ライター）
    audio/        engine, decode, eq, analyser, device, media_controls, tick, timing …
    network/      proxy（scproxy://）, proxy_server, static_server, image_cache, dpi
    track_cache/  sc_anon（HLS）, direct_download, commands, state
    app/          diagnostics, tray
```

## direct モードの仕組み（重要）

- **読み取り** — 公開 `api-v2`（`client_id` を SoundCloud トップページから抽出）。
  `src-tauri/src/direct/routes/` が生データをフロント期待の形へマッピングする。
- **書き込み** — `direct_store.json` に即時ローカル保存し、隠し writer WebView
  （`webview.rs`、実際の soundcloud.com ページ上の `fetch()`）経由で soundcloud.com にも
  書き込む。同期対象: いいね / フォロー / コメント / プレイリスト作成・曲操作 /
  再生履歴。ローカルのみ: プレイリストのメタデータ編集、公開範囲切替、低評価。
  DataDome の挑戦時はローカルが残る。
- **認証** — ログイン画面の「Sign in with SoundCloud」からアプリ内ウィンドウで
  soundcloud.com にサインイン。セッションは自動取得され `auth_session.json` に保存、
  SoundCloud にのみ送信される。
- **既知の SC API の癖**:
  - システムプレイリストの `tracks` は id のみのスタブ → `/tracks?ids=` でハイドレートし、
    解決できないものは除外
  - `/playlists/{id}/tracks` は 404
  - `web-profiles` は `network` フィールドで返る（`service` にマッピング）
  - ステーション再生 API は廃止（404）→ アーティストの曲でローカルラジオを構築
  - Discover は `mixed-selections` から取得

## UI ルール（このフォーク固有）

デザインは上流の装飾層を撤去した**フラット**な見た目で統一する。

- **禁止**: グラデーション、グロー（発光系 `box-shadow`）、`backdrop-blur`、大文字マイクロラベル
- **禁止: 見出しの左に縦のアクセントバーを置かない**（例: `w-1 h-7` の色付きバー、`auraRgb` を
  使った発光バー）。見出しは文字だけで立て、区切りが要るなら余白か hairline を使う
- **角丸カードで要素を囲わない**（カードの入れ子も禁止）。セクション → 見出し → 内容を余白と
  hairline で区切る。角丸が許されるのはモーダル / ポップオーバー、ボタン、入力欄、アートワーク、
  アバター、小さなチップのみ
- **背景は solid で指定**。`index.css` の flat pass（`[style*="gradient"]` など）が inline の
  背景を消すため、gradient 前提のスタイルは使わない
- **サムネイルは小さく・高密度**（スクロール量を抑える）。行のカバーは 36–40px 目安
- **再生中の行** — Spotify 風: 背景ハイライトなし、左に `PlayingBars`（イコライザー）、
  タイトルをアクセント色
- **検索バー** — 内側のフォーカス枠なし（`input` / `textarea` / `select` の `:focus-visible` は
  無効化済み）、ブラウザ自動入力なし（`autoComplete="off"`）、Recent searches ドロップダウンは
  **不透明**
- **アクセント色** — 必ず CSS 変数 `--color-accent` / `--color-accent-hover` /
  `--color-accent-contrast` を使う。ハードコード禁止
- ページレイアウトはアルバムページを基準に統一する

## コードルール（上流から継承・要約）

### 共通
- ファイルを肥大化させない。重複を作らない。フロントは薄く、重い処理は Rust へ
- 大きなリストは `VirtualList` / `VirtualGrid`。見えていないものはレンダリングしない
- i18n: ユーザー文字列は必ず `t('key')`。`src/i18n/locales/{en,ru,tr}.json` を更新する
  （少なくとも英語とロシア語は必須）
- `localStorage` 禁止。zustand persist は `lib/tauri-storage.ts`
- Tauri invoke は `lib/diagnostics.ts` の `trackedInvoke` 経由（例外: diagnostics.ts 自身）

### React
- Zustand はセレクタで購読する（`usePlayerStore((s) => s.isPlaying)`）。`React.memo` は
  必要な箇所のみ
- 60fps の更新は DOM ref / `useSyncExternalStore`（audio の `subscribe` + `notify` パターン）。
  位置は `Math.floor()` で秒に丸めて再レンダーを抑える
- アニメーションは `transform` / `opacity` のみ（`width` / `height` / `font-size` を動かさない）

### Rust
- HTTP は **warp**（サーバー）/ **reqwest**（クライアント）のみ。tokio は単一 Runtime を
  `setup` で作って共有する
- ブロッキングは `spawn_blocking`。エラーは HTTP ステータスで返し、パニックしない
- ファイル I/O は `tokio::fs`。カスタムスキーム `scproxy://` は `lib.rs` で登録

## 参考

- [`../HANDOVER.md`](../HANDOVER.md) — 現在の状態、既知の問題、次の作業
- `../README.md` — プロジェクト概要（英語 / 日本語）
