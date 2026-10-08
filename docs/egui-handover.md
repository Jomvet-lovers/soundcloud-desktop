# egui移行 引継ぎ資料

別PCで作業を継続するための資料。2026-10-08 時点。
対象ブランチ: `egui-migration/wry-auth` (push 済み)。

## 0. TL;DR

- Tauri+React+Rust → Rust単体 (eframe/egui) への移行作業中。機能優先、見た目は後追い。
- backend 約11.5k行の移植・全13画面・キュー再生・テーマ基盤まで完了・検証済み。
- Phase 4 (wry ログイン/writer) は実装まで完了したが **E2E 未検証**。ここから再開する。
- ARM64 Windows 特有の環境問題を多数踏んだ。新PC (特に x64) では大半が不要な見込み。
  環境構築手順は §2、再現コマンドは §5。

## 1. ブランチ・コミット対応表

全て `egui-migration/wry-auth` に積み上げ (push 済み):

| # | message | 内容 |
|---|---|---|
| 1 | docs: add egui migration design document | 設計書 (docs/egui-migration-design.md, §8 に実施記録) |
| 2 | feat(egui): scaffold eframe app shell with CI check | egui-app 雛形 + CI |
| 3 | feat(egui): port backend logic from Tauri shell | backend 移植 + vendor |
| 4 | feat(egui): add query and image helpers with Home view | 基盤 + Home |
| 5 | feat(egui): port remaining pages with central wiring | 残り12画面 |
| 6 | feat(egui): add queue playback with auto-advance | キュー再生 |
| 7 | feat(egui): add theme foundation with shared widgets and fonts | テーマ基盤 |
| 8 | feat(egui): add wry child-process host for login and writer | Phase 4 (E2E 未検証) |

空のマーカー用ブランチが残っている (`docs/...`, `app-shell`, `backend-core`,
`player-shell`, `pages-foundation`, `pages-views`, `theme-foundation`)。
実体は全て wry-auth 上。整理してよい。

## 2. 新PC環境構築チェックリスト

前提: Windows + Rust stable (MSVC)。以下は ARM64 実績。x64 では 2-4 が不要な可能性が高い
(boring-sys ネイティブビルドが通れば prebuilt 不要)。それでも 1, 5, 6 は必要。

### 2.1 必須ツール

- [ ] VS Community 18.x (C++ ビルドツール + Windows SDK)。
  注意: BuildTools 18.9.2 には ARM64 の cl.exe が無い。Community 側を使う。
  vcvars は `C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvarsall.bat`
- [ ] CMake, Ninja (`scoop install cmake ninja` または同等)
- [ ] NASM (boring x64 アセンブリ用。ARM64 NO_ASM では不要だが入れておく)
- [ ] LLVM: ARM64 なら `scoop install llvm-arm64` (bindgen 用 libclang。
  x64 版は ARM64 プロセスにロード不可)。x64 なら `scoop install llvm`
- [ ] Node/pnpm は egui 作業には不要 (Tauri 版の保守時のみ)

### 2.2 環境変数 (永続化推奨: `setx`。`<repo>` は要置換)

```
CMAKE_GENERATOR=Ninja
CMAKE_TOOLCHAIN_FILE=<machine>\boring-noasm.cmake   (内容は §2.4)
BORING_BSSL_PATH=<machine>\deps\boringssl-arm64      (自前ビルド、§2.5)
OPUS_LIB_DIR=<repo>\egui-app\vendor\opus-arm64\lib   (リポジトリ同梱済み)
LIBCLANG_PATH=<llvm-arm64>\bin                        (例: C:\Users\<you>\scoop\apps\llvm-arm64\current\bin)
```

注意: OPUS_LIB_DIR のみリポジトリ内を指す (opus.lib 同梱済みのため再ビルド不要)。
他はマシンローカル。

### 2.3 boring-noasm.cmake (内容。そのまま保存)

```cmake
set(OPENSSL_NO_ASM YES CACHE BOOL "" FORCE)
```

背景: boring-sys2 は host==target の native ビルドで `OPENSSL_NO_ASM` を定義しない。
boring-sys は `CMAKE_TOOLCHAIN_FILE` 環境変数を cmake に転送するため、
ここで NO_ASM を注入する (ARM64 で apple-aarch64 向け perlasm を掴む問題の回避)。

