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

        ui.add_space(20.0);
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
                let gap = 10.0;
                let cols = widgets::grid_cols(ui.ctx(), 3, 4, 5, 6, 7);
                let card_w = widgets::grid_cell(avail, cols, gap);
                ui.add_space(12.0);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = gap;
                    for chunk in items.chunks(cols) {
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
                });
            }
        }

        ui.add_space(40.0);
        if self.discover.loading && self.discover.data.is_none() {
            widgets::loading(ui);
        } else if let Some(mixed) = self.discover.data.as_ref() {
            // 「See all / Show less」トグル (Tauri 版 DiscoverSections は
            // 取得済みアイテムのローカル展開のみ)。
            let mut toggled: Vec<String> = Vec::new();
            ui.scope(|ui| {
                // Tauri: flex-col gap-8 (セクション間 32px)。
                ui.spacing_mut().item_spacing.y = 32.0;
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
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
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
                        ui.add_space(12.0);
                        // グリッド (Tauri: grid-cols-3 sm:4 md:5 lg:6 xl:8、gap-2.5)。
                        let avail = ui.available_width();
                        let gap = 10.0;
                        let cols = widgets::grid_cols(ui.ctx(), 3, 4, 5, 6, 8);
                        let card_w = widgets::grid_cell(avail, cols, gap);
                        ui.scope(|ui| {
                            ui.spacing_mut().item_spacing.y = gap;
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
                                        let uid = item.urn.clone().unwrap_or_default();
                                        let (play_clicked, title_clicked) = ui
                                            .vertical(|ui| {
                                                ui.set_width(card_w);
                                                ui.spacing_mut().item_spacing.y = 0.0;
                                                let next = egui::Rect::from_min_size(
                                                    ui.next_widget_position(),
                                                    egui::Vec2::splat(card_w),
                                                );
                                                let hovered = ui.rect_contains_pointer(next);
                                                let hover_t = ui.ctx().animate_bool_with_time(
                                                    egui::Id::new(("discover-hover", &uid)),
                                                    hovered,
                                                    0.15,
                                                );
                                                let reveal = ui.ctx().animate_bool_with_time(
                                                    egui::Id::new(("discover-reveal", &uid)),
                                                    hovered,
                                                    0.2,
                                                );
                                                let art = item
                                                    .artwork_url
                                                    .as_deref()
                                                    .map(|u| u.replace("-large", "-t300x300"));
                                                let img = widgets::card_art(
                                                    ui,
                                                    rt,
                                                    images,
                                                    art.as_deref(),
                                                    card_w,
                                                    egui::CornerRadius::same(12),
                                                    1.0 + 0.04 * hover_t,
                                                );
                                                // ホバー: 暗転 + 白丸 + 再生グリフ。
                                                widgets::card_overlay(
                                                    ui.painter(),
                                                    img.rect,
                                                    12.0,
                                                    hover_t,
                                                    widgets::TransportIcon::Play,
                                                );
                                                // コピーリンク (右上、hover で表示)。
                                                if let Some(url) = item.permalink_url.clone() {
                                                    let center = egui::pos2(
                                                        img.rect.right() - 20.0,
                                                        img.rect.top() + 20.0,
                                                    );
                                                    let rect = egui::Rect::from_center_size(
                                                        center,
                                                        egui::Vec2::splat(24.0),
                                                    );
                                                    let resp = ui.interact(
                                                        rect,
                                                        egui::Id::new((
                                                            "discover-copy",
                                                            &uid,
                                                        )),
                                                        egui::Sense::click(),
                                                    );
                                                    let a = if resp.hovered() { 179 } else { 128 };
                                                    ui.painter().circle_filled(
                                                        center,
                                                        12.0,
                                                        egui::Color32::from_black_alpha(
                                                            (a as f32 * reveal) as u8,
                                                        ),
                                                    );
                                                    let color = if resp.hovered() {
                                                        egui::Color32::WHITE
                                                    } else {
                                                        egui::Color32::from_white_alpha(204)
                                                    };
                                                    let color = egui::Color32::from_rgba_unmultiplied(
                                                        color.r(),
                                                        color.g(),
                                                        color.b(),
                                                        (color.a() as f32 * reveal) as u8,
                                                    );
                                                    widgets::paint_ui_icon(
                                                        ui.painter(),
                                                        egui::Rect::from_center_size(
                                                            center,
                                                            egui::Vec2::splat(12.0),
                                                        ),
                                                        widgets::UiIcon::Link,
                                                        color,
                                                    );
                                                    if resp
                                                        .on_hover_text("Copy link")
                                                        .clicked()
                                                    {
                                                        ui.ctx().copy_text(url);
                                                    }
                                                }
                                                // タイトル (Tauri: mt-2 / 13 medium
                                                // white/85 hover:white) + 説明 11 white/40。
                                                ui.add_space(8.0);
                                                let lbl = widgets::card_text(
                                                    ui,
                                                    &item.title,
                                                    crate::theme::medium(13.0),
                                                    egui::Color32::from_white_alpha(217),
                                                    egui::Color32::WHITE,
                                                    17.0,
                                                );
                                                let desc = item
                                                    .short_description
                                                    .as_deref()
                                                    .or(item.description.as_deref())
                                                    .unwrap_or("");
                                                let _ = widgets::card_text(
                                                    ui,
                                                    desc,
                                                    egui::FontId::proportional(11.0),
                                                    egui::Color32::from_white_alpha(102),
                                                    egui::Color32::from_white_alpha(102),
                                                    15.0,
                                                );
                                                (img.clicked(), lbl.clicked())
                                            })
                                            .inner;
                                        if play_clicked {
                                            action = HomeAction::StartDiscover(item.clone());
                                        } else if title_clicked {
                                            if let Some((route, param)) = title_nav {
                                                action =
                                                    HomeAction::Navigate(route, Some(param));
                                            } else {
                                                action =
                                                    HomeAction::StartDiscover(item.clone());
                                            }
                                        }
                                    }
                                });
                            }
                        });
                    });
                }
            });
            for urn in toggled {
                if !self.expanded.remove(&urn) {
                    self.expanded.insert(urn);
                }
            }
        }

        action
    }
}
