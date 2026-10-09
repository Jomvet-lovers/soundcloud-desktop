//! Phase 3: Home — 挨拶 + いいね棚 + Discover 行。
//! 対応: `desktop/src/pages/Home.tsx` (+ `Search.tsx` の `DiscoverSections`)。
//! カード装飾・D&D・キューは簡略化。再生は単曲直結 (キューは Phase 3b)。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{DiscoverItem, DiscoverMixed, ScUser, Track};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route, SettingsState};
use crate::widgets::{self, is_currently_playing};

pub enum HomeAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (リスト全体がキューになる)。
    PlayList(Vec<Track>, usize),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    /// Discover 棚のアイテム (ユーザー/ステーション/システムミックス等)。
    StartDiscover(DiscoverItem),
    /// 他ページへの遷移 (Liked Tracks の See all 等)。
    Navigate(Route, Option<String>),
}

#[derive(Default)]
pub struct HomeView {
    me: Query<ScUser>,
    likes: Query<Vec<Track>>,
    discover: Query<DiscoverMixed>,
    /// 「See all」で展開中の Discover 棚 (urn)。
    expanded: std::collections::HashSet<String>,
}

fn greeting(name: Option<&str>) -> String {
    let hour = chrono::Local::now()
        .format("%H")
        .to_string()
        .parse::<u32>()
        .unwrap_or(12);
    let Some(name) = name else {
        return "Home".to_string();
    };
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

impl HomeView {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        api: Option<&ApiClient>,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &mut PlayerState,
        audio: Option<&Arc<AudioState>>,
        settings: &SettingsState,
        ui: &mut egui::Ui,
    ) -> HomeAction {
        let Some(api) = api else {
            ui.label("backend not running");
            return HomeAction::None;
        };
        let api_owned = api.clone();
        if !self.me.requested() {
            let api = api_owned.clone();
            self.me.request(rt, async move {
                api.get_json("/me/cold")
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
        if !self.likes.requested() {
            let api = api_owned.clone();
            self.likes.request(rt, async move {
                let v = api.get_json("/me/likes/tracks?limit=60&page=0").await?;
                let tracks: Vec<Track> = serde_json::from_value(
                    v.get("collection")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .map_err(|e| e.to_string())?;
                Ok(tracks)
            });
        }
        if !self.discover.requested() {
            let api = api_owned.clone();
            self.discover.request(rt, async move {
                api.get_json("/discover/mixed")
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }

        let mut changed = self.me.poll();
        changed |= self.likes.poll();
        changed |= self.discover.poll();
        if changed || self.me.loading || self.likes.loading || self.discover.loading {
            ui.ctx().request_repaint();
        }

        let mut action = HomeAction::None;

        let user_name = self
            .me
            .data
            .as_ref()
            .map(|u| u.username.clone())
            .filter(|n| !n.is_empty());
        ui.add(egui::Label::new(
            egui::RichText::new(greeting(user_name.as_deref()))
                .font(crate::theme::semibold(24.0)),
        ));

        ui.add_space(14.0);
        let accent = widgets::accent_color(settings);
        ui.horizontal(|ui| {
            widgets::section_title(ui, "Liked Tracks");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::see_all(ui) {
                    action = HomeAction::Navigate(
                        Route::LibraryCollection,
                        Some("likes".to_string()),
                    );
                }
            });
        });

        if self.likes.loading && self.likes.data.is_none() {
            widgets::loading(ui);
        } else if let Some(err) = self.likes.error.as_ref() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Likes unavailable: {err}"),
            );
        } else if let Some(tracks) = self.likes.data.as_ref() {
            if tracks.is_empty() {
                widgets::empty_note(ui, "No liked tracks yet");
            } else {
                let _ = audio;
                // グリッド (Tauri 版 Home.tsx: grid-cols-3..7)。egui の
                // horizontal_wrapped が折り返さないため行チャンクで並べる。
                let items: Vec<(usize, &Track)> =
                    tracks.iter().take(60).enumerate().collect();
                let avail = ui.available_width();
                let gap = 12.0;
                let cols = widgets::grid_cols(avail, 3, 4, 5, 6, 7);
                let card_w =
                    ((avail - gap * (cols.saturating_sub(1)) as f32) / cols as f32).max(80.0);
                let per_row = cols;
                for chunk in items.chunks(per_row) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for &(i, track) in chunk {
                            let playing = is_currently_playing(player, track);
                            let hit = widgets::track_card(
                                ui, rt, images, track, card_w, playing, accent,
                            );
                            if hit.play_clicked() {
                                action = HomeAction::PlayList(tracks.clone(), i);
                            } else if hit.title_clicked() {
                                action = HomeAction::Navigate(
                                    Route::Track,
                                    Some(track.urn.clone()),
                                );
                            } else if hit.artist_clicked() {
                                if let Some(u) = track.user.as_ref() {
                                    action = HomeAction::Navigate(
                                        Route::User,
                                        Some(u.urn.clone()),
                                    );
                                }
                            } else if hit.menu_clicked() {
                                action = HomeAction::OpenMenu(track.clone());
                            }
                        }
                    });
                }
            }
        }

        ui.add_space(24.0);
        if self.discover.loading && self.discover.data.is_none() {
            widgets::loading(ui);
        } else if let Some(mixed) = self.discover.data.as_ref() {
            // 「See all / Show less」トグル (Tauri 版 DiscoverSections は
            // 取得済みアイテムのローカル展開のみ)。
            let mut toggled: Vec<String> = Vec::new();
            for sel in &mixed.collection {
                let all: Vec<DiscoverItem> = sel
                    .items
                    .as_ref()
                    .map(|p| p.collection.clone())
                    .unwrap_or_default();
                if all.is_empty() {
                    continue;
                }
                let is_open = self.expanded.contains(&sel.urn);
                let shown: Vec<DiscoverItem> = if is_open {
                    all.clone()
                } else {
                    all.iter().take(10).cloned().collect()
                };
                ui.horizontal(|ui| {
                    widgets::section_title(ui, &sel.title);
                    if all.len() > 10 {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if widgets::link_button(
                                    ui,
                                    if is_open { "Show less" } else { "See all" },
                                ) {
                                    toggled.push(sel.urn.clone());
                                }
                            },
                        );
                    }
                });
                // グリッド (Tauri: grid-cols-3 sm:4 md:5 lg:6 xl:8、gap-2.5)。
                let avail = ui.available_width();
                let gap = 10.0;
                let cols = widgets::grid_cols(avail, 3, 4, 5, 6, 8);
                let card_w =
                    ((avail - gap * (cols.saturating_sub(1)) as f32) / cols as f32).max(80.0);
                for chunk in shown.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for item in chunk {
                            // タイトルはページがあれば遷移 (Tauri: DiscoverCard)。
                            let title_nav: Option<(Route, String)> =
                                item.urn.as_ref().and_then(|urn| {
                                    if urn.starts_with("soundcloud:playlists:") {
                                        Some((Route::Playlist, urn.clone()))
                                    } else if urn.starts_with("soundcloud:users:") {
                                        Some((Route::User, urn.clone()))
                                    } else {
                                        None
                                    }
                                });
                            let (play_clicked, title_clicked) = ui
                                .vertical(|ui| {
                                    ui.set_max_width(card_w);
                                    let art = item
                                        .artwork_url
                                        .as_deref()
                                        .map(|u| u.replace("-large", "-t300x300"));
                                    let img = images.show_rounded(
                                        ui,
                                        rt,
                                        art.as_deref(),
                                        card_w,
                                        egui::CornerRadius::same(12),
                                    );
                                    // ホバー: 暗転 + 再生グリフ (フェード)。
                                    let hover_t = ui.ctx().animate_bool_with_time(
                                        egui::Id::new((
                                            "discover-hover",
                                            item.urn.clone().unwrap_or_default(),
                                        )),
                                        img.hovered(),
                                        0.15,
                                    );
                                    if hover_t > 0.001 {
                                        ui.painter().rect_filled(
                                            img.rect,
                                            4.0,
                                            egui::Color32::from_black_alpha(
                                                (90.0 * hover_t) as u8,
                                            ),
                                        );
                                        widgets::paint_play_glyph_alpha(
                                            ui.painter(),
                                            img.rect,
                                            false,
                                            hover_t,
                                        );
                                    }
                                    let lbl = ui.add(
                                        egui::Label::new(&item.title)
                                            .truncate()
                                            .wrap_mode(egui::TextWrapMode::Truncate)
                                            .sense(egui::Sense::click()),
                                    );
                                    let desc = item
                                        .short_description
                                        .as_deref()
                                        .or(item.description.as_deref())
                                        .unwrap_or("");
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(desc).small().weak(),
                                        )
                                        .truncate()
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                    );
                                    (img.clicked(), lbl.clicked())
                                })
                                .inner;
                            if play_clicked {
                                action = HomeAction::StartDiscover(item.clone());
                            } else if title_clicked {
                                if let Some((route, param)) = title_nav {
                                    action = HomeAction::Navigate(route, Some(param));
                                } else {
                                    action = HomeAction::StartDiscover(item.clone());
                                }
                            }
                        }
                    });
                }
            }
            for urn in toggled {
                if !self.expanded.remove(&urn) {
                    self.expanded.insert(urn);
                }
            }
        }

        action
    }
}
