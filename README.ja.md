[English](README.md) | [日本語](README.ja.md)

<h1 align="center">SCD-Direct（日本語）</h1>

<p align="center">
<a href="https://github.com/Jomvet-lovers/SCD-Direct/releases/latest">
<img src="https://img.shields.io/badge/ダウンロード-最新版-FF5500?style=for-the-badge" alt="Download"/>
</a>
<a href="README.md">
<img src="https://img.shields.io/badge/English-README-0066FF?style=for-the-badge" alt="English README"/>
</a>
</p>

> **非公式のサードパーティ製 SoundCloud デスクトップクライアントです。**
> 本リポジトリは
> [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop)
> （[@zxcloli666](https://github.com/zxcloli666) 氏、MIT ライセンス）を元にした
> 独立した改造版です。SoundCloud 公式とは無関係であり、公認も受けていません。
> オリジナルのクレジットはすべて上流プロジェクトに帰属します。

> [!WARNING]
> **個人利用を目的とした非公式クライアントです。自己責任でご利用ください。**
> 登録・認可された SoundCloud API クライアントではなく、Web プレイヤーと同様の方法で
> SoundCloud と通信します。利用（書き込みの同期を含む）はご自身の判断で行ってください —
> アカウントに影響が及ぶ可能性があります。詳細は SoundCloud の
> [利用規約](https://soundcloud.com/terms-of-use) を参照してください。無保証・責任を負いません。

## スクリーンショット

![ホーム — 挨拶、いいねしたトラック、再生バー](screenshots/home.png)
![検索 — ジャンルウォール](screenshots/search.png)
![ライブラリ — サウンドプリントとフォローフィード](screenshots/library.png)
![履歴 — 再生履歴](screenshots/history.png)
![設定 — 起動オプション](screenshots/settings.png)
![オーディオ設定 — 再生トグルと出力デバイス](screenshots/audio.png)

## 機能

- **フィード** フォロー中のアーティストの新着・リポストを新しい順に
  （SoundCloud 公式のストリーム。トラックとプレイリストを1つのリストで）
- **検索** トラック / プレイリスト / ユーザー / アルバム（公開 api-v2、`client_id` は
  SoundCloud トップページから取得）
- **再生** SoundCloud から直接ストリーミング＋キャッシュ（中継サーバー不要）
- **ライブラリ** いいねしたトラック、プレイリスト、フォロー一覧、フォローフィード
  （SoundCloud アカウントからの読み取りインポート）
- **ダウンロード** トラックを **M4A (AAC)** / **MP3 (320 kbps)** / **FLAC** / **WAV**
  のファイルとして保存。カバーアートを埋め込み（MP3 / FLAC / WAV は管理された ffmpeg で
  変換 — システムの ffmpeg、なければ初回のみ自動ダウンロード）
- **オフライン再生** いいねしたトラックをまとめてキャッシュ（「Download all likes」）
  または 1 曲ずつ保存し、ネットワークなしで再生できます。**ローカルライブラリ**
  ページで管理でき、オーディオキャッシュ上限でサイズを制御（いいねの一括ダウンロードは
  上限対象外の専用フォルダに保存）
- **ページ閲覧** トラック / プレイリスト / ユーザー / アルバム / コメント（読み取り）
- **操作**（`direct_store.json` に保存、多くは SoundCloud にも書き込み — 制限事項参照）—
  いいね/解除、フォロー/解除、プレイリスト作成・編集・削除、コメント、再生履歴、低評価
- **フラット UI** グラデーション・グロー・ぼかしなし、コンパクトなサムネイル、
  Spotify 風の再生インジケーター

## 制限事項

- 書き込み操作はローカルストア（`direct_store.json`）に即時反映され、**同時に
  soundcloud.com にも書き込まれます**。実際の soundcloud.com ページ上で `fetch()` を
  実行する隠し writer WebView（`src-tauri/src/direct/webview.rs`）を使うため、
  サインイン済み Web セッションの指紋で DataDome を通過できます。
  同期対象: いいね（トラック / プレイリスト）、フォロー、コメント投稿・削除、
  プレイリスト作成、プレイリストへの曲追加 / 並べ替え / 削除、再生履歴。
  ローカルのみ: プレイリストのメタデータ編集、公開範囲切替（プレイリスト / トラック）、
  低評価。
- DataDome の挑戦（captcha）が返った場合は検証ウィンドウが開くことがあります。
  解決できない場合もローカルの状態は常に維持されます（`direct:sync-error`）。
- ダウンロード: SoundCloud の配信音源は AAC です。M4A はそのまま保存され、
  MP3 / FLAC / WAV は AAC 音源からの変換です（FLAC / WAV でも AAC 以上の音質にはなりません）。

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
corepack pnpm tauri build --bundles nsis   # リリース＋NSISインストーラー
```

## クレジット / ライセンス

[zxcloli666/SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop) を
元にしています。MIT ライセンス — [LICENSE](LICENSE) を参照。
