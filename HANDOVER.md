# 引継ぎメモ — SCD-Direct

最終更新: 2026-10-05（セッション終了時点）

このファイルは、次のセッションのエージェント／担当者向けに「今どこまで進んでいて、何に注意するか」を
まとめたものです。開発ルールは [`desktop/AGENTS.md`](desktop/AGENTS.md) を参照。

## これは何か

- **`Jomvet-lovers/SCD-Direct`** — [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)
  （MIT）の非公式改造版。開発元バックエンドに依存しない **direct モード** ビルド。
- デフォルトブランチは **`direct-mode`**（作業ブランチ）。`main` は上流ベースライン。
- `origin` = `Jomvet-lovers/SCD-Direct` / `upstream` = `zxcloli666/SoundCloud-Desktop`。

## 現在の状態（完了していること）

- **direct モード本体** — 読み取りは公開 `api-v2`、書き込みはローカル（`direct_store.json`）
- 503 対策、id-only スタブ曲のハイドレート、フォロー状態チェック API、`web-profiles` クラッシュ修正
- **フラット UI 化** — 設定 / プレイリスト / ライブラリ / ユーザー / アーティストページの
  カード入れ子撤去・平坦化
- **サムネイル縮小・高密度化** — グリッド / レール / 行カバー、仮想リストの行高さ修正
- **再生行の Spotify 風表示** — `PlayingBars`（イコライザー）＋タイトルをアクセント色
- **トラックカードのボタン整理** — いいね左上 / 右上にプレイリスト・キュー・共有
  （狭いカードでは共有のみ自動非表示）
- **検索** — Recent searches ドロップダウンを不透明化、内側フォーカス枠なし、
  ブラウザ自動入力を無効化（`autoComplete="off"`）
- **ミニプレイヤー削除** — トレイのポップオーバー（`tray.html` / `src/tray/*` / Rust `popover.rs`）
  を完全撤去。トレイ左クリックはメインウィンドウ表示に変更
- **リポジトリ整備** — README バイリンガル（EN/JA）＋上流クレジット、`HANDOVER.md`（本ファイル）、
  `desktop/AGENTS.md` をこのフォーク向けに刷新

## 直近のコミット（`direct-mode`）

| commit | 内容 |
|---|---|
| `ac86b3b` | docs: rewrite README (bilingual EN/JA, standalone build) |
| `c2e2a12` | feat: remove tray mini-player popover |
| `5cb9a89` | feat(ui): flat pages, compact artwork, spotify-style playing rows |

## 既知の問題・注意点

- **書き込みは SoundCloud にも反映される** — いいね / フォロー / コメント /
  プレイリスト作成・曲追加/並べ替え/削除 / 再生履歴は隠し writer WebView
  （`webview.rs`、実際の soundcloud.com ページ上の `fetch()`）経由で書き込まれる
  （再生履歴 204・コメント synced を実測）。ローカルのみ: プレイリストの
  メタデータ編集、公開範囲切替、低評価。DataDome の挑戦時はローカルが残る。
- **インストール版アプリは古い** — 最新 UI は dev のみ反映。配布するなら NSIS ビルドが必要
  （`corepack pnpm tauri build --bundles nsis`）。ビルド前に起動中の exe をすべて閉じること。
- **dev は単一インスタンス制限** — 起動前に `soundcloud-desktop.exe`（インストール版含む）を閉じる。
- **リリース exe の起動とビルドの併用禁止** — `target\release\soundcloud-desktop.exe` を起動したまま
  ビルドすると失敗する。
