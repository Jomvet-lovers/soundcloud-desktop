# SCD-Direct egui 移行設計書

> 状態: 設計のみ (コード変更なし) / ブランチ: `docs/egui-migration-design`
> 前提の合意: ① UI は「機能優先・見た目は egui 流儀で妥協可」(99% ピクセル一致は追わない)
> ② ログイン/書込同期のため **wry 併用 (ハイブリッド)** を許容 ③ Node 脱却 (Rust のみでビルド)

## 1. 現状の定量把握 (direct-mode 時点)

### 1.1 フロントエンド (`desktop/src`)

| 区分 | 規模 |
|---|---|
| `.tsx` 127 ファイル / `.ts` 92 ファイル / `.css` 1 ファイル | 合計 約 25,300 行 |
| ページ (react-router v7) | 13 ルート (`Home, Search, TagPage, Library, LibraryCollection, OfflinePage, TrackPage, PlaylistPage, UserPage, ArtistPage, AlbumPage, Settings, Login`) |
| 状態管理 | zustand v5 × 9 + `lib/host-status/store.ts`、react-query v5 (サーバーキャッシュ)、Context は最小限 |
| スタイリング | Tailwind v4 + 単一 `index.css` (1,442 行)。CSS Modules なし。**ダーク専用** (ライトモードなし)、5 プリセット + custom |
| Tauri 呼び出し | `invoke` 41 箇所 (約 30 コマンド、最多は `lib/audio.ts` 26 件)、`listen` 7 件 (`audio:ended` 等) |

主要シェル: `AppShell.tsx` (キーバインド 19 種 + Queue ドロワー + ショートカットダイアログ) /
`Sidebar.tsx` / `NowPlayingBar.tsx` (262 行) / `Titlebar.tsx` (カスタム装飾) /
`QueuePanel.tsx` / `TrackContextMenu.tsx` / `EqualizerPanel.tsx` / `SoundTuningPopover.tsx` /
`VirtualList/VirtualGrid` (`@tanstack/react-virtual`) / `Modal/Pager/Skeleton` /
プレイリスト D&D 並替 (`@dnd-kit`) / ダイアログ・ポップオーバー・スライダー (`@radix-ui`) /
トースト (`sonner`) / アイコン (`lucide-react` + `lib/icons.tsx` 259 行)。

### 1.2 バックエンド (`desktop/src-tauri`)

| 区分 | 規模 |
|---|---|
| `.rs` 53 ファイル | 約 13,100 行 |
| `audio/` (rodio 自作エンジン + EQ/リサンプル/FFT/Opus) | 約 3,460 行 (26%) |
| `track_cache/` (3 層キャッシュ + transcode + direct_download + sc_anon) | 約 4,080 行 (31%) |
| `direct/` (api-v2 直読み + ローカルストア + warp ローカル API 9 ルート群) | 約 4,100 行 (31%) |
| `network/` (static/proxy/api 3 ポート + image_cache + edge) | 約 1,430 行 (11%) |
| Tauri commands | 計 60 件 (`audio` 27 / `track_cache` 18 / `auth` 4 / `discord` 4 / `network` 5 / `app/direct` 2) |

### 1.3 流用見積もり (調査結果)

- **そのまま流用可 約 80–85% (約 10.5–11.0k 行)**:
  `audio/engine+decode+resample+eq+analyser+state+types` /
  `track_cache/state+transcode+direct_download+sc_anon` /
  `direct/sc+store+routes/*` (読み系・normalize) /
  `network/edge+proxy/image/audio_route` / `auth` 永続化ロジック / `discord` 組立ロジック。
  いずれも `tauri::` に依存しない純ロジック層。
- **捨てる/書き換え 約 15–20% (約 1.8–2.3k 行)**:
  `lib.rs` の Builder/IPC 全体、`audio/commands.rs`・`track_cache/commands.rs` 等の `#[tauri::command]` 薄皮約 600 行、
  **`direct/webview.rs` (367 行) + `direct/login.rs` (214 行)** の WebView 依存部 (＝最大コスト)、
  `app.emit` 系約 200 行、tray/diagnostics のパス解決部、`tauri.conf.json`/capabilities/build/gen。

## 2. 移行アーキテクチャ (ハイブリッド案)

