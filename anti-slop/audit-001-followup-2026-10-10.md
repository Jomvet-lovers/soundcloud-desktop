# anti-slop 監査 001 フォローアップ (2026-10-10)

- 対応コミット: `97d5349e` (feat(ui): offline page parity rebuild + like/stat chip fixes)。
- 承認: 監査 001 の番号 **1-16 全部**。
- 検証: `cargo check` / `cargo test` 45 件全通。実機スクリーンショットで Offline ページの
  新構造 (ヘッダー / 統計 / タブ / ツールバー / 検索 / 行 + Download・Trash アイコン) を確認。
  残りの操作 (Cache タブ・再生トグル等) はユーザー確認に委ねる。

## 実施内容 (番号順)

1. **ヘッダー再構成**: 24px semibold「Local library」+ 右クラスタ [online/offline ラベル /
   Try online again (セッション無し時のみ・白 10% 枠) / Sign in (アクセント塗り) /
   48px 円形 Play] (views/offline.rs)。Play は「一覧を再生中 → pause、現在曲が一覧内 →
   resume、他 → 先頭から」(`AppState::toggle_play_list`)。icon-pop 0.15s 付き。
2. **統計行**: `{n} files · {bytes} · Likes coverage n/m`。`format_bytes` を Tauri
   `formatters.ts` と同一 (0 B / B / KB 1桁 / MB 1桁 / GB 2桁) に。
3. **Likes / Cache セクションタブ**: 件数付き・rounded-lg・選択 = 白 10%・
   未選択 白 45% → hover 白 70%。「Liked only」「Refresh」は撤去 (Tauri に無い)。
4. **Shuffle**: 再生可能リストを並べ替えて先頭から (`AppState::shuffle_play_list`)。
5. **Download all likes**: Likes 内のみ。実行中はスピナー + `done/total`
   (`track:cache-likes-progress` イベント → `AppState.bulk_likes_progress`)、
   進捗未取得は「Collecting the list…」。Cancel は撤去。
6. **Clear cache**: Cache 内のみ・空なら無効。Refresh は撤去。
7. **検索**: 右寄せ `w-56` 入力 (白 4% 背景 + 白 8% 枠 + フォーカスで白 20%)、
   placeholder「Search by title or artist」、**title / username** で絞り込み。
8. **行**: 40px アートワーク (角丸 4・未キャッシュは 50% 暗転 + クリック不可・
   再生中は黒 50% + Pause オーバーレイ) + タイトル 13px 白 88% + アーティスト 11px 白 40% +
   モノスペース 11px 白 35% の長さ + 右端 Download / Trash (26px・hover 白 6%)。
   行は hairline (白 5%) 区切り。行クリックはフィルタ後の再生可能リストをキューに
   (`AppState::play_list_entry`、同一トラック再クリックは再読込しない)。
9. **空状態/ロード**: `Loading...` / `Nothing matches your search` /
   `No liked tracks have been cached on this device yet.` / `The cache is empty.`
10. **データ層**: オフラインインデックス = `direct_store.json` の `liked_tracks`
    (表示専用・書き込みなし)。オンライン時は `/me/likes/tracks` を 200 件/ページで
    全件取得して置換 (Tauri `fetchAllLikedTracks`)。在庫にしか無いファイルは
    `stubTrack` (urn 末尾をタイトルに) で表示。Offline を開くたびに
    `refresh_on_route_change` で在庫/いいねを読み直す (Tauri の再マウント相当)。
11. **一括 DL の進捗**: 5 に含めて実装 (ライブカウンタ)。
12. **ピル方針**: カード状ピルは不使用のまま。Offline は「行 + hairline」で再構築。
    記録として維持。
13. **Like ボタン**: テキストピルを廃止し、`paint_heart` (線/塗り) +
    `like_chip` (トラックページ: 枠付きハート 15px + 件数、accent/45 枠・accent 文字) +
    `like_ghost` (プレイリスト: 枠なし、hover 白 6%) に。pulse (560ms・心 0.3 +
    ピル 0.97・40% で色切替) を `usePulseHeart` と同じ計算で移植。
    カードのチップは 24px 円形・未いいねは**線画ハート**・いいね済みは
    アクセント 80% + コントラスト色ハート (Tauri TrackCard)。
14. **波形コメントのアバター**: 角丸 9 → 12 (24px の円)。
15. **検索カードの統計チップ**: 右下に「N plays」「M:SS」の 2 チップ
    (rounded-full・黒 35%・10px medium 白 80%)。旧: 左下の単一ピル角丸 4。
16. **カード角丸**: `track_card` / `track_card_stats` を 12 → **16px**
    (Tauri TrackCard `rounded-2xl`)。Discover カードは Tauri `DiscoverCard` が
    `rounded-xl` (12px) のため**変更なし** (照合済み)。

## 実装メモ

- 新規 `UiIcon::Download` / `UiIcon::Trash` (lucide 準拠の線画)、
  `paint_transport_glyph` を `pub(crate)` 化、`fmt_ms_short` を pub 化。
- `AppState`: `bulk_likes_progress` (イベント由来) / `toggle_play_list` /
  `shuffle_play_list` / `play_list_entry` / `refresh_on_route_change`。
- Offline の「online/offline」表示は**セッション有無**を代理指標にしている
  (egui 版に connectivity ストアが無いため)。「Try online again」は Home へ遷移。

## 残差・未対応 (監査 002 候補)

- 検索カード右上の操作群: Tauri は like が**左上**、右上は playlist / queue / share。
  egui は現状 heart / + / … の 3 つを右上に置いたまま (今回はサイズと状態のみ修正)。
- トースト通知 (sonner 相当) が無いため、Clear cache / 追加系の完了通知は出ない。
- セクション切替の view-transition アニメーションは省略 (egui)。
- TrackCard のアートワーク ring (白 6% 1px 枠) は未実装。
- 一括 DL の「失敗時トースト」等、エラー通知は未実装。
