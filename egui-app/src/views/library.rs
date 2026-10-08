//! Phase 3: Library ハブ — タブ切替 + 各タブの一覧 (先頭ページのみ, limit 50)。
//! 対応: `desktop/src/pages/Library.tsx` (+ `LibraryCollection.tsx` の 4 タブ)。
//! SoundPrint / FreshDrops (`useFollowingDrops`) / ContinueRow の装飾は簡略表示
//! (いいね件数 + 上位ジャンル行) に留める。再生は単曲直結 (キューは Phase 3b)。
//! page>0 のページネーション・mutation (like/playlists 編集等) は見送り。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Paged, Playlist, ScUser, Track, tracks_from_value};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum LibraryAction {
    None,
    PlayTrack(Track),
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
    }
}

fn playlists_from_value(v: serde_json::Value) -> Vec<Playlist> {
    if let Ok(paged) = serde_json::from_value::<Paged<Playlist>>(v.clone()) {
        if !paged.collection.is_empty() {
            return paged.collection;
        }
    }
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|it| serde_json::from_value(it.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn users_from_value(v: serde_json::Value) -> Vec<ScUser> {
    if let Ok(paged) = serde_json::from_value::<Paged<ScUser>>(v.clone()) {
        if !paged.collection.is_empty() {
            return paged.collection;
        }
    }
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|it| serde_json::from_value(it.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
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
    likes: Query<Vec<Track>>,
    my_playlists: Query<Vec<Playlist>>,
    liked_playlists: Query<Vec<Playlist>>,
    followings: Query<Vec<ScUser>>,
    history: Query<Vec<HistoryEntry>>,
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
        if !self.likes.requested() {
            let api = api_owned.clone();
            self.likes.request(rt, async move {
                let v = api.get_json("/me/likes/tracks?limit=50&page=0").await?;
                Ok(tracks_from_value(&v))
            });
        }
        if !self.my_playlists.requested() {
            let api = api_owned.clone();
            self.my_playlists.request(rt, async move {
                let v = api.get_json("/me/playlists?limit=50&page=0").await?;
                Ok(playlists_from_value(v))
            });
        }
        if !self.liked_playlists.requested() {
            let api = api_owned.clone();
            self.liked_playlists.request(rt, async move {
                let v = api.get_json("/me/likes/playlists?limit=50&page=0").await?;
                Ok(playlists_from_value(v))
            });
        }
        if !self.followings.requested() {
            let api = api_owned.clone();
            self.followings.request(rt, async move {
                let v = api.get_json("/me/followings?limit=50&page=0").await?;
                Ok(users_from_value(v))
            });
        }
        if !self.history.requested() {
            let api = api_owned.clone();
            self.history.request(rt, async move {
                let v = api.get_json("/history?limit=50").await?;
                let page: HistoryPage =
                    serde_json::from_value(v).map_err(|e| e.to_string())?;
                Ok(page.collection)
            });
        }

        let mut changed = self.likes.poll();
        changed |= self.my_playlists.poll();
        changed |= self.liked_playlists.poll();
        changed |= self.followings.poll();
        changed |= self.history.poll();
        if changed
            || self.likes.loading
            || self.my_playlists.loading
            || self.liked_playlists.loading
            || self.followings.loading
            || self.history.loading
        {
            ui.ctx().request_repaint();
        }

        let mut action = LibraryAction::None;

        ui.heading("Library");
        if let Some(tracks) = self.likes.data.as_ref() {
            ui.label(soundprint_line(tracks));
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
                    let count = self.likes.data.as_ref().map(|t| t.len());
                    widgets::section_header(ui, "Liked Tracks", count);
                    if ui.small_button("See all").clicked() {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("likes".to_string()),
                        );
                    }
                });
                if self.likes.loading && self.likes.data.is_none() {
                    ui.label("Loading...");
                } else if let Some(err) = self.likes.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Likes unavailable: {err}"),
                    );
                } else if let Some(tracks) = self.likes.data.clone() {
                    let rows: Vec<Track> = tracks
                        .into_iter()
                        .filter(|t| {
                            needle.is_empty()
                                || t.title.to_lowercase().contains(&needle)
                                || t.artist_name().to_lowercase().contains(&needle)
                        })
                        .collect();
                    if rows.is_empty() {
                        ui.label("No liked tracks yet");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("library:likes")
                            .show(ui, |ui| {
                                for track in &rows {
                                    if Self::track_row(ui, rt, images, player, track, accent) {
                                        action = LibraryAction::PlayTrack(track.clone());
                                    }
                                }
                            });
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
                if (self.my_playlists.loading && self.my_playlists.data.is_none())
                    || (self.liked_playlists.loading && self.liked_playlists.data.is_none())
                {
                    ui.label("Loading...");
                } else {
                    if let Some(playlists) = self.my_playlists.data.clone() {
                        let rows: Vec<Playlist> = playlists
                            .into_iter()
                            .filter(|p| {
                                needle.is_empty() || p.title.to_lowercase().contains(&needle)
                            })
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
                                });
                        }
                    }
                    if let Some(playlists) = self.liked_playlists.data.clone() {
                        let rows: Vec<Playlist> = playlists
                            .into_iter()
                            .filter(|p| {
                                needle.is_empty() || p.title.to_lowercase().contains(&needle)
                            })
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
                                });
                        }
                    }
                }
            }
            LibraryTab::Following => {
                ui.horizontal(|ui| {
                    let count = self.followings.data.as_ref().map(|u| u.len());
                    widgets::section_header(ui, "Following", count);
                    if ui.small_button("See all").clicked() {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("following".to_string()),
                        );
                    }
                });
                if self.followings.loading && self.followings.data.is_none() {
                    ui.label("Loading...");
                } else if let Some(err) = self.followings.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Following unavailable: {err}"),
                    );
                } else if let Some(users) = self.followings.data.clone() {
                    let rows: Vec<ScUser> = users
                        .into_iter()
                        .filter(|u| {
                            needle.is_empty() || u.username.to_lowercase().contains(&needle)
                        })
                        .collect();
                    if rows.is_empty() {
                        ui.label("You are not following anyone");
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
                    ui.label("Loading...");
                } else if let Some(err) = self.history.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("History unavailable: {err}"),
                    );
                } else if let Some(entries) = self.history.data.clone() {
                    if entries.is_empty() {
                        ui.label("No listening history");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("library:history")
                            .show(ui, |ui| {
                                for entry in &entries {
                                    if Self::history_row(ui, rt, images, player, entry) {
                                        action = LibraryAction::PlayTrack(
                                            history_entry_to_track(entry),
                                        );
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
    ) -> bool {
        let playing = widgets::is_currently_playing(player, track);
        let dur = fmt_duration(track.duration);
        widgets::track_row(ui, rt, images, track, playing, accent, Some(&dur)).clicked()
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
            if images.show(ui, rt, user.avatar_url.as_deref(), 40.0).clicked() {
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
    ) -> bool {
        // `HistoryEntry` は `Track` ではないため行描画は手書きのまま。
        // 判定のみ共通化 (`Track` へ変換してタイトル一致を見る)。
        let probe = history_entry_to_track(entry);
        let is_current = widgets::is_currently_playing(player, &probe);
        let mut clicked = false;
        ui.horizontal(|ui| {
            if images
                .show(ui, rt, entry.artwork_url.as_deref(), 40.0)
                .clicked()
            {
                clicked = true;
            }
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
        });
        let _ = &entry.id;
        clicked
    }
}
