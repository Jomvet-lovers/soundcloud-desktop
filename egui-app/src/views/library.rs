//! Phase 3: Library ハブ — SoundPrint マストヘッド + Fresh drops + コレクションのレール。
//! 対応: `desktop/src/pages/Library.tsx`。各セクションの全件ページは
//! LibraryCollection (collection.rs) が担う。

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

fn fmt_duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// 対応: `SoundPrintMasthead.tsx` の greeting。
fn greeting(name: &str) -> String {
    let hour = chrono::Local::now()
        .format("%H")
        .to_string()
        .parse::<u32>()
        .unwrap_or(12);
    if hour < 5 {
        format!("Late night, {name}")
    } else if hour < 12 {
        format!("Good morning, {name}")
    } else if hour < 18 {
        format!("Good afternoon, {name}")
    } else {
        format!("Good evening, {name}")
    }
}

/// 上位ジャンル (share 付き)。対応: `topGenres`。
fn top_genres(tracks: &[Track], n: usize) -> Vec<(String, f32, egui::Color32)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut with_genre = 0usize;
    for t in tracks {
        if let Some(g) = t.genre.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
            *counts.entry(g.to_string()).or_insert(0) += 1;
            with_genre += 1;
        }
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(n);
    v.into_iter()
        .map(|(g, c)| {
            let share = if with_genre > 0 {
                c as f32 / with_genre as f32
            } else {
                0.0
            };
            let color = widgets::genre_color(&g);
            (g, share, color)
        })
        .collect()
}

/// ジャンルフィルタ (Soundprint バーで選択中のみ適用)。
fn genre_match(filter: &Option<String>, genre: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(f) => genre.map(|g| g.trim() == f.as_str()).unwrap_or(false),
    }
}

#[derive(Default)]
pub struct LibraryView {
    likes: Pager<Track>,
    my_playlists: Pager<Playlist>,
    liked_playlists: Pager<Playlist>,
    followings: Pager<ScUser>,
    history: Query<Vec<HistoryEntry>>,
    /// Fresh drops (フォロー中ユーザの新着を合成)。
    fresh: Query<Vec<Track>>,
    /// マストヘッドのアバター用 (自分のプロフィール)。
    me: Query<ScUser>,
    /// Soundprint バーで選んだジャンル (レールのフィルタ)。
    genre: Option<String>,
}