### 2.4 wreq vendor (リポジトリ同梱済み。作業不要)

wreq 5.x / wreq-util 2.x は全バージョン yank 済みのため解決不可。
`egui-app/vendor/{wreq-5.3.0,wreq-util-2.2.6}` に registry キャッシュから展開し、
`[patch.crates-io]` で差し替えている (src-tauri の lock と同一版)。
CI (ubuntu) でも解決に必須のためコミット済み。触らない。

### 2.5 BoringSSL prebuilt の再現手順 (ARM64 のみ。x64 は素通りの可能性)

boring-sys の dev-profile ビルドは /MDd (Debug CRT) のため Rust (/MD 常時) と
リンクできない。Release ビルド品を prebuilt 配置する。

```powershell
# 1. cargo check を1回回し、boring-sys にパッチ適用済みソースを生成させる
#    (egui-app\target\debug\build\boring-sys2-*\out\boringssl)
# 2. out\boringssl を C:\build\boringssl-src 等へコピー
# 3. 手動ビルド (vcvarsarm64 環境、Ninja):
cmake -S C:\build\boringssl-src -B C:\build\boringssl-build -G Ninja `
  -DCMAKE_C_COMPILER=cl -DCMAKE_CXX_COMPILER=cl `
  -DOPENSSL_NO_ASM=YES -DCMAKE_BUILD_TYPE=Release `
  -DCMAKE_C_FLAGS=-MD -DCMAKE_CXX_FLAGS=-MD
