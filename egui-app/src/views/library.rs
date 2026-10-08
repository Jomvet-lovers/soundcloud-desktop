//! Phase 3: Library ハブ — タブ切替 + 各タブの一覧 (無限スクロール)。
//! 対応: `desktop/src/pages/Library.tsx` (+ `LibraryCollection.tsx` の 4 タブ)。
//! SoundPrint / FreshDrops (`useFollowingDrops`) / ContinueRow の装飾は簡略表示
//! (いいね件数 + 上位ジャンル行) に留める。再生は単曲直結 (キューは Phase 3b)。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Playlist, ScUser, Track, tracks_from_value};
use crate::images::Images;
use crate::pager::Pager;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum LibraryAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (リスト全体がキューになる)。
    PlayList(Vec<Track>, usize),
    /// いいね一覧の再生 (最後まで継続ソース付き)。
    PlayLikes(Vec<Track>, usize),
    /// いいね一括シャッフル再生。
    ShuffleLikes(Vec<Track>),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    Navigate(Route, Option<String>),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum LibraryTab {
    #[default]
    Likes,
    Playlists,
    Following,
    History,
}

/// `desktop/src/lib/hooks.ts` の `HistoryEntry` に対応。
/// 履歴は `Track` ではなく `{ scTrackId, title, artistName, ... }` 形式で返る。
#[derive(Clone, Debug, Default, serde::Deserialize)]
struct HistoryEntry {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "scTrackId")]
    sc_track_id: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "artistName")]
    artist_name: String,
    #[serde(rename = "artistUrn")]
    artist_urn: Option<String>,
    #[serde(rename = "artworkUrl")]
    artwork_url: Option<String>,
    #[serde(default)]
    duration: i64,
    #[serde(rename = "playedAt")]
    played_at: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
struct HistoryPage {
    #[serde(default)]
    collection: Vec<HistoryEntry>,
}

/// 対応: `desktop/src/components/library/history-utils.ts` の `historyTrackUrn`。
fn history_track_urn(sc_track_id: &str) -> String {
    if sc_track_id.starts_with("soundcloud:tracks:") {
        sc_track_id.to_string()
    } else {
        format!("soundcloud:tracks:{sc_track_id}")
    }
}

/// 対応: `history-utils.ts` の `historyEntryToTrack`。
fn history_entry_to_track(entry: &HistoryEntry) -> Track {
    Track {
        id: 0,
        urn: history_track_urn(&entry.sc_track_id),
        title: entry.title.clone(),
        duration: entry.duration,
        artwork_url: entry.artwork_url.clone(),
        permalink_url: None,
        waveform_url: None,
        genre: None,
        playback_count: None,
        likes_count: None,
        comment_count: None,
        user: Some(ScUser {
            id: 0,
            urn: entry.artist_urn.clone().unwrap_or_default(),
            username: entry.artist_name.clone(),
            avatar_url: None,
            permalink_url: None,
        }),
        user_favorite: None,
        created_at: None,
    }
}

/// SoundPrint の簡略表示: 件数 + 上位ジャンル (装飾集計は `useSoundprint` 由来)。
fn soundprint_line(tracks: &[Track]) -> String {
    if tracks.is_empty() {
        return "No liked tracks yet".to_string();
    }
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for t in tracks {
        if let Some(g) = t.genre.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
            *counts.entry(g).or_insert(0) += 1;
        }
    }
    let mut top: Vec<(&str, usize)> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    top.truncate(3);
    if top.is_empty() {
        format!("{} liked tracks", tracks.len())
    } else {
        let genres = top
            .iter()
            .map(|(g, n)| format!("{g} x{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} liked tracks — {genres}", tracks.len())
    }
}

fn fmt_duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

#[derive(Default)]
pub struct LibraryView {
    tab: LibraryTab,
    filter: String,
    likes: Pager<Track>,
    my_playlists: Pager<Playlist>,
    liked_playlists: Pager<Playlist>,
    followings: Pager<ScUser>,
    history: Query<Vec<HistoryEntry>>,
    /// Fresh drops (フォロー中ユーザの新着を合成)。
    fresh: Query<Vec<Track>>,
}

