//! Phase 3: Album — カバー + タイトル + トラック行リスト。
//! 対応: `desktop/src/pages/AlbumPage.tsx` (+ `components/album/` の
//! `AlbumHero` / `AlbumTrackList` / `AlbumTrackRow`)。
//! アーティストの star/aura 装飾・wanted (未索引) 区画・ジャンル演出は見送り。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Album, Track};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};

pub enum AlbumAction {
    None,
    PlayTrack(Track),
    Navigate(Route, Option<String>),
}

#[derive(Default)]
pub struct AlbumView {
    album: Query<Album>,
    last_id: Option<String>,
}

/// ミリ秒 → `m:ss` / `h:mm:ss`。対応: `desktop/src/lib/formatters.ts` の `dur()`。
fn fmt_dur(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

impl AlbumView {
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
        ui: &mut egui::Ui,
    ) -> AlbumAction {
        let _ = audio;
        let _ = cache;
        let Some(api) = api else {
            ui.label("backend not running");
            return AlbumAction::None;
        };
        let Some(id) = param else {
            ui.label("no album selected");
            return AlbumAction::None;
        };
        // param (= album id/urn) 変化で再取得。対応: `useAlbumDetail(id)` のキー変化。
        if self.last_id.as_deref() != Some(id) {
            self.album = Query::default();
            self.last_id = Some(id.to_string());
        }
        let api_owned = api.clone();
        if !self.album.requested() {
            let api = api_owned.clone();
            let path = format!("/albums/{}", urlencoding::encode(id));
            self.album.request(rt, async move {
                let mut v = api.get_json(&path).await?;
                // React の `AlbumDetail` は `cover_url` / `id` 持ち。
                // egui の `Album` (`artwork_url` / `urn`) に寄せてから読む。
                if v.get("artwork_url").is_none() {
                    if let Some(c) = v.get("cover_url").cloned() {
                        v["artwork_url"] = c;
                    }
                }
                if v.get("urn").is_none() {
                    if let Some(alpha) = v.get("id").cloned() {
                        let s = match &alpha {
                            serde_json::Value::String(s) => s.clone(),
                            _ => alpha.to_string(),
                        };
                        v["urn"] = serde_json::Value::String(s);
                    }
                }
                serde_json::from_value(v).map_err(|e| e.to_string())
            });
        }

        if self.album.poll() || self.album.loading {
            ui.ctx().request_repaint();
        }

        let mut action = AlbumAction::None;

        if self.album.loading && self.album.data.is_none() {
            ui.label("Loading...");
            return action;
        }
        if let Some(err) = self.album.error.as_ref() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Album unavailable: {err}"),
            );
            return action;
        }
        let Some(album) = self.album.data.as_ref() else {
            ui.label("Loading...");
            return action;
        };

        // 対応: `AlbumDetail.tracks`。`Album::track_list()` が寛容パース済み。
        let tracks: Vec<Track> = album.track_list();
        let cover = album.artwork("t500x500").or_else(|| {
            tracks
                .first()
                .and_then(|t| t.artwork("t500x500"))
        });

        // Hero (対応: `AlbumHero`)。
        ui.horizontal(|ui| {
            images.show(ui, rt, cover.as_deref(), 180.0);
            ui.vertical(|ui| {
                ui.label("Album");
                ui.heading(&album.title);
                if let Some(user) = album.user.as_ref() {
                    if ui
                        .selectable_label(false, user.username.clone())
                        .clicked()
                    {
                        action =
                            AlbumAction::Navigate(Route::Artist, Some(user.urn.clone()));
                    }
                }
                let total_ms: i64 = tracks.iter().map(|t| t.duration).sum();
                ui.label(format!(
                    "{} {} — {}",
                    tracks.len(),
                    if tracks.len() == 1 { "track" } else { "tracks" },
                    fmt_dur(total_ms)
                ));
                if let Some(desc) = album.description.as_ref() {
                    if !desc.is_empty() {
                        ui.label(desc);
                    }
                }
                if ui.button("▶ Play").clicked() {
                    if let Some(first) = tracks.first() {
                        action = AlbumAction::PlayTrack(first.clone());
                    }
                }
            });
        });

        // Tracklist (対応: `AlbumTrackList` / `AlbumTrackRow`)。
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Tracks");
            ui.label(format!("{}", tracks.len()));
        });
        if tracks.is_empty() {
            ui.label("No tracks indexed yet");
            return action;
        }
        for (i, track) in tracks.iter().enumerate() {
            if Self::track_row(ui, rt, images, player, i, track) {
                action = AlbumAction::PlayTrack(track.clone());
            }
        }

        action
    }

    /// 1行描画。戻り値はクリックされたか (対応: `AlbumTrackRow`)。
    fn track_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        index: usize,
        track: &Track,
    ) -> bool {
        let is_current = player
            .current_title
            .as_deref()
            .map(|t| t == track.display_title())
            .unwrap_or(false)
            && player.is_playing;
        let mut clicked = false;
        ui.horizontal(|ui| {
            ui.label(format!("{:>3}", index + 1));
            let art = track.artwork("t200x200");
            if images.show(ui, rt, art.as_deref(), 36.0).clicked() {
                clicked = true;
            }
            ui.vertical(|ui| {
                ui.set_min_width(160.0);
                let title = if is_current {
                    format!("▶ {}", track.display_title())
                } else {
                    track.display_title().to_string()
                };
                if ui.selectable_label(is_current, title).clicked() {
                    clicked = true;
                }
                ui.label(track.artist_name());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Play").clicked() {
                    clicked = true;
                }
                ui.monospace(fmt_dur(track.duration));
            });
        });
        clicked
    }
}
