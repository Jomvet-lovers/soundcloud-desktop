# 引継ぎメモ — SCD-Direct

最終更新: 2026-10-05（セッション終了時点）

このファイルは、次のセッションのエージェント／担当者向けに「今どこまで進んでいて、何に注意するか」を
まとめたものです。開発ルールは [`desktop/AGENTS.md`](desktop/AGENTS.md) を参照。

## これは何か

- **`Jomvet-lovers/SCD-Direct`**（private）— [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)
  （MIT）の非公式改造版。開発元バックエンドに依存しない **direct モード** ビルド。
- デフォルトブランチは **`direct-mode`**（作業ブランチ）。`main` は上流ベースライン。
- `origin` = `Jomvet-lovers/SCD-Direct` / `upstream` = `zxcloli666/SoundCloud-Desktop`。
- 公開の準備ができたら、以下のコマンドで public に戻せる:
  `gh repo edit Jomvet-lovers/SCD-Direct --visibility public --accept-visibility-change-consequences`

## アカウント / 権限

- GitHub: **`Natsumil`**（`gh` 認証済み。リポジトリ admin / 組織 `Jomvet-lovers` の owner）
- `huzun1` = **write**（push 可）、`oscar269` = read
- 組織のデフォルト権限は read のため、Jomvet-lovers の全メンバーが private リポジトリを閲覧できる

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

- **書き込みの SoundCloud 同期は未解決** — DataDome の bot 保護により、非信頼クライアントからの
  書き込みは拒否される。`src-tauri/src/direct/webview.rs` の実験実装は `SYNC_ENABLED = false`。
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

1. 最新 UI で **NSIS インストーラーをビルド**して配布（必要なときに）
2. カード / 行レイアウトの残りの微調整（ユーザーの目視フィードバック対応）
3. 書き込み同期の再挑戦（優先度は低め）
4. 準備ができたら **リポジトリを公開**に切り替え

## 起動・ビルド（Windows）

```powershell
# dev（Vite HMR + debug アプリ）
cd C:\Users\natsumi\SoundCloud-Desktop\desktop
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

- dev ログの運用例: `... | Tee-Object -FilePath C:\Users\natsumi\scd-spike\dev2.log`
- DirectAPI / StaticServer / ProxyServer のポートは起動ごとに変わる（ログに表示される）
