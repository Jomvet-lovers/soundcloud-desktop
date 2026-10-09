//! Phase 3: Offline — ダウンロード済みライブラリ。
//! 対応: `desktop/src/pages/OfflinePage.tsx` (+ `desktop/src/components/offline/`)。
//! Likes / Cache の 2 セクション、direct store のいいねをオフライン
//! インデックスとして使うメタデータ表示、一括 DL のライブ進捗、
//! 40px アートワーク行まで Tauri と同じ構造で描く。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::oneshot;

use crate::backend::api::ApiClient;
use crate::backend::direct::DirectState;
use crate::backend::models::{Track, tracks_from_value};
use crate::backend::track_cache::{LikeCacheEntry, TrackCacheState};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route, SettingsState};
use crate::widgets::{self, UiIcon};

pub enum OfflineAction {
    None,
    /// 行の再生: フィルタ後の再生可能リストをキューにして index から。
    PlayList { tracks: Vec<Track>, index: usize },
    /// ヘッダーの再生トグル (Tauri `playAll`)。
    TogglePlayAll { tracks: Vec<Track> },
    /// ヘッダーのシャッフル (Tauri `Shuffle`)。
    ShufflePlay { tracks: Vec<Track> },
    Navigate(Route, Option<String>),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Section {
    #[default]
    Likes,
    Cached,
}

/// キャッシュ在庫の行に必要な情報 (Tauri `CacheInventoryEntry`)。
#[derive(Clone)]
struct CachedInfo {
    urn: String,
    bytes: u64,
    duration_ms: Option<u64>,
}

/// Offline の 1 行 (Tauri `OfflineEntry`)。`inv == None` = 未キャッシュのいいね。
#[derive(Clone)]
struct OfflineEntry {
    track: Track,
    inv: Option<CachedInfo>,
}

#[derive(Default)]
struct Stats {
    liked_count: usize,
    liked_cached_count: usize,
    cached_count: usize,
    total_bytes: u64,
}

pub struct OfflineView {
    /// 初回ロード (在庫 + オフラインインデックス) が済んだか。
    loaded: bool,
    section: Section,
    query: String,
    /// いいね一覧のメタデータ (オフラインインデックス相当)。
    likes: Vec<Track>,
    /// オンライン時の全件取得 (Tauri `fetchAllLikedTracks`)。
    likes_fetch: Query<Vec<Track>>,
    inventory: Vec<CachedInfo>,
    likes_entries: Vec<OfflineEntry>,
    cached_entries: Vec<OfflineEntry>,
    stats: Stats,
    /// 一括 DL の「いいね全件収集 → cache_likes」タスク。
    bulk: Query<String>,
    /// 一括 DL が走っていたか (完了検知で在庫を読み直す)。
    bulk_running: bool,
    /// 行の個別 DL (完了で在庫を読み直す)。
    downloads: Vec<oneshot::Receiver<Result<String, String>>>,
}

impl Default for OfflineView {
    fn default() -> Self {
        Self {
            loaded: false,
            section: Section::Likes,
            query: String::new(),
            likes: Vec::new(),
            likes_fetch: Query::default(),
            inventory: Vec::new(),
            likes_entries: Vec::new(),
            cached_entries: Vec::new(),
            stats: Stats::default(),
            bulk: Query::default(),
            bulk_running: false,
            downloads: Vec::new(),
        }
    }
}

fn white(a: u8) -> egui::Color32 {
    egui::Color32::from_white_alpha(a)
}

/// 対応: `desktop/src/lib/formatters.ts` の `formatBytes`。
fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1} KB", bytes as f64 / 1024.0);
    }
    if bytes < 1024 * 1024 * 1024 {
        return format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0));
    }
    format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// オフラインインデックスに無いファイルの最小 Track (Tauri `stubTrack`)。
fn stub_track(inv: &CachedInfo) -> Track {
    Track {
        urn: inv.urn.clone(),
        title: inv
            .urn
            .rsplit(':')
            .next()
            .unwrap_or(&inv.urn)
            .to_string(),
        duration: inv.duration_ms.unwrap_or(0) as i64,
        ..Default::default()
    }
}