impl LibraryView {
    /// プレイリスト追加後にレールを取り直す (Tauri: invalidateQueries 相当)。
    pub fn invalidate_playlists(&mut self) {
        self.my_playlists = Pager::default();
        self.liked_playlists = Pager::default();
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
        if !self.me.requested() {
            let api = api_owned.clone();
            self.me.request(rt, async move {
                api.get_json("/me/cold")
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
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
                    let path = format!(
                        "/users/{}/tracks?limit=6&page=0",
                        urlencoding::encode(&urn)
                    );
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
        changed |= self.me.poll();
        if changed {
            ui.ctx().request_repaint();
        }

        let mut action = LibraryAction::None;

        // ── SoundPrint マストヘッド (アバター + 挨拶 + 円形シャッフル) ──
        ui.horizontal(|ui| {
            let avatar = self.me.data.as_ref().and_then(|u| u.avatar_url.clone());
            images.show(ui, rt, avatar.as_deref(), 100.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Library").size(11.0).weak());
                let name = self
                    .me
                    .data
                    .as_ref()
                    .map(|u| u.username.clone())
                    .unwrap_or_default();
                let title = if name.is_empty() {
                    "Library".to_string()
                } else {
                    greeting(&name)
                };
                ui.add(egui::Label::new(
                    egui::RichText::new(title).font(crate::theme::semibold(34.0)),
                ));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::hero_play_button(
                    ui,
                    false,
                    !self.likes.items.is_empty(),
                    56.0,
                )
                .clicked()
                {
                    action = LibraryAction::ShuffleLikes(self.likes.items.clone());
                }
            });
        });

        // ── Soundprint バー (上位ジャンル。クリックでレールを絞り込み) ──
        let spectrum = top_genres(&self.likes.items, 7);
        if !spectrum.is_empty() {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                widgets::ui_icon(ui, widgets::UiIcon::Library, 13.0, spectrum[0].2);
                ui.label(egui::RichText::new("Your soundprint").size(10.0).weak());
            });
            ui.add_space(4.0);
            let mut clicked: Option<String> = None;
            let total_w = ui.available_width();
            let n = spectrum.len();
            let gap = 8.0;
            let bar_w = ((total_w - gap * (n.saturating_sub(1)) as f32) / n as f32).max(24.0);
            let h_total = 88.0;
            let max_share = spectrum[0].1.max(1e-6);
            let selected_any = self.genre.is_some();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (g, share, color) in &spectrum {
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(bar_w, h_total + 34.0),
                        egui::Sense::click(),
                    );
                    let selected = self.genre.as_deref() == Some(g.as_str());
                    let color = if selected_any && !selected {
                        color.gamma_multiply(0.45)
                    } else {
                        *color
                    };
                    let ratio = share / max_share;
                    // 登場アニメーション (Tauri: sp-rise)。
                    let rise = ui.ctx().animate_bool_with_time(
                        egui::Id::new(("sp-rise", g.as_str())),
                        true,
                        0.6,
                    );
                    let bh = h_total * (32.0 + ratio * 68.0) / 100.0 * rise;
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.top() + h_total - bh),
                        egui::pos2(rect.right(), rect.top() + h_total),
                    );
                    ui.painter().rect_filled(
                        bar,
                        egui::CornerRadius {
                            nw: 7,
                            ne: 7,
                            sw: 0,
                            se: 0,
                        },
                        color,
                    );
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.top() + h_total + 6.0),
                        egui::Align2::CENTER_TOP,
                        g,
                        egui::FontId::proportional(10.5),
                        if selected {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_white_alpha(150)
                        },
                    );
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.top() + h_total + 20.0),
                        egui::Align2::CENTER_TOP,
                        format!("{}%", (share * 100.0).round() as i64),
                        egui::FontId::proportional(9.0),
                        egui::Color32::from_white_alpha(80),
                    );
                    if resp.clicked() {
                        clicked = Some(g.clone());
                    }
                }
            });
            if let Some(g) = clicked {
                self.genre = if self.genre.as_deref() == Some(g.as_str()) {
                    None
                } else {
                    Some(g)
                };
            }
        }

        // ── Fresh drops (大きめの行 + New バッジ + 経過) ──
        if let Some(fresh) = self.fresh.data.clone() {
            if !fresh.is_empty() {
                ui.add_space(16.0);
                let mut refresh = false;
                ui.horizontal(|ui| {
                    widgets::section_title(ui, "Fresh from who you follow");
                    ui.label(
                        egui::RichText::new(fresh.len().to_string())
                            .size(11.0)
                            .weak(),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.small_button("Refresh").clicked() {
                                refresh = true;
                            }
                        },
                    );
                });
                if refresh {
                    self.fresh = Query::default();
                }
                ui.add_space(4.0);
                let list: Vec<Track> = fresh.iter().take(6).cloned().collect();
                for (i, t) in list.iter().enumerate() {
                    let bg_idx = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        let art = t.artwork("t300x300");
                        images.show(ui, rt, art.as_deref(), 88.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let (badge, _) = ui.allocate_exact_size(
                                    egui::vec2(38.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    badge,
                                    9.0,
                                    egui::Color32::from_rgb(40, 40, 46),
                                );
                                ui.painter().text(
                                    badge.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "New",
                                    egui::FontId::proportional(10.0),
                                    egui::Color32::from_white_alpha(220),
                                );
                                let age = widgets::age_text(t.created_at.as_deref());
                                if !age.is_empty() {
                                    ui.label(egui::RichText::new(age).size(11.0).weak());
                                }
                            });
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(t.display_title())
                                        .font(crate::theme::semibold(15.0)),
                                )
                                .truncate()
                                .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                            ui.label(egui::RichText::new(t.artist_name()).size(12.0).weak());
                        });
                    });
                    let resp = ui.interact(
                        row.response.rect,
                        egui::Id::new(("lib-fresh-row", i)),
                        egui::Sense::click(),
                    );
                    let hover_t = ui.ctx().animate_bool_with_time(
                        egui::Id::new(("lib-fresh-hover", i)),
                        resp.hovered(),
                        0.12,
                    );
                    if hover_t > 0.001 {
                        let rect = row.response.rect.expand2(egui::vec2(6.0, 4.0));
                        ui.painter().set(
                            bg_idx,
                            egui::Shape::rect_filled(
                                rect,
                                8.0,
                                egui::Color32::from_white_alpha((14.0 * hover_t) as u8),
                            ),
                        );
                    }
                    if resp.clicked() {
                        action = LibraryAction::PlayList(list.clone(), i);
                    } else if resp.secondary_clicked() {
                        action = LibraryAction::OpenMenu(t.clone());
                    }
                    ui.add_space(6.0);
                }
            }
        }

        // ── Continue (履歴のレール) ──
        let continue_tracks: Vec<Track> = self
            .history
            .data
            .as_ref()
            .map(|entries| entries.iter().map(history_entry_to_track).collect())
            .unwrap_or_default();
        let continue_preview: Vec<Track> = continue_tracks.iter().take(10).cloned().collect();
        if !continue_preview.is_empty() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                widgets::ui_icon(
                    ui,
                    widgets::UiIcon::History,
                    16.0,
                    egui::Color32::from_white_alpha(140),
                );
                ui.add(egui::Label::new(
                    egui::RichText::new("Jump back in")
                        .font(crate::theme::bold(16.0))
                        .color(egui::Color32::from_white_alpha(230)),
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::see_all(ui) {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("history".to_string()),
                        );
                    }
                });
            });
            ui.add_space(12.0);
            egui::ScrollArea::horizontal()
                .id_salt("lib:continue")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for (i, t) in continue_preview.iter().enumerate() {
                            let playing = widgets::is_currently_playing(player, t);
                            let hit =
                                widgets::track_card(ui, rt, images, t, 112.0, playing, accent);
                            if hit.play_clicked() {
                                action =
                                    LibraryAction::PlayList(continue_preview.clone(), i);
                            } else if hit.title_clicked() {
                                action = LibraryAction::Navigate(
                                    Route::Track,
                                    Some(t.urn.clone()),
                                );
                            } else if hit.artist_clicked() {
                                if let Some(u) = t.user.as_ref() {
                                    action = LibraryAction::Navigate(
                                        Route::User,
                                        Some(u.urn.clone()),
                                    );
                                }
                            } else if hit.menu_clicked() {
                                action = LibraryAction::OpenMenu(t.clone());
                            }
                        }
                    });
                });
        }

        // ── Your Playlists ──
        let pl_preview: Vec<Playlist> =
            self.my_playlists.items.iter().take(12).cloned().collect();
        if !pl_preview.is_empty() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                widgets::ui_icon(
                    ui,
                    widgets::UiIcon::Library,
                    15.0,
                    egui::Color32::from_white_alpha(160),
                );
                widgets::section_title(ui, "Your Playlists");
                ui.label(
                    egui::RichText::new(self.my_playlists.items.len().to_string())
                        .size(11.0)
                        .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::link_button(ui, "See all") {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("playlists".to_string()),
                        );
                    }
                });
            });
            egui::ScrollArea::horizontal()
                .id_salt("lib:my-playlists")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for p in &pl_preview {
                            if playlist_card(ui, rt, images, p, 120.0).clicked() {
                                action = LibraryAction::Navigate(
                                    Route::Playlist,
                                    Some(p.urn.clone()),
                                );
                            }
                        }
                    });
                });
        }

        // ── Liked Playlists ──
        let liked_pl_preview: Vec<Playlist> =
            self.liked_playlists.items.iter().take(12).cloned().collect();
        if !liked_pl_preview.is_empty() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                widgets::ui_icon(
                    ui,
                    widgets::UiIcon::Bookmark,
                    15.0,
                    egui::Color32::from_white_alpha(160),
                );
                widgets::section_title(ui, "Liked Playlists");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::link_button(ui, "See all") {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("playlists".to_string()),
                        );
                    }
                });
            });
            egui::ScrollArea::horizontal()
                .id_salt("lib:liked-playlists")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for p in &liked_pl_preview {
                            if playlist_card(ui, rt, images, p, 120.0).clicked() {
                                action = LibraryAction::Navigate(
                                    Route::Playlist,
                                    Some(p.urn.clone()),
                                );
                            }
                        }
                    });
                });
        }

        // ── Artists (フォロー中) ──
        let artist_preview: Vec<ScUser> =
            self.followings.items.iter().take(14).cloned().collect();
        if !artist_preview.is_empty() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                widgets::ui_icon(
                    ui,
                    widgets::UiIcon::Users,
                    15.0,
                    egui::Color32::from_white_alpha(160),
                );
                widgets::section_title(ui, "Artists");
                ui.label(
                    egui::RichText::new(self.followings.items.len().to_string())
                        .size(11.0)
                        .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::link_button(ui, "See all") {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("following".to_string()),
                        );
                    }
                });
            });
            egui::ScrollArea::horizontal()
                .id_salt("lib:artists")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for u in &artist_preview {
                            if artist_card(ui, rt, images, u).clicked() {
                                action =
                                    LibraryAction::Navigate(Route::User, Some(u.urn.clone()));
                            }
                        }
                    });
                });
        }

        // ── Liked Tracks (ジャンルフィルタ適用) ──
        let likes_preview: Vec<Track> = self
            .likes
            .items
            .iter()
            .filter(|t| genre_match(&self.genre, t.genre.as_deref()))
            .take(12)
            .cloned()
            .collect();
        if !likes_preview.is_empty() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                widgets::ui_icon(
                    ui,
                    widgets::UiIcon::Heart,
                    15.0,
                    egui::Color32::from_white_alpha(160),
                );
                widgets::section_title(ui, "Liked Tracks");
                ui.label(
                    egui::RichText::new(self.likes.items.len().to_string())
                        .size(11.0)
                        .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::link_button(ui, "See all") {
                        action = LibraryAction::Navigate(
                            Route::LibraryCollection,
                            Some("likes".to_string()),
                        );
                    }
                });
            });
            egui::ScrollArea::horizontal()
                .id_salt("lib:likes")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, t) in likes_preview.iter().enumerate() {
                            let playing = widgets::is_currently_playing(player, t);
                            let hit =
                                widgets::track_card(ui, rt, images, t, 112.0, playing, accent);
                            if hit.play_clicked() {
                                action = LibraryAction::PlayLikes(likes_preview.clone(), i);
                            } else if hit.title_clicked() {
                                action = LibraryAction::Navigate(
                                    Route::Track,
                                    Some(t.urn.clone()),
                                );
                            } else if hit.artist_clicked() {
                                if let Some(u) = t.user.as_ref() {
                                    action = LibraryAction::Navigate(
                                        Route::User,
                                        Some(u.urn.clone()),
                                    );
                                }
                            } else if hit.menu_clicked() {
                                action = LibraryAction::OpenMenu(t.clone());
                            }
                        }
                    });
                });
        }

        // フォロワー数などの簡易フッタ (サウンドプリントの補足)。
        if self.likes.items.is_empty() && !self.likes.q.loading {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} liked tracks — {}",
                    self.likes.items.len(),
                    fmt_duration(
                        self.likes.items.iter().map(|t| t.duration).sum::<i64>()
                    )
                ))
                .size(11.0)
                .weak(),
            );
        }

        action
    }
}

