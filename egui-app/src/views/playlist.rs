//! Phase 3: Playlist — カバー + タイトル + トラック行リスト。
//! 対応: `desktop/src/pages/PlaylistPage.tsx` (+ `components/playlist/` の
//! `PlaylistHero` / `SequenceList` / `SequenceRow`)。
//! D&D 並替・トラック追加/削除・pin/delete・MoreCrates・無限スクロールは見送り。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{LikedFlag, Playlist, Track, tracks_from_value};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum PlaylistAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (プレイリスト全曲がキューになる)。
    PlayList(Vec<Track>, usize),
    Navigate(Route, Option<String>),
}

#[derive(Default)]
pub struct PlaylistView {
    detail: Query<Playlist>,
    tracks: Query<Vec<Track>>,
    last_urn: Option<String>,
    liked: Option<bool>,
    like_count: Option<i64>,
    likes_status: Query<LikedFlag>,
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
        accent: egui::Color32,
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
            self.liked = None;
            self.like_count = None;
            self.likes_status = Query::default();
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
        // Like 状態 (ローカルストア。cf. `PlaylistActions.tsx` の PlaylistLikeBtn)。
        if !self.likes_status.requested() {
            let api = api_owned.clone();
            let path = format!("/likes/playlists/{}", urlencoding::encode(urn));
            self.likes_status.request(rt, async move {
                api.get_json(&path)
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }

        let mut changed = self.detail.poll();
        changed |= self.tracks.poll();
        changed |= self.likes_status.poll();
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
        if self.like_count.is_none() {
            self.like_count = playlist.likes_count;
        }
        if self.liked.is_none() {
            if let Some(st) = self.likes_status.data {
                self.liked = Some(st.liked);
            } else if self.likes_status.error.is_some() {
                self.liked = Some(false);
            }
        }

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
                ui.horizontal(|ui| {
                    if ui.button("▶ Play all").clicked() {
                        if !tracks.is_empty() {
                            action = PlaylistAction::PlayList(tracks.clone(), 0);
                        }
                    }
                    let liked = self.liked.unwrap_or(false);
                    if crate::widgets::like_button(ui, liked, accent) {
                        // ローカル即時反映 + writer で best-effort 同期。
                        let next = !liked;
                        self.liked = Some(next);
                        if let Some(c) = self.like_count.as_mut() {
                            *c = (*c + if next { 1 } else { -1 }).max(0);
                        }
                        let api = api_owned.clone();
                        let path = format!("/likes/playlists/{}", urlencoding::encode(urn));
                        rt.spawn(async move {
                            let method = if next { "POST" } else { "DELETE" };
                            let _ = api.request_json(method, &path, None).await;
                        });
                    }
                    if let Some(c) = self.like_count {
                        ui.label(if c >= 1000 {
                            format!("{:.1}k", c as f64 / 1000.0)
                        } else {
                            c.to_string()
                        });
                    }
                });
            });
        });

        // Sequence (対応: `SequenceList`。owner/非owner の分岐は read-only に統一)。
        ui.separator();
        widgets::section_header(ui, "Tracks", Some(tracks.len()));
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
            if Self::track_row(ui, rt, images, player, track, accent) {
                action = PlaylistAction::PlayList(tracks.clone(), i);
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
        track: &Track,
        accent: egui::Color32,
    ) -> bool {
        let playing = widgets::is_currently_playing(player, track);
        let dur = fmt_dur(track.duration);
        widgets::track_row(ui, rt, images, track, playing, accent, Some(&dur)).clicked()
    }
}
