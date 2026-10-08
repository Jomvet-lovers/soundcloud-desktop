//! Phase 3: Collection 深掘りページ — `param` のセクション名 1 件を全件表示。
//! 対応: `desktop/src/pages/LibraryCollection.tsx`
//! (`/library/:section`, section = likes | playlists | following | history)。
//! フィルタはクライアント側の部分一致 (React の `deferredFilter` 相当、履歴除く)。
//! 一覧は無限スクロール (`pager::auto_load`)、履歴のみ先頭 50 件。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Playlist, ScUser, Track};
use crate::images::Images;
use crate::pager::Pager;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum CollectionAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (リスト全体がキューになる)。
    PlayList(Vec<Track>, usize),
    /// いいね一覧の再生 (最後まで継続ソース付き)。
    PlayLikes(Vec<Track>, usize),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    Navigate(Route, Option<String>),
}

/// `desktop/src/lib/hooks.ts` の `HistoryEntry` に対応 (library.rs と同型)。
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

fn history_track_urn(sc_track_id: &str) -> String {
    if sc_track_id.starts_with("soundcloud:tracks:") {
        sc_track_id.to_string()
    } else {
        format!("soundcloud:tracks:{sc_track_id}")
    }
}

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

/// `LibraryCollection.tsx` の `SECTIONS` / `TITLE_KEY` に対応。不明値は likes。
fn normalize_section(param: Option<&str>) -> &'static str {
    match param {
        Some("playlists") => "playlists",
        Some("following") => "following",
        Some("history") => "history",
        _ => "likes",
    }
}

fn section_title(section: &str) -> &'static str {
    match section {
        "playlists" => "Playlists",
        "following" => "Following",
        "history" => "History",
        _ => "Liked Tracks",
    }
}

fn fmt_duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

#[derive(Default)]
pub struct CollectionView {
    loaded_for: String,
    filter: String,
    likes: Pager<Track>,
    my_playlists: Pager<Playlist>,
    liked_playlists: Pager<Playlist>,
    followings: Pager<ScUser>,
    history: Query<Vec<HistoryEntry>>,
}

impl CollectionView {
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
    ) -> CollectionAction {
        let _ = audio;
        let _ = cache;
        let Some(api) = api else {
            ui.label("backend not running");
            return CollectionAction::None;
        };
        let section = normalize_section(param);

        // セクション切替で取得状態を捨てる。
        if self.loaded_for != section {
            self.loaded_for = section.to_string();
            self.likes.reset();
            self.my_playlists.reset();
            self.liked_playlists.reset();
            self.followings.reset();
            self.history = Query::default();
        }

        let api_owned = api.clone();
        match section {
            "playlists" => {
                self.my_playlists.ensure_page(rt, api, "/me/playlists", 50);
                self.liked_playlists
                    .ensure_page(rt, api, "/me/likes/playlists", 50);
            }
            "following" => {
                self.followings.ensure_page(rt, api, "/me/followings", 50);
            }
            "history" => {
                if !self.history.requested() {
                    let api = api_owned.clone();
                    self.history.request(rt, async move {
                        let v = api.get_json("/history?limit=50").await?;
                        let page: HistoryPage =
                            serde_json::from_value(v).map_err(|e| e.to_string())?;
                        Ok(page.collection)
                    });
                }
            }
            _ => {
                self.likes.ensure_page(rt, api, "/me/likes/tracks", 50);
            }
        }

        let mut changed = self.likes.poll();
        changed |= self.my_playlists.poll();
        changed |= self.liked_playlists.poll();
        changed |= self.followings.poll();
        changed |= self.history.poll();
        if changed {
            ui.ctx().request_repaint();
        }

        let mut action = CollectionAction::None;

        if ui.small_button("← Library").clicked() {
            action = CollectionAction::Navigate(Route::Library, None);
        }
        // React 版 (`LibrarySubHeader` + user count 由来) と同様、件数を添える。
        match section {
            "likes" => {
                widgets::section_header(ui, section_title(section), Some(self.likes.items.len()));
            }
            "following" => {
                widgets::section_header(
                    ui,
                    section_title(section),
                    Some(self.followings.items.len()),
                );
            }
            _ => {
                widgets::section_header(ui, section_title(section), None);
            }
        }
        if section != "history" {
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.text_edit_singleline(&mut self.filter);
            });
        }
        ui.separator();

        let needle = self.filter.trim().to_lowercase();

        match section {
            "playlists" => {
                if (self.my_playlists.q.loading && self.my_playlists.items.is_empty())
                    || (self.liked_playlists.q.loading && self.liked_playlists.items.is_empty())
                {
                    widgets::loading(ui);
                } else {
                    let mut empty = true;
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
                            empty = false;
                            widgets::section_header(ui, "Your Playlists", Some(rows.len()));
                            egui::ScrollArea::vertical()
                                .id_salt("collection:my-playlists")
                                .max_height(300.0)
                                .show(ui, |ui| {
                                    for p in &rows {
                                        if Self::playlist_row(ui, rt, images, p) {
                                            action = CollectionAction::Navigate(
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
                            empty = false;
                            widgets::section_header(ui, "Liked Playlists", Some(rows.len()));
                            egui::ScrollArea::vertical()
                                .id_salt("collection:liked-playlists")
                                .max_height(300.0)
                                .show(ui, |ui| {
                                    for p in &rows {
                                        if Self::playlist_row(ui, rt, images, p) {
                                            action = CollectionAction::Navigate(
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
                    if empty {
                        widgets::empty_note(ui, "No playlists found");
                    }
                }
            }
            "following" => {
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
                            .id_salt("collection:following")
                            .show(ui, |ui| {
                                for u in &rows {
                                    if Self::user_row(ui, rt, images, u) {
                                        action = CollectionAction::Navigate(
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
            "history" => {
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
                            .id_salt("collection:history")
                            .show(ui, |ui| {
                                let tracks: Vec<Track> =
                                    entries.iter().map(history_entry_to_track).collect();
                                for (i, entry) in entries.iter().enumerate() {
                                    match Self::history_row(ui, rt, images, player, entry) {
                                        widgets::RowHit::Clicked => {
                                            action = CollectionAction::PlayList(tracks.clone(), i);
                                        }
                                        widgets::RowHit::Menu => {
                                            action = CollectionAction::OpenMenu(
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
            _ => {
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
                            .id_salt("collection:likes")
                            .show(ui, |ui| {
                                for (i, track) in rows.iter().enumerate() {
                                    match Self::track_row(ui, rt, images, player, track, accent) {
                                        widgets::RowHit::Clicked => {
                                            // フィルタ無しのみ「いいね最後まで」継続。
                                            action = if needle.is_empty() {
                                                CollectionAction::PlayLikes(rows.clone(), i)
                                            } else {
                                                CollectionAction::PlayList(rows.clone(), i)
                                            };
                                        }
                                        widgets::RowHit::Menu => {
                                            action = CollectionAction::OpenMenu(track.clone());
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
                        // フィルタ中は残りページを自動取得 (Tauri: LikesTab)。
                        if !needle.is_empty() && self.likes.has_more && !self.likes.q.loading {
                            self.likes.fetch_page(rt, api, "/me/likes/tracks", 50);
                        }
                    }
                }
            }
        }

        action
    }

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
