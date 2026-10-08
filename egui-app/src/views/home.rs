//! Phase 3: Home — 挨拶 + いいね棚 + Discover 行。
//! 対応: `desktop/src/pages/Home.tsx` (+ `Search.tsx` の `DiscoverSections`)。
//! カード装飾・D&D・キューは簡略化。再生は単曲直結 (キューは Phase 3b)。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{DiscoverMixed, ScUser, Track};
use crate::images::Images;
use crate::query::Query;
use crate::state::PlayerState;

pub enum HomeAction {
    None,
    PlayTrack(Track),
}

#[derive(Default)]
pub struct HomeView {
    me: Query<ScUser>,
    likes: Query<Vec<Track>>,
    discover: Query<DiscoverMixed>,
}

fn greeting(name: Option<&str>) -> String {
    let hour = chrono::Local::now().format("%H").to_string().parse::<u32>().unwrap_or(12);
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
                    v.get("collection").cloned().unwrap_or(serde_json::Value::Null),
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
        ui.heading(greeting(user_name.as_deref()));

        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Liked Tracks");
            if let Some(tracks) = self.likes.data.as_ref() {
                if !tracks.is_empty() {
                    ui.label(format!("{} tracks", tracks.len()));
                }
            }
        });

        if self.likes.loading && self.likes.data.is_none() {
            ui.label("Loading...");
        } else if let Some(err) = self.likes.error.as_ref() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Likes unavailable: {err}"),
            );
        } else if let Some(tracks) = self.likes.data.as_ref() {
            if tracks.is_empty() {
                ui.label("No liked tracks yet");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for track in tracks.iter().take(60) {
                        if let HomeAction::PlayTrack(_) = action {
                            // 最初の1件のみ処理済み扱いはしない (複数クリックは最後が勝つ)
                        }
                        if Self::track_card(ui, rt, images, player, audio, track) {
                            action = HomeAction::PlayTrack(track.clone());
                        }
                    }
                });
            }
        }

        ui.separator();
        if self.discover.loading && self.discover.data.is_none() {
            ui.label("Loading...");
        } else if let Some(mixed) = self.discover.data.as_ref() {
            for sel in &mixed.collection {
                let items: Vec<_> = sel
                    .items
                    .as_ref()
                    .map(|p| p.collection.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .take(10)
                    .collect();
                if items.is_empty() {
                    continue;
                }
                ui.heading(&sel.title);
                egui::ScrollArea::horizontal()
                    .id_salt(format!("discover:{}", sel.urn))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for item in items {
                                ui.vertical(|ui| {
                                    let art = item.artwork_url.as_deref().map(|u| {
                                        u.replace("-large", "-t300x300")
                                    });
                                    images.show(ui, rt, art.as_deref(), 64.0);
                                    ui.add(
                                        egui::Label::new(&item.title)
                                            .truncate()
                                            .wrap_mode(egui::TextWrapMode::Truncate),
                                    );
                                });
                            }
                        });
                    });
            }
        }

        action
    }

    /// 1カード描画。戻り値はクリックされたか。
    fn track_card(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        audio: Option<&Arc<AudioState>>,
        track: &Track,
    ) -> bool {
        let _ = audio;
        let is_current = player
            .current_title
            .as_deref()
            .map(|t| t == track.display_title())
            .unwrap_or(false)
            && player.is_playing;
        let mut clicked = false;
        ui.vertical(|ui| {
            ui.set_max_width(140.0);
            let art = track.artwork("t300x300");
            if images.show(ui, rt, art.as_deref(), 132.0).clicked() {
                clicked = true;
            }
            let title = if is_current {
                format!("▶ {}", track.display_title())
            } else {
                track.display_title().to_string()
            };
            ui.add(
                egui::Label::new(title)
                    .truncate()
                    .wrap_mode(egui::TextWrapMode::Truncate),
            );
            ui.add(
                egui::Label::new(track.artist_name())
                    .truncate()
                    .wrap_mode(egui::TextWrapMode::Truncate),
            );
        });
        clicked
    }
}