impl OfflineView {
    fn refresh_inventory(&mut self, cache: &TrackCacheState) {
        self.inventory = cache
            .cache_inventory()
            .into_iter()
            .map(|e| CachedInfo {
                urn: e.urn,
                bytes: e.bytes,
                duration_ms: e.duration_ms,
            })
            .collect();
        self.rebuild();
    }

    /// いいね + 在庫から表示用エントリと統計を作り直す。
    fn rebuild(&mut self) {
        let inv_by_urn: HashMap<&str, &CachedInfo> = self
            .inventory
            .iter()
            .map(|e| (e.urn.as_str(), e))
            .collect();
        let track_by_urn: HashMap<&str, &Track> =
            self.likes.iter().map(|t| (t.urn.as_str(), t)).collect();

        self.likes_entries = self
            .likes
            .iter()
            .map(|t| OfflineEntry {
                track: t.clone(),
                inv: inv_by_urn.get(t.urn.as_str()).map(|e| (*e).clone()),
            })
            .collect();

        self.cached_entries = self
            .inventory
            .iter()
            .map(|e| match track_by_urn.get(e.urn.as_str()) {
                Some(t) => OfflineEntry {
                    track: (*t).clone(),
                    inv: Some(e.clone()),
                },
                None => OfflineEntry {
                    track: stub_track(e),
                    inv: Some(e.clone()),
                },
            })
            .collect();

        self.stats = Stats {
            liked_count: self.likes.len(),
            liked_cached_count: self
                .likes
                .iter()
                .filter(|t| inv_by_urn.contains_key(t.urn.as_str()))
                .count(),
            cached_count: self.inventory.len(),
            total_bytes: self.inventory.iter().map(|e| e.bytes).sum(),
        };
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        api: Option<&ApiClient>,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        cache: Option<&TrackCacheState>,
        direct: Option<&Arc<DirectState>>,
        settings: &SettingsState,
        progress: Option<(u32, u32)>,
        accent: egui::Color32,
        ui: &mut egui::Ui,
    ) -> OfflineAction {
        let mut action = OfflineAction::None;
        let has_session = api.and_then(|a| a.session_token()).is_some();
        let session = api.and_then(|a| a.session_token().map(str::to_string));

        // ── 初回ロード (Tauri: ページマウント時の load) ──
        if !self.loaded {
            if let Some(cache) = cache {
                self.refresh_inventory(cache);
            }
            let mut store_ok = direct.is_none();
            if let Some(direct) = direct {
                if let Ok(store) = direct.store.try_lock() {
                    self.likes = store
                        .liked_tracks
                        .iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect();
                    self.rebuild();
                    store_ok = true;
                }
            }
            if store_ok {
                self.loaded = true;
            } else {
                ui.ctx().request_repaint();
            }
        }

        // ── オンライン: いいね全件を取得 (Tauri `fetchAllLikedTracks`) ──
        if !self.likes_fetch.requested() {
            if let Some(api) = api.filter(|a| a.session_token().is_some()) {
                let api = api.clone();
                self.likes_fetch.request(rt, async move {
                    let mut all: Vec<Track> = Vec::new();
                    for page in 0..50u64 {
                        let v = api
                            .get_json(&format!("/me/likes/tracks?limit=200&page={page}"))
                            .await?;
                        let n = v
                            .get("collection")
                            .and_then(|c| c.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        all.extend(tracks_from_value(&v));
                        let more = v
                            .get("has_more")
                            .and_then(|b| b.as_bool())
                            .unwrap_or(false);
                        if !more || n == 0 {
                            break;
                        }
                    }
                    Ok(all)
                });
            }
        }
        if self.likes_fetch.poll() {
            if let Some(likes) = self.likes_fetch.data.take() {
                if !likes.is_empty() {
                    self.likes = likes;
                    self.rebuild();
                }
            }
        }
        if self.likes_fetch.loading {
            ui.ctx().request_repaint();
        }

        // ── 一括 DL の回収 + 進捗 (Tauri: useCacheLikes + 進捗イベント) ──
        let running = cache.map(|c| c.cache_likes_running()).unwrap_or(false);
        if self.bulk.poll() {
            if let Some(cache) = cache {
                self.refresh_inventory(cache);
            }
        }
        if self.bulk_running && !running {
            if let Some(cache) = cache {
                self.refresh_inventory(cache);
            }
        }
        self.bulk_running = running;
        if self.bulk.loading || running {
            ui.ctx().request_repaint();
        }

        // ── 個別 DL の回収 ──
        let mut download_done = false;
        self.downloads.retain_mut(|rx| match rx.try_recv() {
            Ok(_) => {
                download_done = true;
                false
            }
            Err(oneshot::error::TryRecvError::Empty) => true,
            Err(oneshot::error::TryRecvError::Closed) => {
                download_done = true;
                false
            }
        });
        if download_done {
            if let Some(cache) = cache {
                self.refresh_inventory(cache);
            }
        }
        if !self.downloads.is_empty() {
            ui.ctx().request_repaint();
        }

        // ── セクション + フィルタ ──
        let all = match self.section {
            Section::Likes => &self.likes_entries,
            Section::Cached => &self.cached_entries,
        };
        let q = self.query.trim().to_lowercase();
        let filtered: Vec<&OfflineEntry> = all
            .iter()
            .filter(|e| {
                q.is_empty()
                    || e.track.title.to_lowercase().contains(&q)
                    || e.track
                        .user
                        .as_ref()
                        .map(|u| u.username.to_lowercase().contains(&q))
                        .unwrap_or(false)
            })
            .collect();
        let playable: Vec<&OfflineEntry> =
            filtered.iter().copied().filter(|e| e.inv.is_some()).collect();
        let playable_urns: HashSet<&str> =
            playable.iter().map(|e| e.track.urn.as_str()).collect();
        let is_playing_this = player.is_playing
            && player
                .current_queued()
                .map(|t| playable_urns.contains(t.urn.as_str()))
                .unwrap_or(false);

        // ── ヘッダー (Tauri: 24px タイトル + 右クラスタ) ──
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Local library")
                    .font(crate::theme::semibold(24.0))
                    .color(white(235)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if play_all_button(ui, is_playing_this, !playable.is_empty()).clicked() {
                    action = OfflineAction::TogglePlayAll {
                        tracks: playable.iter().map(|e| e.track.clone()).collect(),
                    };
                }
                if !has_session && accent_button(ui, "Sign in", accent).clicked() {
                    action = OfflineAction::Navigate(Route::Login, None);
                }
                if !has_session && outline_button(ui, "Try online again").clicked() {
                    action = OfflineAction::Navigate(Route::Home, None);
                }
                ui.label(
                    egui::RichText::new(if has_session { "online" } else { "offline" })
                        .size(11.0)
                        .color(white(102)),
                );
            });
        });

        // ── 統計行 ──
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "{} files · {} · Likes coverage {}/{}",
                self.stats.cached_count,
                format_bytes(self.stats.total_bytes),
                self.stats.liked_cached_count,
                self.stats.liked_count,
            ))
            .size(12.0)
            .color(white(102)),
        );

        // ── ツールバー ──
        ui.add_space(16.0);
        let mut start_bulk = false;
        let mut clear_cache = false;
        ui.horizontal(|ui| {
            if section_tab(
                ui,
                &format!("Likes {}", self.stats.liked_count),
                self.section == Section::Likes,
            ) {
                self.section = Section::Likes;
            }
            if section_tab(
                ui,
                &format!("Cache {}", self.stats.cached_count),
                self.section == Section::Cached,
            ) {
                self.section = Section::Cached;
            }
            if toolbar_button(ui, UiIcon::Shuffle, "Shuffle", !playable.is_empty(), false).clicked()
            {
                action = OfflineAction::ShufflePlay {
                    tracks: playable.iter().map(|e| e.track.clone()).collect(),
                };
            }
            match self.section {
                Section::Likes => {
                    let collecting = self.bulk.loading;
                    let busy = running || collecting;
                    let label = if running {
                        progress
                            .map(|(d, t)| format!("{d}/{t}"))
                            .unwrap_or_else(|| "Collecting the list…".to_string())
                    } else if collecting {
                        "Collecting the list…".to_string()
                    } else {
                        "Download all likes".to_string()
                    };
                    if toolbar_button(
                        ui,
                        UiIcon::Download,
                        &label,
                        has_session && !busy,
                        busy,
                    )
                    .clicked()
                    {
                        start_bulk = true;
                    }
                }
                Section::Cached => {
                    let empty = self.cached_entries.is_empty();
                    if toolbar_button(ui, UiIcon::Trash, "Clear cache", !empty, false).clicked() {
                        clear_cache = true;
                    }
                }
            }
            // 右寄せの検索入力 (Tauri: ml-auto w-56 rounded-lg)。
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let frame = egui::Frame::NONE
                    .fill(white(10))
                    .stroke(egui::Stroke::new(1.0, white(20)))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 12,
                        top: 6,
                        bottom: 6,
                    });
                let search = frame.show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .frame(egui::Frame::NONE)
                            .desired_width(198.0)
                            .font(egui::FontId::proportional(12.0))
                            .text_color(white(204))
                            .hint_text(
                                egui::RichText::new("Search by title or artist").color(white(64)),
                            ),
                    )
                });
                if search.inner.has_focus() {
                    ui.painter().rect_stroke(
                        search.response.rect,
                        8.0,
                        egui::Stroke::new(1.0, white(51)),
                        egui::StrokeKind::Inside,
                    );
                }
            });
        });

        // ── 一括 DL の開始 (いいね全件収集 → cache_likes) ──
        if start_bulk {
            let api_owned = api.cloned();
            let cache_owned = cache.cloned();
            let session_bulk = session.clone();
            let hq = settings.hq_streaming;
            self.bulk = Query::default();
            self.bulk.request(rt, async move {
                let Some(api) = api_owned else {
                    return Err("not signed in".to_string());
                };
                let mut entries = Vec::new();
                for page in 0..50u64 {
                    let v = api
                        .get_json(&format!("/me/likes/tracks?limit=200&page={page}"))
                        .await?;
                    let items = v
                        .get("collection")
                        .and_then(|c| c.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let n = items.len();
                    for it in items {
                        let track = it.get("track").cloned().unwrap_or(it);
                        let Some(urn) = track.get("urn").and_then(|u| u.as_str()) else {
                            continue;
                        };
                        entries.push(LikeCacheEntry {
                            urn: urn.to_string(),
                            urls: Vec::new(),
                            download_urls: Vec::new(),
                            storage_urls: Vec::new(),
                            session_id: session_bulk.clone(),
                            hq,
                            duration_ms: track.get("duration").and_then(|d| d.as_u64()),
                        });
                    }
                    let more = v
                        .get("has_more")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    if !more || n == 0 {
                        break;
                    }
                }
                let count = entries.len();
                if count == 0 {
                    return Err("no liked tracks".to_string());
                }
                let Some(cache) = cache_owned else {
                    return Err("cache unavailable".to_string());
                };
                cache.cache_likes(entries).await?;
                Ok(format!("{count}"))
            });
        }
        if clear_cache {
            if let Some(cache) = cache {
                cache.clear_cache();
                cache.clear_liked_cache();
            }
        }

        // ── リスト ──
        ui.add_space(16.0);
        if !self.loaded {
            ui.add_space(24.0);
            ui.label(egui::RichText::new("Loading...").size(13.0).color(white(89)));
            return action;
        }
        if filtered.is_empty() {
            ui.add_space(24.0);
            let message = if !q.is_empty() {
                "Nothing matches your search"
            } else if self.section == Section::Likes {
                "No liked tracks have been cached on this device yet."
            } else {
                "The cache is empty."
            };
            ui.label(egui::RichText::new(message).size(13.0).color(white(89)));
            return action;
        }

        let mut play_req: Option<(Vec<Track>, usize)> = None;
        let mut download_req: Option<(String, i64)> = None;
        let mut remove_req: Option<String> = None;
        let current_urn = player.current_queued().map(|t| t.urn.clone());

        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for entry in &filtered {
                let is_current = current_urn.as_deref() == Some(entry.track.urn.as_str());
                let playable_index =
                    playable.iter().position(|p| p.track.urn == entry.track.urn);
                ui.add_space(8.0);
                let row = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let art_url = entry.track.artwork("t120x120");
                    let art = images.show_rounded(
                        ui,
                        rt,
                        art_url.as_deref(),
                        40.0,
                        egui::CornerRadius::same(4),
                    );
                    let art = art.on_hover_text("Play");
                    if entry.inv.is_none() {
                        ui.painter().rect_filled(
                            art.rect,
                            4.0,
                            egui::Color32::from_black_alpha(128),
                        );
                    } else if is_current && player.is_playing {
                        ui.painter().rect_filled(
                            art.rect,
                            4.0,
                            egui::Color32::from_black_alpha(128),
                        );
                        widgets::paint_transport_glyph(
                            ui.painter(),
                            egui::Rect::from_center_size(
                                art.rect.center(),
                                egui::Vec2::splat(14.0),
                            ),
                            widgets::TransportIcon::Pause,
                            white(230),
                            14.0,
                        );
                    }
                    if entry.inv.is_some() && art.clicked() {
                        if let Some(i) = playable_index {
                            play_req =
                                Some((playable.iter().map(|p| p.track.clone()).collect(), i));
                        }
                    }
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(entry.track.display_title())
                                    .size(13.0)
                                    .color(white(224)),
                            )
                            .truncate()
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                        let artist = entry
                            .track
                            .user
                            .as_ref()
                            .map(|u| u.username.clone())
                            .unwrap_or_default();
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(artist).size(11.0).color(white(102)),
                            )
                            .truncate()
                            .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        if entry.inv.is_none() {
                            if row_icon_button(ui, UiIcon::Download, "Download to device")
                                .clicked()
                            {
                                download_req =
                                    Some((entry.track.urn.clone(), entry.track.duration));
                            }
                        } else if row_icon_button(ui, UiIcon::Trash, "Remove from cache")
                            .clicked()
                        {
                            remove_req = Some(entry.track.urn.clone());
                        }
                        ui.label(
                            egui::RichText::new(widgets::fmt_ms_short(entry.track.duration))
                                .font(egui::FontId::monospace(11.0))
                                .color(white(89)),
                        );
                    });
                });
                let rect = row.response.rect;
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() + 8.0,
                    egui::Stroke::new(1.0, white(13)),
                );
                ui.add_space(8.0);
            }
        });

        if let Some((tracks, index)) = play_req {
            action = OfflineAction::PlayList { tracks, index };
        }
        if let Some(urn) = remove_req {
            if let Some(cache) = cache {
                cache.remove_cached(&urn);
                self.refresh_inventory(cache);
            }
        }
        if let Some((urn, duration)) = download_req {
            if let Some(cache) = cache {
                let (tx, rx) = oneshot::channel();
                let cache = cache.clone();
                let expected = (duration > 0).then_some(duration as u64);
                let session = session.clone();
                let hq = settings.hq_streaming;
                rt.spawn(async move {
                    let result = cache
                        .ensure_playable(&urn, session.as_deref(), hq, expected)
                        .await
                        .map(|_| urn);
                    let _ = tx.send(result);
                });
                self.downloads.push(rx);
            }
        }
        if clear_cache {
            if let Some(cache) = cache {
                self.refresh_inventory(cache);
            }
        }
        action
    }
}