- **ユーザーデータ** — `%APPDATA%\fun.natsumi.scd.direct\`。`direct_store.json`（ローカル操作）、
  `auth_session.json`（OAuth トークン）、`sc-settings.json`（設定）。消すと再ログイン。
- `CLAUDE.md`（旧・別プロジェクト向けの内容）は削除済み。

### セッション終了時に観測した問題（要調査）

- **audio-output スレッドのパニック** — 出力デバイスが取得できない環境で
  `src-tauri/src/audio/state.rs:113` が
  `no audio output device: "No audio output: Error opening the stream with the OS"` で panic し、
  その後フロントの `audio_switch_device` が `sending on a closed channel` で失敗し続ける。
  デバイス無しでも落ちない graceful なフォールバック（再検出ループ等）が望ましい。
- **`track_enforce_cache_limit` がハング** — dev ログで「Slow task still running:
  invoke:track_enforce_cache_limit」が 20 分以上続き、完了しなかった（`track_cache` 側の
  ブロッキング I/O かロック競合の疑い）。次回調査候補。

## 次の候補

1. カード / 行レイアウトの残りの微調整（ユーザーの目視フィードバック対応）
2. 書き込み同期の対象拡大（コメント等。優先度は低め）
3. README スクリーンショットの更新（UI 変更を反映する場合）

## 2026-10-07 のリファクタリング（PR #1 で `direct-mode` にマージ済み）

フォーク元の残骸を削除＋監査。方針: 直下は直接削除、Discord 維持、CI は最小1本化。

- **routes.rs 分割** — 2796行の god file を `direct/routes/` 9モジュールに分割
  （mod=振り分け＋Ctx、me/tracks/playlists/catalog/discover/local=ドメイン、
  common/normalize=共通処理）。純粋なコード移動でロジック無変更。
  検証: `cargo check` エラー0・新規警告0、`cargo test` 30件パス、
  経路84件・関数35件の保全をスクリプト照合で確認。

- **トークンログイン撤去** — ログイン画面のトークン貼り付け UI（デバッグ用）を削除し、
  アプリ内 WebView ログイン（`open_login_window`）に一本化。Rust の `direct_login`
  コマンドも除去。
- **書き込み同期の記述修正** — README（日英）・AGENTS.md の古い記述（`SYNC_ENABLED = false`
  等）を実態に合わせて修正。書き込みはローカル即時反映＋隠し writer WebView 経由で
  SoundCloud にも反映（いいね / フォロー / コメント / プレイリスト作成・曲操作 /
  再生履歴）。当初「コメント・新規プレイリストはローカルのみ」と記載していたが誤りで、
  `sync_local_comment` と POST /playlists の同期実装を確認して訂正済み。

- **削除（直下）** — `App/`（旧 RN 実験）、`bindings/`（sc-rpc）、`depens/`（5 crates の二重実装）、
  `utils/call/`（mock P2P）、`utils/decrypt/`（未参照の stub）、`docs/`（上流の露語 agent 文書5件）、
  `_refs/`（PDF/OpenAPI/ロゴ）、`*.iml`（全12件、`.gitignore` に `*.iml` 追加）
- **削除（desktop 内）** — npm 未使用 5 件（`fraunces` / `unbounded` / `geist-mono` /
  `qr-code-styling` / `react-markdown`、`pnpm-lock.yaml` 更新＋node_modules から84件 prune）、
  `GLASS_UI_GUIDE.md`（flat-UI 方針と矛盾）、`desktop/CLAUDE.md`（1行重複）
- **削除（CI）** — workflows 7本＋`.github/flatpak` を撤去し、
  `tsc` + `biome check` + `cargo check/test` の check-only `ci.yml` 1本に集約
  （対象ブランチを `main` → `direct-mode` に変更）
- **監査の結果「使用中のため保持」** — `utils/decrypt-client`（mock stub だが
  `track_cache/direct_download.rs:253` が使用）、Discord 関連（front＋Rust）、
  `aura` / `host-status` / `premium-cache` 系（配線あり）、`@dnd-kit/*` / `simple-icons`、
  Rust 依存は全件使用中（`sc-fingerprint` は direct mode の TLS 指紋クライアントで要）
- **対象外・別タスク** — CEF 用 `[patch.crates-io]`（変更リスクのため残置）、
  `direct/routes.rs`（123KB の god file、分割は別タスク化）

## 2026-10-07 の作業メモ（direct-mode）

- **再生404の修正**（`src-tauri/src/track_cache/sc_anon/mod.rs`）— `abr_sq` の恒常404を
  「音声なし」と誤判定していた問題を修正（全候補404のときのみ `Ok(None)`）。
  `invalidate_and_refresh` が30秒ゲートで古い client_id を返し続けるバグも修正。
  404では refresh しない。回帰テスト3件追加。
- **タイトル不一致の修正**（`src/lib/track-display/clean.ts`）— `stripInlineTags` が
  `(ETIA. Remix)` のようにアーティスト名入りの括弧まで削除していた問題を修正
  （ノイズのみの括弧は従来通り除去）。
- **ジャケットなし曲のフォールバック**（`src-tauri/src/direct/routes.rs`）—
  `normalize_urn` でトラック限定に `artwork_url` が null なら `user.avatar_url` を
  代入（公式Webと同等）。履歴エントリも同様。テスト4件追加。`cargo test` 全27件通過。

## 起動・ビルド（Windows）

```powershell
# dev（Vite HMR + debug アプリ）
cd <repo>\desktop
$env:Path = [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [System.Environment]::GetEnvironmentVariable('Path','User')
$env:LIBCLANG_PATH='C:\Program Files\LLVM\bin'
corepack pnpm tauri dev

# リリース（NSIS インストーラー）
corepack pnpm tauri build --bundles nsis

# チェック
npx tsc --noEmit
npx biome check --write src
cd src-tauri; cargo check
```

- dev ログの運用例: `... | Tee-Object -FilePath <log-file>.log`
- DirectAPI / StaticServer / ProxyServer のポートは起動ごとに変わる（ログに表示される）

### ARM64 Windows マシンでのビルド条件

- 既定ツールチェーン（aarch64）＋ `--target x86_64` のクロスでビルドする。
  x86_64ツールチェーンでのネイティブビルドは不可（cmake が `CMAKE_SYSTEM_PROCESSOR=ARM64`
  を返し boring-sys の NASM 分岐に入らないためリンクが壊れる）。
- 必要な環境変数（cargo 実行前に設定）:
  `$env:LIBCLANG_PATH='<LLVM ARM64>\bin'`（bindgen は
  ホスト=ARM64 用、pip の x86_64 版では不可）、
  `$env:OPUS_LIB_DIR='<registry>/audiopus_sys-0.1.8/msvc\x64'`、
  `$env:CARGO_BUILD_TARGET='x86_64-pc-windows-msvc'`（`tauri dev` 用）。
- `~/.cargo/registry/.../audiopus_sys-0.1.8/build.rs` に当機限定の1行パッチ適用済み
  （aarch64 用 `ARCHITECTURE` 定義。`OPUS_LIB_DIR` 指定時は実行時に未使用）。
  リポジトリ外の変更なので再 checkout・更新時は再適用が必要。
- pnpm は v10 を使用（v9 は `pnpm-workspace.yaml` に `packages` が無いと全コマンド失敗）。
  `npm i -g pnpm@10` で導入済み。
