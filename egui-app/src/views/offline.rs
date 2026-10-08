//! Phase 3: Offline — ダウンロード済みライブラリの一覧 + 再生。
//! 対応: `desktop/src/pages/OfflinePage.tsx` (+ `desktop/src/components/offline/`)。
//! likes/未キャッシュ行・一括 DL・削除進捗は簡略化。再生はキャッシュ済み
//! ファイル直結 (`PlayFile`)。ナビゲーションは shell が処理する。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::images::Images;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum OfflineAction {
    None,
    PlayFile(String),
    Navigate(Route, Option<String>),
}

#[derive(Clone, Default)]
struct CachedRow {
    urn: String,
    bytes: u64,
    stage: String,
    liked: bool,
    path: Option<String>,
}

#[derive(Default)]
pub struct OfflineView {
    query: String,
    liked_only: bool,
    rows: Vec<CachedRow>,
    loaded: bool,
    status: Option<String>,
    /// 「Download all likes」の状態 (メッセージ)。
    bulk: crate::query::Query<String>,
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes / 1024)
    }
}

impl OfflineView {
    fn refresh(&mut self, cache: &crate::backend::track_cache::TrackCacheState) {
        let mut rows: Vec<CachedRow> = cache
            .cache_inventory()
            .into_iter()
            .map(|e| {
                let path = cache.get_cache_path(&e.urn);
                CachedRow {
                    urn: e.urn,
                    bytes: e.bytes,
                    stage: e.stage.to_string(),
                    liked: e.liked,
                    path,
                }
            })
            .collect();
        rows.sort_by(|a, b| a.urn.cmp(&b.urn));
        self.rows = rows;
        self.loaded = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        api: Option<&ApiClient>,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &mut PlayerState,
        audio: Option<&Arc<AudioState>>,
        param: Option<&str>,
        cache: Option<&crate::backend::track_cache::TrackCacheState>,
        accent: egui::Color32,
        ui: &mut egui::Ui,
    ) -> OfflineAction {
        let _ = images;
        let _ = player;
        let _ = audio;
        let _ = param;
        let _ = accent;

        let mut action = OfflineAction::None;
        let has_session = api.and_then(|a| a.session_token()).is_some();
        let api_cloned = api.cloned();

        widgets::section_header(ui, "Local library", None);
        ui.horizontal(|ui| {
            ui.label(if has_session { "online" } else { "offline" });
            if !has_session && ui.button("Sign in").clicked() {
                action = OfflineAction::Navigate(Route::Login, None);
            }
            if ui.button("Try online again").clicked() {
                action = OfflineAction::Navigate(Route::Home, None);
            }
        });

        let Some(cache) = cache else {
            ui.label("cache unavailable");
            return action;
        };

        if !self.loaded {
            self.refresh(cache);
        }
        // 一括 DL の完了回収 (完了したら在庫を再読込)。
        if self.bulk.poll() {
            if let Some(msg) = self.bulk.data.clone() {
                self.status = Some(msg);
            }
            self.refresh(cache);
        }

        let total_bytes: u64 = self.rows.iter().map(|r| r.bytes).sum();
        ui.label(format!(
            "{} files · {}",
            self.rows.len(),
            format_bytes(total_bytes),
        ));
        if let Some(status) = self.status.as_ref() {
            ui.label(status);
        }

        ui.horizontal(|ui| {
            let running = cache.cache_likes_running();
            if ui
                .add_enabled(
                    has_session && !running,
                    egui::Button::new("Download all likes"),
                )
                .clicked()
            {
                self.bulk = crate::query::Query::default();
                let api = api_cloned.clone();
                let cache = cache.clone();
                let session = api
                    .as_ref()
                    .and_then(|a| a.session_token().map(str::to_string));
                self.bulk.request(rt, async move {
                    let Some(api) = api else {
                        return Err("not signed in".to_string());
                    };
                    // いいね全件をページングで収集。
                    let mut entries = Vec::new();
                    for page in 0..20 {
                        let v = api
                            .get_json(&format!(
                                "/me/likes/tracks?limit=200&page={page}"
                            ))
                            .await?;
                        let items = v
                            .get("collection")
                            .and_then(|c| c.as_array())
                            .cloned()
                            .unwrap_or_default();
                        let n = items.len();
                        for it in items {
                            let track = it.get("track").cloned().unwrap_or(it);
                            let Some(urn) =
                                track.get("urn").and_then(|u| u.as_str())
                            else {
                                continue;
                            };
                            entries.push(
                                crate::backend::track_cache::LikeCacheEntry {
                                    urn: urn.to_string(),
                                    urls: Vec::new(),
                                    download_urls: Vec::new(),
                                    storage_urls: Vec::new(),
                                    session_id: session.clone(),
                                    hq: true,
                                    duration_ms: track
                                        .get("duration")
                                        .and_then(|d| d.as_u64()),
                                },
                            );
                        }
                        if n < 200 {
                            break;
                        }
                    }
                    let count = entries.len();
                    if count == 0 {
                        return Err("no liked tracks".to_string());
                    }
                    cache.cache_likes(entries).await?;
                    Ok(format!("Enqueued {count} liked tracks (background)"))
                });
            }
            if cache.cache_likes_running() && ui.button("Cancel").clicked() {
                cache.cancel_cache_likes();
            }
            if self.bulk.loading {
                ui.spinner();
                ui.label("Enqueueing...");
            } else if let Some(s) = self.bulk.data.as_ref() {
                ui.label(s.as_str());
            } else if let Some(e) = self.bulk.error.as_ref() {
                ui.colored_label(egui::Color32::from_rgb(255, 150, 150), e.as_str());
            }
        });

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.liked_only, "Liked only");
            ui.label("Search");
            ui.text_edit_singleline(&mut self.query);
            if ui.button("Refresh").clicked() {
                self.refresh(cache);
                self.status = None;
            }
            if ui.button("Clear cache").clicked() {
                cache.clear_cache();
                cache.clear_liked_cache();
                self.refresh(cache);
                self.status = Some("Cache cleared".to_string());
            }
        });
        ui.separator();

        if self.rows.is_empty() {
            ui.label("The cache is empty.");
            return action;
        }

        let q = self.query.trim().to_lowercase();
        let mut play: Option<String> = None;
        let mut remove: Option<String> = None;
        let mut shown = 0usize;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in self.rows.iter().filter(|r| {
                (!self.liked_only || r.liked)
                    && (q.is_empty() || r.urn.to_lowercase().contains(&q))
            }) {
                shown += 1;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(&row.urn)
                            .truncate()
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    ui.label(format_bytes(row.bytes));
                    ui.label(row.stage.as_str());
                    if row.liked {
                        ui.label("liked");
                    }
                    if ui
                        .add_enabled(row.path.is_some(), egui::Button::new("Play"))
                        .clicked()
                    {
                        if let Some(path) = row.path.clone() {
                            play = Some(path);
                        }
                    }
                    if ui.button("Remove").clicked() {
                        remove = Some(row.urn.clone());
                    }
                });
            }
        });
        if shown == 0 {
            widgets::empty_note(ui, "Nothing matches your search");
        }

        if let Some(urn) = remove {
            cache.remove_cached(&urn);
            self.refresh(cache);
            self.status = Some(format!("Removed {urn}"));
        }
        if let Some(path) = play {
            action = OfflineAction::PlayFile(path);
        }
        action
    }
}