/// セクションタブ (Tauri: rounded-lg px-3 py-1.5 / 選択 = 白 10% + 白 90%)。
fn section_tab(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let font = crate::theme::medium(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), white(230));
    let size = egui::vec2(galley.size().x + 24.0, galley.size().y + 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return false;
    }
    if selected {
        ui.painter().rect_filled(rect, 8.0, white(26));
    }
    let color = if selected {
        white(230)
    } else if resp.hovered() {
        white(179)
    } else {
        white(115)
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    resp.clicked()
}

/// ツールバーの操作ボタン (Tauri: rounded-lg px-3 py-1.5 / 白 60% →
/// hover 白 90% + 白 6% 背景 / 無効 = 40% 不透明)。
fn toolbar_button(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &str,
    enabled: bool,
    spinner: bool,
) -> egui::Response {
    let font = crate::theme::medium(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), white(153));
    let icon_w = 13.0;
    let gap = 6.0;
    let pad = egui::vec2(12.0, 6.0);
    let size = egui::vec2(
        galley.size().x + icon_w + gap + pad.x * 2.0,
        galley.size().y.max(icon_w) + pad.y * 2.0,
    );
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hovered = enabled && resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 8.0, white(15));
    }
    let dim = if enabled { 1.0 } else { 0.4 };
    let base = if hovered { white(230) } else { white(153) };
    let color = egui::Color32::from_rgba_unmultiplied(
        base.r(),
        base.g(),
        base.b(),
        (base.a() as f32 * dim) as u8,
    );
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + pad.x + icon_w * 0.5, rect.center().y),
        egui::Vec2::splat(icon_w),
    );
    if spinner {
        ui.put(icon_rect, egui::Spinner::new().size(13.0));
    } else {
        widgets::paint_ui_icon(ui.painter(), icon_rect, icon, color);
    }
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    ui.painter().galley(
        egui::pos2(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    resp
}

/// 「Try online again」(Tauri: 白 10% 枠 + 白 70% テキスト + hover 白 6% 背景)。
fn outline_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let font = crate::theme::medium(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), white(179));
    let size = egui::vec2(galley.size().x + 24.0, galley.size().y + 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, white(15));
    }
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(1.0, white(26)),
        egui::StrokeKind::Inside,
    );
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, white(179));
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        white(179),
    );
    resp
}