cmake --build C:\build\boringssl-build --target ssl crypto --config Release
# 4. 下記レイアウトで配置 (Debug/Release 両方に同一品でよい):
#    <deps>\include\openssl\*.h        (パッチ適用済みヘッダ)
#    <deps>\build\crypto\Debug\crypto.lib
#    <deps>\build\crypto\Release\crypto.lib
#    <deps>\build\ssl\Debug\ssl.lib
#    <deps>\build\ssl\Release\ssl.lib
# 5. BORING_BSSL_PATH=<deps> を設定
```

## 3. リポジトリ内外の成果物 inventory

リポジトリ内 (git 管理):
- `egui-app/` 本体 (`src/main|state|shell|theme|widgets|query|images.rs`)
- `egui-app/src/backend/` 移植層 (`audio/track_cache/direct/network/auth/discord/app` +
  `api|models|events|paths|writer|weblogin|webhost|boot|prefs|shared`)
- `egui-app/src/views/` 13画面 + `waveform.rs`
- `egui-app/vendor/{wreq-5.3.0,wreq-util-2.2.6,opus-arm64}` (tests/examples 削減済み)
- `egui-app/assets/fonts/` (Inter OTF x4 + JetBrains Mono TTF x4)
- `egui-app/Cargo.{toml,lock}`, `.github/workflows/ci.yml` (egui job 追加済み)
- `docs/egui-migration-design.md` (§8 に実施記録)、本ファイル

リポジトリ外 (新PCで再現が必要。旧PCパス):
- `C:\Users\kota\build\boring-noasm.cmake` (§2.3 の3行)
- `C:\Users\kota\build\deps\boringssl-arm64\` (§2.5)
- `C:\Users\kota\build\{boringssl-src,boringssl-build,opus-1.5.2,opus-build,opus-*.tar.gz,tctest.c}`
- scoop パッケージ群 (ninja/llvm/llvm-arm64/nasm/cmake)
- 環境変数 (§2.2)。`%APPDATA%\soundcloud-desktop\` の実行時データ
  (auth_session.json / direct_store.json / cache / ffmpeg。実機テスト資産)

## 4. アーキテクチャ要点

- UI 状態: `AppState` (egui-app/src/state.rs)。zustand/react-query の代替。
- backend 呼出: Tauri IPC 廃止。60 commands は直接関数呼び出しに潰した。
  対応表は Phase 1 移植報告 (設計書 §8 参照…口頭引継ぎ: 各 subagent 報告は会話ログのみ。
  重要度は低いため再調査可)。
- `EventBus` (backend/events.rs) が `AppHandle`+`emit` を代替。
- writer (backend/writer.rs): 書込はローカル即時反映 + wry writer でベストエフォート同期。
  wry host 不在時は `direct:sync-error` 通知の縮退動作。
- wry host は**子プロセス** (`--wry-host <profile>`) の main スレッドで tao loop。
  親 (eframe) とは TCP JSON-RPC (backend/webhost.rs)。同一プロセス二重 loop は
  当環境で不可 (tao/winit 共に main スレッド外の window 作成不可)。
  子は親の死を検知して exit(0)。親終了時は child.kill()。
- ログイン (`--smoke-login` / Login 画面のボタン): 可視 window → cookie 監視 →
  `/me` 検証 → SessionStore 保存。手動トークン貼付も可 (LoginAction::SetToken)。
- テーマ: theme.rs + widgets.rs。accent 引数で全 view 統一 (Login のみ対象外)。

## 5. 検証コマンド (毎回 env 付きで実行)

PowerShell、リポジトリルートで。vcvars は Community 側を使うこと
(BuildTools には ARM64 の cl.exe が無い)。

```powershell
$env:CMAKE_GENERATOR='Ninja'
$env:CMAKE_TOOLCHAIN_FILE='<machine>\boring-noasm.cmake'
$env:BORING_BSSL_PATH='<machine>\deps\boringssl-arm64'
$env:OPUS_LIB_DIR="<repo>\egui-app\vendor\opus-arm64\lib"
$env:LIBCLANG_PATH='<llvm-arm64>\bin'
cmd /c '"<VS>\VC\Auxiliary\Build\vcvarsall.bat" arm64 >nul && cargo test --manifest-path egui-app/Cargo.toml'
```

smoke モード (どちらも表示環境用。CI では不可):

| コマンド | 内容 |
|---|---|
| `--smoke` | boot+サーバ+Discover+`/me/cold` |
| `--smoke <wav>` | 上記+読込・再生・位置進行確認 |
| `--smoke-login` | ログイン窓を10秒維持 (要wry子プロセス) |
| `--smoke-writer` | writer で soundcloud.com へ GET (要wry子プロセス) |
| (無引数) | GUI 起動 |
| `--smoke-tao` | tao window 作成可否の環境プローブ |

期待値: `cargo test` 38件全通 (backend 移植分 30 + queue 8)。

## 6. 未完了・既知の問題 (優先度順)

1. **Phase 4 E2E 未検証 (最優先)**: `--smoke-writer` の実行結果確認、
   ログイン窓の実アカウントでの手動テスト (サインイン→token 保存→Library 表示)。
   DataDome の振る舞いは未知数。
2. **CI (ubuntu) 未検証**: egui job (cmake/libclang-dev/libopus-dev 追加済み) が
   グリーンか不明。x64 では boring ネイティブビルドが素通る想定。要 push して確認。
3. キュー以外の残件: 無限スクロール (先頭ページのみ)、D&D 並替、mutation の一部
   (like/follow/comment 投稿は Track のみ実装)、設定の音量永続化 (PlayerState 側、
   SettingsState に無い)、track-display 高度分解、非Track カードの共通化
   (AlbumHit/ConnUser 等のローカル DTO 重複あり: search.rs / user.rs 参照)。
4. `regex` は既定 features に戻してある (src-tauri の no-default は
   unification で偶然 unicode 有効化されていた。単独では `\s` が実行時失敗)。
5. コミット時の注意: この環境の `git commit` は絵文字を自動付与する。
   回避には plumbing (`write-tree` + `commit-tree` + `update-ref` ガード付き) を使う。
   また Write ツール経由の非ASCIIコメントが稀に壊れる。**新規ファイルのコメントは
   原則 ASCII** (既存の日本語コメントは触らない)。
6. 各 bash 実行は fresh process のため `$env:` は毎回設定し直す
   (setx は通常ターミナルには効くが、エージェント実行環境には継承されない)。

## 7. 再開手順 (提案)

1. §2 の環境構築 → §5 の `cargo test` が通ることを確認。
2. `--smoke-writer` 実行 → writer 経路の成否を確認 (§6-1)。
3. 実アカウントでログイン窓テスト → Library/履歴の動作確認。
4. 緑なら CI push → ubuntu 結果を確認 (§6-2)。
5. 残件 (§6-3) を優先度順に消化。wry 安定後に Phase 5 (tray/自動更新/配布)。

## 8. x64 PC での続き (2026-10-08 追記)

引継ぎ後、x64 Windows PC で Phase 4 E2E を検証し、以下を修正・追加した
(コミット `c9f96c0` 〜 `aa2ff0a`)。

### 8.1 環境 (x64 では §2 の大半が不要)

- VS2022 BuildTools (cl 14.44) + Win SDK 26100 + CMake 4.4.3 + 同梱 Ninja +
  LLVM 23 (libclang) で `cargo test` 38 件全通。**NASM なしで boring-sys2 がビルド可**。
- `OPUS_LIB_DIR` 不要: audiopus_sys 0.1.8 同梱の `msvc/x64/opus.lib` を自動使用。
- 設定は `LIBCLANG_PATH=C:\Program Files\LLVM\bin` のみ。
- コミットの絵文字自動付与はこのPCでは再現せず (hook 等なし、通常 commit で可)。

### 8.2 Phase 4 E2E (検証済み・修正あり)

- `--smoke-writer`: **成功** (`status=200 bytes=800 captcha=false`)。DataDome チャレンジなし。
- ログイン窓: 実アカウントでサインイン成功 → `auth_session.json` 保存 →
  ローカル API `/me/cold` 200 (username 取得) を確認。
- 発見・修正した不具合:
  1. wry host が起動しない (子が `accept()`・親が `WRY_PORT=` 待ちの相互デッドロック)。
     port 通知を accept より前に移動し、accept にタイムアウトを追加。
  2. 親側で子起動失敗時に kill しておらず、孤児プロセスが exe をロックしていた。
  3. UI スレッドからの `tokio::spawn` (weblogin) がランタイム外で panic。
     Handle を渡す形に修正 (diagnostics の Linux FD モニタも同様)。
  4. eframe は入力まで再描画しないため、ログイン完了等が画面に反映されなかった。
     `EventBus::set_wake` (egui `request_repaint`) を追加し、`auth:changed` で
     `reset_views` + API セッション同期。
  5. `--smoke-login` / `--smoke-writer` 単体で smoke に入らなかった (app_main の判定漏れ)。
- UX 追加: 起動時に未ログインなら Login 画面、サイドバー上部に Sign in / Sign out、
  ログアウト後は Login 画面へ遷移。

### 8.3 CI (ubuntu) — 検証済み

- egui job の失敗は apt 不足 (`glib-2.0` / `gobject-2.0` が見つからない)。
  `libasound2-dev` `libwebkit2gtk-4.1-dev` `libgtk-3-dev` `libglib2.0-dev` を
  追加して green。以降の push はすべて CI green。

### 8.4 残件 (§6-3 の更新)

- like/follow/comment の mutation は Track like/comment 投稿のみだった (8.5 で拡充)。
- 無限スクロール・D&D 並替・音量永続化・track-display・非Trackカード共通化は未着手。
- `--smoke-login` は 10 秒固定のまま (手動サインイン検証は GUI から実施)。

### 8.5 追加修正 (同日、E2E 後)

- **再生 404 の修正**: `ApiClient::stream_url` は存在しないローカル `/stream`
  (DirectAPI は 404 を返す設計) を指しており、再生が失敗していた。
  Tauri 版 `loadTrack` と同様に `TrackCacheState::ensure_playable`
  (anon で SC api-v2 から取得) → ローカルファイル再生へ変更。
- **mutation UI**: Track like / Playlist like / 自分のコメント削除を追加。
  ボタンはフォント依存の ♥/♡ を避け、`Like` (通常) / `Liked` (アクセント塗り) で表現。
- **NowPlaying バー**: 再生中タイトル/アーティストの表示と各ページへのリンク、
  Like トグルを追加 (Track ページへの導線が無かったため)。
- **CJK 文字化けの修正**: Inter/JetBrains に日本語グリフが無く □ になっていた。
  Noto Sans JP (`assets/fonts/NotoSansJP-Regular.otf`, OFL) に加え、
  Noto Sans KR (Hangul) と Noto Sans Math (U+1D400 帯の装飾英字・ℜ 等) を同梱し、
  Proportional/Monospace/太字系のフォールバックに追加。ユーザーのライク曲タイトルで
  カバレッジ検証済み (欠け 0)。絵文字は egui 同梱の NotoEmoji でカバー。
- 検証: `cargo test` 38 件・実機で like PUT/DELETE 200 / 再生 (anon 取得 →
  transcode → 再生) / NowPlaying リンクを確認。
- **設定の永続化**: 音量 (0..=100) と HQ ストリーミングを `SettingsState`
  (`egui_ui.json`) に保存。音量は起動時にエンジンへ反映、スライダーのドラッグ終了で
  保存。HQ は `ensure_playable` に渡すようにした。
- **無限スクロール**: 共通 `Pager`/`auto_load` (`egui-app/src/pager.rs`) を新設し、
  Library と LibraryCollection、User ページの各タブを「末尾到達で自動追加読込」に
  変更 (従来の「More」/ページ送りボタンは廃止)。Search/Tag は従来の Prev/Next、
  Home の棚は先頭ページのみ (未対応)。

### 8.6 Phase 1〜2: 再生・操作系の拡充 (2026-10-09)

- 再生: リスト文脈キュー (`play_list`)、+ Next up (`insert_next`)、Mute、
  キーボードショートカット 19 種 + 一覧 (Ctrl+/)、キューの D&D 並替、
  A-B ループ (B キー/バーボタン)、EQ (11 プリセット + 10 バンド)、
  速度/ピッチ (auto/manual)、ノーマライズの**ライブ**トグル、起動ページ永続化。
- 操作系: トラック右クリックメニュー (Add/Remove from library・Not interested・
  Play next・Add to playlist・Copy link・Go to track/artist)、
  「プレイリストに追加/新規作成」ダイアログ、
  プレイリスト編集 (曲の削除・D&D 並替を `POST /playlists/:urn/tracks {order}` に同期、
  削除、公開範囲切替、pin → サイドバー Quick access、Shuffle 再生)、
  Library のいいね一括シャッフル。
- 音質メモ: 再生音源は Tauri 版と同じ anon 160k AAC (Tauri のキャッシュで実証)。
  EQ/ノーマライズ/速度は即時反映。

### 8.7 Phase 3: ダウンロード / オフライン (2026-10-09)

- **単曲ダウンロード**: Track ページ / 右クリックメニュー「Download...」→
  形式選択 (M4A (AAC) / MP3 320 / FLAC / WAV) + **ネイティブ保存ダイアログ** (`rfd`)。
  バックエンドの `export_track` (カバー埋め込み + ffmpeg トランスコード) を直接呼ぶ。
- **いいね一括 DL**: Offline ページ「Download all likes」→ いいね全件をページングで収集し
  `cache_likes` に投入 (バックグラウンド、Cancel 可)。

### 8.8 Phase 4: 連携・設定の拡充 (2026-10-09)

- **出力デバイス**: Settings → Playback にデバイス一覧 + 「Follow system default」。
  選択は `egui_ui.json` に保存し起動時に適用 (`switch_device` / `set_follow_default_output`)。
- **メディアコントロール**: souvlaki (SMTC/MPRIS) を起動。OS の再生/一時停止/次へ/前へ/
  シークを `media:*` イベントで受けて処理。メタデータは `engine::set_metadata` で送信。
- **Discord Rich Presence**: Settings → Account で有効化。再生トラック/状態の変化時のみ
  `set_activity` を送る (10 秒スロットルで再接続)。
- **Fresh drops**: Library に、フォロー中 (最大 24 人 × 6 曲) の新着を created_at 降順で
  24 件表示 (Tauri 版 `useFollowingDrops` 相当)。
- **検索履歴**: 直近 10 件を `egui_ui.json` に保存し、空クエリ時に表示・クリックで再利用。
- **認証**: 期限切れ (401) を Login 画面で「Session expired」として表示。
- **シングルインスタンス**: 名前付き mutex で GUI の二重起動を防止
  (smoke / wry 子プロセスは除外)。
- **キャッシュ上限**: 設定値を永続化し、起動時に `enforce_limit` を適用。

### 8.9 Phase 5: オートパイロット / 更新チェック (2026-10-09)

- **Autopilot**: キュー終端で関連曲 (`/tracks/:urn/related`) を取得して自動継続
  (設定 → Playback で ON/OFF、既定 ON)。
- **更新チェック**: Settings → General に「Check for updates」
  (GitHub Releases の最新タグを表示 + リンク。インストールは手動)。
- **残り**: Tray (常駐 + メニュー)。eframe のイベントループと統合が必要なため未実装
  (設計メモ: tray-icon を専用スレッドで動かし、メニューイベントを EventBus 経由で送り、
  `ctx.send_viewport_cmd` で Show/Quit を操作する)。
  任意項目の壁紙・i18n も未着手。

### 8.10 Tray / コンテキスト継続 / 細部パリティ (2026-10-10 未明)

- **Tray**: `tray-icon` を専用スレッドで動かし Win32 メッセージループを実行。
  メニュー (Show/Hide・Play/Pause・Next・Previous・Quit)、左クリックで表示切替、
  設定 General に「Close to tray」(閉じる→常駐、Quit は `force_quit` で貫通)。
  イベントは std mpsc → `poll_tray` が UI スレッドで回収。
- **SMTC 修正**: souvlaki は HWND 必須 (None で panic)。隠しトップレベル
  ウィンドウを作り `PeekMessageW` ポンプを同居させた (message-only は E_INVALIDARG)。
- **コンテキスト継続** (`state.rs`): Tauri `queue-continuation.ts` 相当。
  いいね (50/ページ)・プレイリスト (200/ページ) を再生時に arm し、キュー終端で
  次ページを読んで重複除去→追記→継続。shuffle 時は全件遅延取得→50件チャンク。
  継続ソース枯渇後の順序: コンテキスト継続 → autopilot (関連曲) → 停止。
  arm 箇所: Library/Collection の Likes 再生 (フィルタ無し時)、ShuffleLikes、
  プレイリスト Play/Shuffle。
- **shuffle 意味論**: 元実装同様「現在位置より後ろを並替 + OFF で復元
  (`original_queue`)」。next/prev は順送りのみ (乱択ではない)。
  `ShuffleLikes` はランダム開始 + 全件先読み追記 (`likes_full`)。
- **Discover 棚**: See all / Show less (先頭10件のローカル展開)。
- **Home**: Liked Tracks に「See all」→ LibraryCollection。
- **フィルタ時自動全ページ取得**: Library/Collection の Likes タブ。
- **More crates**: プレイリスト下部にキュレーターの他クレート (最大12)。
- **Track follow**: アップローダーの Follow/Following (PUT/DELETE `/me/followings/:urn`)。
- **Search**: 全タブ先読み + タブ名に件数表示。query/tab/sort 変更でページ先頭へ。
- **検証**: `cargo test` 44件全通。CI green (0e4d3221 まで確認、以降 push 済み)。
- **未検証**: Tray の実表示・メニュー操作、継続再生の実データ挙動はユーザー確認待ち。
- **見た目の再現 (フラット UI) 第1弾** (11e2f3de〜4caeeb4d):
  - Visuals: 影なし (window/popup shadow NONE)、角丸 6px、アクセントリンク、
    スライダー trailing-fill、控えめなボタン塗り。余白/ボタンパディングを高密度化。
    スクロールバーは細く非フローティング。
  - 再生中行: 左にイコライザー (自作描画アニメ) + アクセント色タイトル (背景ハイライト無し)。
  - NowPlaying: 44px アートワーク + タイトル/アーティスト + like。Prev/Play/Next は
    自作描画アイコンボタン。シーク/音量は値テキスト無しスライダー。
  - サイドバー: 210px 固定、セミボールドロゴ、全幅ナビ/クイックアクセス、診断は折り畳み。
  - Discover 棚: 150px カードのラップ格子 + ホバー再生オーバーレイ + 説明文。
  - カード/アートワークは 4px 角丸。見出しはセミボールド。主要 Play はアクセント塗り。
  - キューパネル: 28px アートワーク + 現在行アクセント。
  - 未着手 (次の見た目候補): NowPlaying の 3 分割レイアウト、モーダル/ポップオーバー調整、
    空状態・ローディング表示、アルバム/トラック/プレイリストのヒーロー詳細、
    フローティングコメントレーン (波形上のコメント表示)。
- **残り**: 見た目の再現 (フラット UI 適用) — 次のフェーズ。Vibe/Lyrics 検索は
  direct backend が空応答のスタブのため見送り (実装しても空)。