```
┌─ egui フロント (eframe, Rust のみ) ──────────────┐
│ AppState (zustand + react-query の代替)          │
│ Sidebar / NowPlayingBar / 各ページ / ダイアログ  │
│ 波形・仮想リスト・トースト自前実装               │
└──────────────┬──────────────────────────────────┘
               │ 直接関数呼び出し (IPC なし)
┌─ 既存ロジック層 (流用, Tauri 非依存) ───────────┐
│ audio engine / track_cache / direct routes      │
│ network edge/proxy / auth store / discord       │
└──────────────┬──────────────────────────────────┘
               │ ログイン・書込のみ
┌─ wry 専用ウィンドウ (2 用途に限定) ─────────────┐
│ ① 可視ログイン窓 (soundcloud.com/signin)        │
│ ② 不可視 writer (DataDome 回避の fetch 実行)    │
└─────────────────────────────────────────────────┘
```

要点:

- **IPC 廃止**: 60 コマンドは egui から直接 Rust 関数呼び出しに潰す。`invoke`/`listen` のシリアライズ境界が消え、`tick.rs`・`timing.rs` の `emit` は `channel` + `request_repaint` 駆動に置換する。
- **非同期実行**: `tokio::Runtime` を eframe 内で保持。react-query のキャッシュ戦略 (invalidate パターン) は自前のクエリキャッシュ (`hooks.ts` 1,201 行の移植対象) として再実装する。
- **wry の範囲限定**: 現行 `webview.rs` の SEQ+LANE 単一 writer 設計を維持し、wry ウィンドウは「cookie の取得」と「soundcloud.com 上での fetch 実行」にのみ使う。wry が無ければ動かない機能はログインと書込同期 (対象: いいね/フォロー/コメント/プレイリスト操作/再生履歴) に限定し、それ以外は完全オフライン動作可能にする。
- ** CEF パッチとの関係**: 現行 `Cargo.toml` の `[patch.crates-io] tauri feat/cef` は Tauri 由来。egui 移行後は不要になり、wry 単体 (WebView2) で足りる。

## 3. UI マッピング (機能優先・見た目妥協あり)

| 現行 | egui 方針 | 妥協点 |
|---|---|---|
| Sidebar + AppShell + Titlebar | `SidePanel` + `TopBottomPanel`。カスタムタイトルバーは `ViewportBuilder::with_decorations` 既定装飾に戻す | backdrop-blur ガラス、自由な角丸・影は捨てる |
| NowPlayingBar (再生/音量/EQ/キュー) | 下部パネルに再生・シーク・音量・シャッフル/リピート・EQ ポップアップを再実装。`useAudioClock` の 500ms ポーリングは `request_repaint` 間引きで代替 | Spotify 風インジケーターの微アニメは簡略化 |
| 波形 (`waveform.tsx` 380 div + `--sw-progress`) | 自前バー描画 + シークヒットテスト + コメントレーン。`lib/waveform.ts` の downsample ロジックは流用 | DOM 並みの滑らかなホバー演出は捨てる |
| VirtualList/VirtualGrid | `ScrollArea` + 手動仮想化 (可視範囲のみ行生成)。`hooks.ts` の `IntersectionObserver` 系は不要 | 仮想化は自前実装が必要 (中コスト) |
| Radix Dialog/Popover/Slider | `Window`/`Popup`/`Slider` 標準ウィジェットに置換 | view-transition のページ遷移アニメなし |
| dnd-kit 並替 (`SequenceList/Row`) | 上下ボタン + ドラッグの簡易自作 (または後回し) | フル D&D は Phase 分割 |
| sonner トースト | 右下オーバーレイ自作 (軽量) | — |
| 設定カード 8 種 + テーマ 5 プリセット | フォーム再実装。テーマは egui `Visuals` に映射、壁紙・blur は静止画のみ | `perf.ts` の blur 段階・`index.css` の 20 種 keyframes は捨てる |
| キーバインド 19 種 | eframe の input ハンドリングで再実装 | — |
| フォント (Inter + JetBrains Mono) | `include_bytes!` 埋込で同等維持 (オフライン安全は継続) | — |
| 画像 (アートワーク) | `image` クレート + テクスチャキャッシュ (`image_cache.rs` 流用) | `convertFileSrc`/`objectURL` 方式は廃止 |

捨てることが確定したもの: Tailwind/`index.css` 全体、ViewTransition API、backdrop-filter、装飾 keyframes、Radix/dnd-kit/sonner 等の npm 依存 17 件すべて。

## 4. 段階計画 (コードは別ブランチで)