impl LibraryView {
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
    ) -> LibraryAction {
        let _ = audio;
        let _ = param;
        let _ = cache;
        let Some(api) = api else {
            ui.label("backend not running");
            return LibraryAction::None;
        };
        let api_owned = api.clone();
        self.likes.ensure_page(rt, api, "/me/likes/tracks", 50);
        self.my_playlists.ensure_page(rt, api, "/me/playlists", 50);
        self.liked_playlists
            .ensure_page(rt, api, "/me/likes/playlists", 50);
        self.followings.ensure_page(rt, api, "/me/followings", 50);
        if !self.history.requested() {
            let api = api_owned.clone();
            self.history.request(rt, async move {
                let v = api.get_json("/history?limit=50").await?;
                let page: HistoryPage = serde_json::from_value(v).map_err(|e| e.to_string())?;
                Ok(page.collection)
            });
        }
        // Fresh drops: フォロー中 (最大24人) の最近のアップロードを合成し
        // created_at 降順で 24 件 (Tauri 版 `useFollowingDrops` 相当)。
        if !self.fresh.requested() && !self.followings.items.is_empty() {
            let api = api_owned.clone();
            let targets: Vec<String> = self
                .followings
                .items
                .iter()
                .take(24)
                .map(|u| u.urn.clone())
                .collect();
            self.fresh.request(rt, async move {
                let mut seen = std::collections::HashSet::new();
                let mut merged: Vec<Track> = Vec::new();
                for urn in targets {
                    let path =
                        format!("/users/{}/tracks?limit=6&page=0", urlencoding::encode(&urn));
                    if let Ok(v) = api.get_json(&path).await {
                        for t in tracks_from_value(&v) {
                            if seen.insert(t.urn.clone()) {
                                merged.push(t);
                            }
                        }
                    }
                }
                merged.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                merged.truncate(24);
                Ok(merged)
            });
        }

        let mut changed = self.likes.poll();
        changed |= self.my_playlists.poll();
        changed |= self.liked_playlists.poll();
        changed |= self.followings.poll();
        changed |= self.history.poll();
        changed |= self.fresh.poll();
        if changed {
            ui.ctx().request_repaint();
        }

        let mut action = LibraryAction::None;

        ui.heading("Library");
        if !self.likes.items.is_empty() {
            ui.label(soundprint_line(&self.likes.items));
        }

        // Fresh drops (フォロー中ユーザの新着)。Tauri 版 `FreshDrops` 相当。
        if let Some(fresh) = self.fresh.data.as_ref() {
            if !fresh.is_empty() {
                widgets::section_header(ui, "Fresh drops", Some(fresh.len()));
                egui::ScrollArea::vertical()
                    .id_salt("library:fresh")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for (i, track) in fresh.iter().enumerate() {
                            match Self::track_row(ui, rt, images, player, track, accent) {
                                widgets::RowHit::Clicked => {
                                    action = LibraryAction::PlayList(fresh.clone(), i);
                                }
                                widgets::RowHit::Menu => {
                                    action = LibraryAction::OpenMenu(track.clone());
                                }
                                widgets::RowHit::None => {}
                            }
                        }
                    });
                ui.separator();
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, LibraryTab::Likes, "Likes");
            ui.selectable_value(&mut self.tab, LibraryTab::Playlists, "Playlists");
            ui.selectable_value(&mut self.tab, LibraryTab::Following, "Following");
            ui.selectable_value(&mut self.tab, LibraryTab::History, "History");
        });
        // React 版 (`LibraryCollection.tsx`) と同様、履歴タブにフィルタは無い。
        if self.tab != LibraryTab::History {
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.text_edit_singleline(&mut self.filter);
            });
        }
        ui.separator();

        let needle = self.filter.trim().to_lowercase();

        match self.tab {
            LibraryTab::Likes => {
                ui.horizontal(|ui| {
                    widgets::section_header(ui, "Liked Tracks", Some(self.likes.items.len()));
                    if ui.small_button("See all").clicked() {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("likes".to_string()),
                        );
                    }
                    if ui.small_button("Shuffle").clicked() && !self.likes.items.is_empty() {
                        action = LibraryAction::ShuffleLikes(self.likes.items.clone());
                    }
                });
                if self.likes.q.loading && self.likes.items.is_empty() {
                    widgets::loading(ui);
                } else if let Some(err) = self.likes.q.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Likes unavailable: {err}"),
                    );
                } else if !self.likes.items.is_empty() {
                    let rows: Vec<Track> = self
                        .likes
                        .items
                        .iter()
                        .filter(|t| {
                            needle.is_empty()
                                || t.title.to_lowercase().contains(&needle)
                                || t.artist_name().to_lowercase().contains(&needle)
                        })
                        .cloned()
                        .collect();
                    if rows.is_empty() {
                        widgets::empty_note(ui, "No liked tracks yet");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("library:likes")
                            .show(ui, |ui| {
                                for (i, track) in rows.iter().enumerate() {
                                    match Self::track_row(ui, rt, images, player, track, accent) {
                                        widgets::RowHit::Clicked => {
                                            // フィルタ無しのみ「いいね最後まで」継続。
                                            action = if needle.is_empty() {
                                                LibraryAction::PlayLikes(rows.clone(), i)
                                            } else {
                                                LibraryAction::PlayList(rows.clone(), i)
                                            };
                                        }
                                        widgets::RowHit::Menu => {
                                            action = LibraryAction::OpenMenu(track.clone());
                                        }
                                        widgets::RowHit::None => {}
                                    }
                                }
                                if crate::pager::auto_load(
                                    ui,
                                    self.likes.q.loading,
                                    self.likes.has_more,
                                ) {
                                    self.likes.fetch_page(rt, api, "/me/likes/tracks", 50);
                                }
                            });
                        // フィルタ中は一致漏れを防ぐため残りページを自動取得する
                        // (Tauri: LikesTab の Auto-fetch remaining pages)。
                        if !needle.is_empty() && self.likes.has_more && !self.likes.q.loading {
                            self.likes.fetch_page(rt, api, "/me/likes/tracks", 50);
                        }
                    }
                }
            }
            LibraryTab::Playlists => {
                ui.horizontal(|ui| {
                    widgets::section_header(ui, "Playlists", None);
                    if ui.small_button("See all").clicked() {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("playlists".to_string()),
                        );
                    }
                });
                if (self.my_playlists.q.loading && self.my_playlists.items.is_empty())
                    || (self.liked_playlists.q.loading && self.liked_playlists.items.is_empty())
                {
                    widgets::loading(ui);
                } else {
                    if !self.my_playlists.items.is_empty() {
                        let rows: Vec<Playlist> = self
                            .my_playlists
                            .items
                            .iter()
                            .filter(|p| {
                                needle.is_empty() || p.title.to_lowercase().contains(&needle)
                            })
                            .cloned()
                            .collect();
                        if !rows.is_empty() {
                            widgets::section_header(ui, "Your Playlists", Some(rows.len()));
                            egui::ScrollArea::vertical()
                                .id_salt("library:my-playlists")
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    for p in &rows {
                                        if Self::playlist_row(ui, rt, images, p) {
                                            action = LibraryAction::Navigate(
                                                Route::Playlist,
                                                Some(p.urn.clone()),
                                            );
                                        }
                                    }
                                    if crate::pager::auto_load(
                                        ui,
                                        self.my_playlists.q.loading,
                                        self.my_playlists.has_more,
                                    ) {
                                        self.my_playlists.fetch_page(rt, api, "/me/playlists", 50);
                                    }
                                });
                        }
                    }
                    if !self.liked_playlists.items.is_empty() {
                        let rows: Vec<Playlist> = self
                            .liked_playlists
                            .items
                            .iter()
                            .filter(|p| {
                                needle.is_empty() || p.title.to_lowercase().contains(&needle)
                            })
                            .cloned()
                            .collect();
                        if !rows.is_empty() {
                            widgets::section_header(ui, "Liked Playlists", Some(rows.len()));
                            egui::ScrollArea::vertical()
                                .id_salt("library:liked-playlists")
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    for p in &rows {
                                        if Self::playlist_row(ui, rt, images, p) {
                                            action = LibraryAction::Navigate(
                                                Route::Playlist,
                                                Some(p.urn.clone()),
                                            );
                                        }
                                    }
                                    if crate::pager::auto_load(
                                        ui,
                                        self.liked_playlists.q.loading,
                                        self.liked_playlists.has_more,
                                    ) {
                                        self.liked_playlists.fetch_page(
                                            rt,
                                            api,
                                            "/me/likes/playlists",
                                            50,
                                        );
                                    }
                                });
                        }
                    }
                }
            }
            LibraryTab::Following => {
                ui.horizontal(|ui| {
                    widgets::section_header(ui, "Following", Some(self.followings.items.len()));
                    if ui.small_button("See all").clicked() {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("following".to_string()),
                        );
                    }
                });
                if self.followings.q.loading && self.followings.items.is_empty() {
                    widgets::loading(ui);
                } else if let Some(err) = self.followings.q.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Following unavailable: {err}"),
                    );
                } else if !self.followings.items.is_empty() {
                    let rows: Vec<ScUser> = self
                        .followings
                        .items
                        .iter()
                        .filter(|u| {
                            needle.is_empty() || u.username.to_lowercase().contains(&needle)
                        })
                        .cloned()
                        .collect();
                    if rows.is_empty() {
                        widgets::empty_note(ui, "You are not following anyone");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("library:following")
                            .show(ui, |ui| {
                                for u in &rows {
                                    if Self::user_row(ui, rt, images, u) {
                                        action = LibraryAction::Navigate(
                                            Route::User,
                                            Some(u.urn.clone()),
                                        );
                                    }
                                }
                                if crate::pager::auto_load(
                                    ui,
                                    self.followings.q.loading,
                                    self.followings.has_more,
                                ) {
                                    self.followings.fetch_page(rt, api, "/me/followings", 50);
                                }
                            });
                    }
                }
            }
            LibraryTab::History => {
                ui.horizontal(|ui| {
                    widgets::section_header(ui, "History", None);
                    if ui.small_button("See all").clicked() {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("history".to_string()),
                        );
                    }
                });
                if self.history.loading && self.history.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(err) = self.history.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("History unavailable: {err}"),
                    );
                } else if let Some(entries) = self.history.data.clone() {
                    if entries.is_empty() {
                        widgets::empty_note(ui, "No listening history");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("library:history")
                            .show(ui, |ui| {
                                let tracks: Vec<Track> =
                                    entries.iter().map(history_entry_to_track).collect();
                                for (i, entry) in entries.iter().enumerate() {
                                    match Self::history_row(ui, rt, images, player, entry) {
                                        widgets::RowHit::Clicked => {
                                            action = LibraryAction::PlayList(tracks.clone(), i);
                                        }
                                        widgets::RowHit::Menu => {
                                            action = LibraryAction::OpenMenu(
                                                history_entry_to_track(entry),
                                            );
                                        }
                                        widgets::RowHit::None => {}
                                    }
                                }
                            });
                    }
                }
            }
        }

        action
    }

    /// 1トラック行。戻り値は再生クリックされたか (対応: `LibraryTrackRow.tsx` 簡略)。
    fn track_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        track: &Track,
        accent: egui::Color32,
    ) -> widgets::RowHit {
        let playing = widgets::is_currently_playing(player, track);
        let dur = fmt_duration(track.duration);
        widgets::hit_of(&widgets::track_row(
            ui,
            rt,
            images,
            track,
            playing,
            accent,
            Some(&dur),
        ))
    }

    /// 1プレイリスト行。戻り値は遷移クリックされたか。
    fn playlist_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        playlist: &Playlist,
    ) -> bool {
        let mut clicked = false;
        ui.horizontal(|ui| {
            let art = playlist.artwork("t200x200");
            if images.show(ui, rt, art.as_deref(), 48.0).clicked() {
                clicked = true;
            }
            ui.vertical(|ui| {
                ui.set_max_width(320.0);
                ui.add(
                    egui::Label::new(&playlist.title)
                        .truncate()
                        .wrap_mode(egui::TextWrapMode::Truncate),
                );
                if let Some(user) = playlist.user.as_ref() {
                    ui.label(&user.username);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Open").clicked() {
                    clicked = true;
                }
            });
        });
        clicked
    }

    /// 1ユーザ行。戻り値は遷移クリックされたか (対応: `UserCard.tsx` 簡略)。
    fn user_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        user: &ScUser,
    ) -> bool {
        let mut clicked = false;
        ui.horizontal(|ui| {
            if images
                .show(ui, rt, user.avatar_url.as_deref(), 40.0)
                .clicked()
            {
                clicked = true;
            }
            ui.vertical(|ui| {
                ui.set_max_width(320.0);
                ui.add(
                    egui::Label::new(&user.username)
                        .truncate()
                        .wrap_mode(egui::TextWrapMode::Truncate),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Open").clicked() {
                    clicked = true;
                }
            });
        });
        clicked
    }

    /// 1履歴行。戻り値は再生クリックされたか (対応: `HistoryTab.tsx` 簡略)。
    fn history_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        entry: &HistoryEntry,
    ) -> widgets::RowHit {
        // `HistoryEntry` は `Track` ではないため行描画は手書きのまま。
        // 判定のみ共通化 (`Track` へ変換してタイトル一致を見る)。
        let probe = history_entry_to_track(entry);
        let is_current = widgets::is_currently_playing(player, &probe);
        let mut clicked = false;
        let mut art_secondary = false;
        let row = ui
            .horizontal(|ui| {
                let img = images.show(ui, rt, entry.artwork_url.as_deref(), 40.0);
                if img.clicked() {
                    clicked = true;
                }
                art_secondary = img.secondary_clicked();
                ui.vertical(|ui| {
                    ui.set_max_width(320.0);
                    let title = if is_current {
                        format!("▶ {}", entry.title)
                    } else {
                        entry.title.clone()
                    };
                    ui.add(
                        egui::Label::new(title)
                            .truncate()
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    ui.add(
                        egui::Label::new(&entry.artist_name)
                            .truncate()
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("▶").clicked() {
                        clicked = true;
                    }
                    if let Some(played) = entry.played_at.as_deref() {
                        let short: String = played.chars().take(16).collect();
                        ui.label(short);
                    }
                });
            })
            .response
            .interact(egui::Sense::click());
        let _ = &entry.id;
        if clicked {
            widgets::RowHit::Clicked
        } else if row.secondary_clicked() || art_secondary {
            widgets::RowHit::Menu
        } else {
            widgets::RowHit::None
        }
    }
}
