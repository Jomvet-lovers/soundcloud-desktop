//! Phase 3: Playlist — カバー + タイトル + トラック行リスト。
//! 対応: `desktop/src/pages/PlaylistPage.tsx` (+ `components/playlist/` の
//! `PlaylistHero` / `SequenceList` / `SequenceRow`)。
//! D&D 並替・トラック追加/削除・pin/delete・MoreCrates・無限スクロールは見送り。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{tracks_from_value, Playlist, Track};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};

pub enum PlaylistAction {
    None,
    PlayTrack(Track),
    Navigate(Route, Option<String>),
}

#[derive(Default)]
pub struct PlaylistView {
    detail: Query<Playlist>,
    tracks: Query<Vec<Track>>,
    last_urn: Option<String>,
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

impl PlaylistView {
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
    ) -> PlaylistAction {
        let _ = audio;
        let _ = cache;
        let Some(api) = api else {
            ui.label("backend not running");
            return PlaylistAction::None;
        };
        let Some(urn) = param else {
            ui.label("no playlist selected");
            return PlaylistAction::None;
        };
        // param (= playlist urn) 変化で再取得。対応: `usePlaylist(urn)` のキー変化。
        if self.last_urn.as_deref() != Some(urn) {
            self.detail = Query::default();
            self.tracks = Query::default();
            self.last_urn = Some(urn.to_string());
        }
        let api_owned = api.clone();
        if !self.detail.requested() {
            let api = api_owned.clone();
            let path = format!("/playlists/{}", urlencoding::encode(urn));
            self.detail.request(rt, async move {
                api.get_json(&path)
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
        if !self.tracks.requested() {
            let api = api_owned.clone();
            let base = format!("/playlists/{}", urlencoding::encode(urn));
            self.tracks.request(rt, async move {
                // 対応: `usePlaylistTracks` (autoFetchAll, limit=200)。
                let mut all = Vec::new();
                for page in 0..10 {
                    let v = api
                        .get_json(&format!("{base}/tracks?limit=200&page={page}"))
                        .await?;
                    let batch = tracks_from_value(&v);
                    let n = batch.len();
                    all.extend(batch);
                    if n < 200 {
                        break;
                    }
                }
                Ok(all)
            });
        }

        let mut changed = self.detail.poll();
        changed |= self.tracks.poll();
        if changed || self.detail.loading || self.tracks.loading {
            ui.ctx().request_repaint();
        }

        let mut action = PlaylistAction::None;

        if self.detail.loading && self.detail.data.is_none() {
            ui.label("Loading...");
            return action;
        }
        if let Some(err) = self.detail.error.as_ref() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Playlist unavailable: {err}"),
            );
            return action;
        }
        let Some(playlist) = self.detail.data.as_ref() else {
            ui.label("Loading...");
            return action;
        };

        // 対応: `serverTracks` memo。`/tracks` が空なら詳細同梱の `tracks` に退避。
        let remote = self.tracks.data.as_ref().cloned().unwrap_or_default();
        let tracks: Vec<Track> = if remote.is_empty() {
            playlist.track_list()
        } else {
            remote
        };
        // React (`rawPlaylistCover`) 同様、カバー欠損時は先頭トラックから借用。
        let cover = playlist.artwork("t500x500").or_else(|| {
            tracks
                .first()
                .and_then(|t| t.artwork("t500x500"))
        });

        // Hero (対応: `PlaylistHero`)。
        ui.horizontal(|ui| {
            images.show(ui, rt, cover.as_deref(), 180.0);
            ui.vertical(|ui| {
                ui.label("Playlist");
                ui.heading(&playlist.title);
                if let Some(user) = playlist.user.as_ref() {
                    if ui
                        .selectable_label(false, format!("Curated by {}", user.username))
                        .clicked()
                    {
                        action =
                            PlaylistAction::Navigate(Route::User, Some(user.urn.clone()));
                    }
                }
                let total_ms: i64 = tracks.iter().map(|t| t.duration).sum();
                ui.label(format!("{} tracks — {}", tracks.len(), fmt_dur(total_ms)));
                if let Some(desc) = playlist.description.as_ref() {
                    if !desc.is_empty() {
                        ui.label(desc);
                    }
                }
                if ui.button("▶ Play all").clicked() {
                    if let Some(first) = tracks.first() {
                        action = PlaylistAction::PlayTrack(first.clone());
                    }
                }
            });
        });

        // Sequence (対応: `SequenceList`。owner/非owner の分岐は read-only に統一)。
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Tracks");
            ui.label(format!("{}", tracks.len()));
        });
        if tracks.is_empty() {
            if let Some(err) = self.tracks.error.as_ref() {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 150, 150),
                    format!("tracks unavailable: {err}"),
                );
            } else if self.tracks.loading {
                ui.label("Loading tracks...");
            } else {
                ui.label("This crate is empty — start digging");
            }
            return action;
        }
        for (i, track) in tracks.iter().enumerate() {
            if Self::track_row(ui, rt, images, player, i, track) {
                action = PlaylistAction::PlayTrack(track.clone());
            }
        }

        action
    }

    /// 1行描画。戻り値はクリックされたか (対応: `SequenceRow`)。
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