- **Phase 0 — リポジトリ雛形** (`egui-migration/app-shell`): `egui-app/` クレート新設 (eframe + tokio)、空の `SidePanel/CentralPanel/BottomPanel` と `AppState` 骨格。CI に `cargo check` 追加。
- **Phase 1 — ロジック移植** (`egui-migration/backend-core`): `audio`・`track_cache`・`direct/sc+store+routes`・`network/edge` を Tauri 非依存化して移動。`AppHandle`/`emit` を channel 化。単体テストで回帰確認。
- **Phase 2 — 再生シェル** (`egui-migration/player-shell`): Sidebar + NowPlayingBar + 再生・シーク・音量・キュー・波形の最小再生体験。`audio:ended` 等イベントの再配線。
- **Phase 3 — ページ移植** (`egui-migration/pages-*`): Home → Search → Library → Track/Playlist/User/Artist/Album → Settings/Offline の順。各ページ単位でブランチを切る。
- **Phase 4 — ログイン/writer** (`egui-migration/wry-auth`): wry ログイン窓 + 不可視 writer + `auth_session.json` 連携 + `direct:sync-error` 相当の通知。DataDome 検証窓フローを E2E 確認。
- **Phase 5 — 仕上げ**: トレイ・メディアキー (souvlaki 継続)・Discord RPC・自動更新 (`UpdateChecker` 要再実装)・インストーラ (NSIS 相当)・Linux 対応。

工数 (AI 支援込みの相対見積り、絶対日数ではない):

| 範囲 | 規模感 |
|---|---|
| Phase 0–1 (雛形 + ロジック移植) | 小〜中 (流用 8 割のため主に外皮剥がし) |
| Phase 2 (再生シェル) | 中〜大 (egui 習熟と自作ウィジェットが支配的) |
| Phase 3 (全ページ) | 大 (25k 行 React の書き直し。最大の工数) |
| Phase 4 (wry 認証/writer) | 中 (設計は現行踏襲、検証コストあり) |
| Phase 5 (仕上げ・配布) | 中 |

## 5. リスク

1. **DataDome 迂回 (最大)**: writer を wry で再現できるかが成否分岐。失敗時は「読み専用 + 手動トークン」に縮退するフォールバックを Phase 4 に含める。
2. **見た目の期待値**: 機能優先で合意済みのため受容。ピクセル一致を求めないことを README に明記する。
3. **仮想化・大量リスト**: いいね数千件のスクロール性能は egui 自前仮想化の出来に依存。Phase 2 で先行検証する。
4. **Linux**: 現行は WebKitGTK/CEF 併持ち。egui (wgpu/glow) では別種の GPU 問題が出る可能性。CI マトリクス維持。
5. **配布サイズ・更新**: Tauri のバンドラ/Updater を失う。配布方式 (NSIS 相当・自動更新) は Phase 5 で再設計が必要。

## 6. ブランチ戦略 (本設計書以降)

- 本設計書: `docs/egui-migration-design` (このブランチ。設計書のみ、コードなし)。
- 実装開始時: `egui-migration/` プレフィクスで Phase 毎に分岐し、`direct-mode` には触れない。
  例: `egui-migration/app-shell` → `egui-migration/backend-core` → …
- `direct-mode` は現行 Tauri 版の保守線として維持。egui 版は `egui-app/` 新規ディレクトリで同居させ、PoC 期間は両ビルドが共存できる構成にする。

## 7. 未決事項

- [ ] wry ウィンドウの常駐方式 (プロセス内別スレッド vs 別プロセス)
- [ ] 自動更新の方式 (Tauri Updater 代替)
- [ ] インストーラ (NSIS 継続か、単一 exe 配布か)
- [ ] 最小対応 OS (WebView2 同梱要否 — wry のため Windows では WebView2 ランタイムが依然必要)

---

## 8. Phase 0 / Phase 1 実施記録

- ブランチ: `egui-migration/app-shell` (Phase 0) → `egui-migration/backend-core` (Phase 1)。
  `direct-mode` には触れない。コミットは Phase 完了時点では行わない方針。
- Phase 0: `egui-app/` 新設 (`scd-egui`, eframe 0.36 + tokio + serde)。
  eframe 0.36 で旧 API 廃止 (`App::update` → `App::ui`、`SidePanel` → `Panel::left` 等) のため新 API で実装。