/// 「Sign in」(Tauri: アクセント背景 + 白 semibold 12px)。
fn accent_button(ui: &mut egui::Ui, label: &str, accent: egui::Color32) -> egui::Response {
    let font = crate::theme::semibold(12.0);
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        font.clone(),
        egui::Color32::WHITE,
    );
    let size = egui::vec2(galley.size().x + 24.0, galley.size().y + 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    ui.painter().rect_filled(rect, 8.0, accent);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, egui::Color32::WHITE);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        egui::Color32::WHITE,
    );
    resp
}

/// 48px 円形の再生トグル (Tauri: 白 18% 枠 + 18px グリフ + icon-pop)。
fn play_all_button(ui: &mut egui::Ui, playing: bool, enabled: bool) -> egui::Response {
    let size = 48.0;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(size), sense);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hovered = enabled && resp.hovered();
    let border = if !enabled {
        white(18)
    } else if hovered {
        white(102)
    } else {
        white(46)
    };
    ui.painter().circle_stroke(
        rect.center(),
        size * 0.5 - 0.5,
        egui::Stroke::new(1.0, border),
    );
    let t = ui
        .ctx()
        .animate_bool_with_time(egui::Id::new("offline-play-all-icon"), playing, 0.15);
    let grow = if playing { t } else { 1.0 - t };
    let scale = 0.86 + 0.14 * grow;
    let icon_color = if !enabled {
        white(92)
    } else if hovered {
        white(255)
    } else {
        white(230)
    };
    let glyph = 18.0 * scale;
    let mut center = rect.center();
    if !playing {
        // Tauri: 再生三角は ml-0.5 (右へ 2px)。
        center.x += 1.0;
    }
    widgets::paint_transport_glyph(
        ui.painter(),
        egui::Rect::from_center_size(center, egui::Vec2::splat(glyph)),
        if playing {
            widgets::TransportIcon::Pause
        } else {
            widgets::TransportIcon::Play
        },
        icon_color,
        glyph,
    );
    resp
}

/// 行末のアイコンボタン (Tauri: p-1.5 rounded / 白 40% → hover 白 80% + 白 6%)。
fn row_icon_button(ui: &mut egui::Ui, icon: UiIcon, tooltip: &str) -> egui::Response {
    let size = 26.0;
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 4.0, white(15));
    }
    let color = if hovered { white(204) } else { white(102) };
    widgets::paint_ui_icon(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(14.0)),
        icon,
        color,
    );
    resp.on_hover_text(tooltip)
}