/// プレイリストカード (レール用)。戻り値はクリック応答。
fn playlist_card(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    p: &Playlist,
    size: f32,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.set_max_width(size + 8.0);
        let art = p.artwork("t300x300");
        let resp = images.show(ui, rt, art.as_deref(), size);
        let title = ui.add(
            egui::Label::new(
                egui::RichText::new(&p.title)
                    .font(crate::theme::medium(12.5))
                    .color(egui::Color32::from_white_alpha(217)),
            )
            .truncate()
            .wrap_mode(egui::TextWrapMode::Truncate)
            .sense(egui::Sense::click()),
        );
        let mut merged = resp.union(title);
        if let Some(u) = p.user.as_ref() {
            let user = ui.add(
                egui::Label::new(
                    egui::RichText::new(&u.username)
                        .size(11.0)
                        .color(egui::Color32::from_white_alpha(102)),
                )
                .truncate()
                .wrap_mode(egui::TextWrapMode::Truncate)
                .sense(egui::Sense::click()),
            );
            merged = merged.union(user);
        }
        merged
    })
    .inner
}

/// アーティストのミニカード (円形アバター + 名前)。
fn artist_card(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    u: &ScUser,
) -> egui::Response {
    ui.vertical_centered(|ui| {
        ui.set_max_width(88.0);
        let resp = images.show_rounded(
            ui,
            rt,
            u.avatar_url.as_deref(),
            64.0,
            egui::CornerRadius::same(32),
        );
        let label = ui.add(
            egui::Label::new(&u.username)
                .truncate()
                .wrap_mode(egui::TextWrapMode::Truncate)
                .sense(egui::Sense::click()),
        );
        resp.union(label)
    })
    .inner
}