- Phase 1: `egui-app/src/backend/` に移植完了 (約 11.5k 行)。
  `shared` / `direct` (store/sc/routes 11件。login/webview 除く) /
  `auth` / `discord` / `app::diagnostics` / `audio` (12件。commands 除く) /
  `track_cache` (6件。commands 除く) / `network` (8件)。
  `EventBus` が `AppHandle`+`emit` を代替。writer は `backend::writer` 縮退スタブ (Phase 4 まで sync-error)。
  `cargo check --all-targets` + `cargo test` (移植単体テスト 30 件) が通過。
- 落とした `#[tauri::command]` → 内部関数の対応表は各移植報告に記録済み (Phase 2 の UI 直呼び用)。

### 8.1 ビルド環境要件 (Windows ARM64 実績)

Tauri 版と異なり Node は不要。以下が追加で必要 (x86_64 では大半が不要な見込み)。

- Rust stable (MSVC) + VS Community 18.x (C++ ARM64 ツール付き)。
  注意: BuildTools 18.9.2 は ARM64 の cl.exe を含まないため不可。
  vcvars は Community 側 (`VC\Auxiliary\Build\vcvarsall.bat arm64`) を使う。
- `CMAKE_GENERATOR=Ninja` (ninja は scoop 等で導入)。
  既定の VS ジェネレータはこの環境では VCTargetsPath 検出に失敗する。
- `CMAKE_TOOLCHAIN_FILE=<repo外>/boring-noasm.cmake` (内容は `set(OPENSSL_NO_ASM YES ...)` のみ)。
  boring-sys2 は host==target の native ビルドで `OPENSSL_NO_ASM` を定義しないため、
  BoringSSL が apple-aarch64 向け perlasm を掴んで失敗する。これを回避する。
- `BORING_BSSL_PATH`: 手動 Release ビルドした BoringSSL の prebuilt 配置
  (`include/` + `build/{crypto,ssl}/{Debug,Release}/*.lib`)。
  boring-sys の dev-profile ビルドは /MDd (Debug CRT) のため Rust (常時 /MD) と
  リンクできず、Release ビルド品が必須。Debug/Release 両ディレクトリに同一品を配置。
- `OPUS_LIB_DIR`: 手動ビルドした opus 1.5.2 の static `opus.lib` 配置
  (audiopus_sys は ARM64 向け prebuilt を同梱しない)。`OPUS_LIB_DIR` が無ければ解決不可。
- `LIBCLANG_PATH`: llvm-arm64 の bin (boring-sys の bindgen 用。x64 版は不可)。
- `egui-app/vendor/`: yank 済みクレートの救済 (`wreq-5.3.0` / `wreq-util-2.2.6` を
  registry キャッシュから展開し `[patch.crates-io]` で差替え)。
  src-tauri の lock と同一版のため API 互換。CI (ubuntu) でも解決に必須。
- CI (ubuntu-latest): apt に `cmake libclang-dev libopus-dev` を追加済み
  (boring ネイティブビルド + bindgen + opus pkg-config 用)。

### 8.2 Phase 2 実施記録 (ブランチ `egui-migration/player-shell`)

- `backend/boot.rs` 新設: lib.rs `setup()` の egui 版。`ensure_dirs` → edge/proxy/image_cache/
  static+proxy サーバ → DirectState+routes(api) → SessionStore → DiscordState →
  track_cache (+ffmpeg/recover バックグラウンド) → audio (`catch_unwind` で
  デバイス無しでも起動継続) → diagnostics。tray (Phase 5) と writer (Phase 4) は起動しない。
  media_controls (souvlaki) は Windows の HWND 必須panic のため Phase 4 まで停止。
- UI: Play/Pause/Stop・シークスライダ・音量・ファイル読込 (非同期 oneshot) を
  engine 直結。`audio:ended` → 再生表示反映、`direct:sync-error` → サイドバー表示。
  再生中は `request_repaint` で位置を追従。
- `--smoke [--smoke <audio-file>]`: ヘッドレス起動確認 (CI 回帰用)。
  実機検証済み: サーバ3種起動・WAV読込 (duration 検出)・再生位置 0.49→3.00 進行・
  ffmpeg 自動取得・GUI 15秒安定起動・`cargo test` 30件通過。
- 波形・キュー・デバイス切替は Phase 3 以降。

### 8.3 Phase 3 基盤 + Home (ブランチ `egui-migration/pages-foundation`)

- 全 API 通信がローカル warp (`http://127.0.0.1:{api_port}`) 向きであることを確認。
  egui は同一エンドポイントを叩く (`backend/api.rs` の `ApiClient`)。
- 基盤: `backend/models.rs` (Track/ScUser/Paged/Discover)、`src/query.rs`
  (react-query 代替。stale-while-revalidate)、`src/images.rs` (texture キャッシュ。
  `image` 0.25 追加)、`src/views/home.rs`。
- Home: 挨拶 + いいね棚 (60件グリッド) + Discover 行。カードクリックで
  `/stream/{urn}` 直結再生 (`AppState::play_stream`)。キュー・D&D は Phase 3b。
  タイトル/アーティストの高度分解 (`track-display`) は簡略版に留める。
- `SessionStore::token()` 同期 getter を追加。ApiClient の session は毎フレーム同期。
- 実機検証: `--smoke` にデータ面チェック追加 (Discover 3件取得・`/me/cold` 401 正常)。
  副産物の修正: `regex` を既定 features に戻す (src-tauri では unification で
  偶然 unicode 有効化されていたが、単独では `\s` が実行時失敗する)。
- `cargo test` 30件通過、GUI 20秒安定起動 (Home クエリ並行・クラッシュなし)。
- 残りページ (Search/Library/Track/Playlist/User/Artist/Album/Settings/Offline/
  Tag/Login) は `egui-migration/pages-*` で分割移植する。

### 8.4 Phase 3 全ページ (ブランチ `egui-migration/pages-views`)

- 残り 11 view を 6 並列 subagent で移植: search/tag/library/collection/track/
  playlist/album/user/artist/settings/offline/login (計 `views/` 13件)。
- 統一契約: `show(api, rt, images, player, audio, param, cache, ui) -> XxxAction`
  (`None`/`PlayTrack`/`PlayFile`/`Navigate`/`SetToken`/`Logout`)。
  shell が一括配線 (`PlayTrack`→`play_stream`、`Navigate`→route+nav_param、
  `SetToken`/`Logout`→SessionStore 経由 + `reset_views`)。
- 共通追加: `models` に Playlist/Album/Comment + 寛容 `tracks_from_value`、
  `AppState::nav_param`、`play_file`、`reset_views`、`SessionStore` 据置き。
- 見送り (次バッチ以降): キュー再生 (単曲直結のみ)、D&D 並替、無限スクロール
  (先頭ページのみ)、like/follow 等 mutation の一部、設定永続化、波形シーク、
  track-display 高度分解。
- 検証: `cargo check --all-targets` 通過 (ListPage の Deserialize 境界を
  `DeserializeOwned` で修正)、`cargo test` 30件通過、GUI 25秒安定起動。

### 8.6 テーマ基盤 (ブランチ `egui-migration/theme-foundation`)

- 方針: 機能移行を止めず、見た目は共通基盤→各画面→最終仕上げの二段構え。
  egui に無い部品 (blur/アニメ等) は対象外、足りない部品は自作する。
- `src/theme.rs`: プリセット6種の背景 + `settings.accent` 駆動の Visuals。
  フォントは Inter (400/500/600/700) + JetBrains Mono を `assets/fonts/` に同梱
  (fontsource は woff のため公式配布から TTF/OTF を取得)。
  文字サイズは Style 側 (`set_style_of`)、色は Visuals 側で分担。
- `src/widgets.rs`: TrackCard/TrackRow/SectionHeader + 自作の再生グリフ
  (フォント依存回避のため painter 描画) + 再生中アクセント表示。
  全13 view が利用し、`accent` 引数で統一 (Login のみ対象外)。
- `views/waveform.rs`: `_m.json` 取得→自前バー描画→クリックシーク
  (`TrackAction::Seek`)。Track ページに統合。
- `backend/prefs.rs`: 設定の JSON 永続化 (`egui_ui.json`) + Settings の Save。
- 検証: `cargo test` 38件通過、GUI 20秒安定起動。

### 8.5 キュー再生 (ブランチ `egui-migration/queue-player`)

- `PlayerState` に queue/queue_index + next/prev/shuffle/repeat-one 計算。
  単曲再生は1件キューとして統一。ファイル再生は `file:` 仮 Track で統一。
- 終了時自動送り (`audio:ended`→repeat-one はシーク再生/他は次へ/末尾は停止)、
  3秒超え Prev は先頭戻し。再生開始で `/history` 記録 (fire-and-forget)。
- UI: Prev/Next/Shuffle/Repeat/Queue ボタン + 右 drawer (jump/remove/clear)。
- `cargo test` 38件 (queue 8件追加) 通過。smoke 再生・GUI 20秒安定を確認。
