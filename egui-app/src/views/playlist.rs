//! Phase 3: Playlist — カバー + タイトル + トラック行リスト。
//! 対応: `desktop/src/pages/PlaylistPage.tsx` (+ `components/playlist/` の
//! `PlaylistHero` / `SequenceList` / `SequenceRow`)。
//! D&D 並替・トラック追加/削除・pin/delete・MoreCrates・無限スクロールは見送り。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{LikedFlag, Playlist, ScUser, Track, tracks_from_value};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum PlaylistAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (プレイリスト全曲がキューになる)。
    PlayList(Vec<Track>, usize),
    /// シャッフル有効で先頭から再生。
    ShufflePlay(Vec<Track>),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    /// サイドバーへの pin トグル。
    TogglePin(String, String),
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
    me: Query<ScUser>,
    /// 編集 (削除/並替) 後のローカル上書き。
    edit_tracks: Option<Vec<Track>>,
    /// 公開範囲 (owner のみ変更可)。
    sharing: Option<String>,
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
    /// 現在表示中のプレイリスト URN (継続ソースの arm 用)。
    pub fn urn(&self) -> Option<&str> {
        self.last_urn.as_deref()
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
        settings: &crate::state::SettingsState,
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
            self.me = Query::default();
            self.edit_tracks = None;
            self.sharing = None;
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
        // 自分のユーザ (owner 判定)。
        if !self.me.requested() {
            let api = api_owned.clone();
            self.me.request(rt, async move {
                api.get_json("/me/cold")
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }

        let mut changed = self.detail.poll();
        changed |= self.tracks.poll();
        changed |= self.likes_status.poll();
        changed |= self.me.poll();
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
        let mut tracks: Vec<Track> = if remote.is_empty() {
            playlist.track_list()
        } else {
            remote
        };
        // 編集 (削除/並替) のローカル上書きを優先。
        if let Some(edit) = self.edit_tracks.as_ref() {
            tracks = edit.clone();
        }
        // owner 判定 (ローカル作成のプレイリストは常に編集可)。
        let is_owner = playlist.urn.starts_with("local:")
            || self
                .me
                .data
                .as_ref()
                .map(|m| {
                    playlist
                        .user
                        .as_ref()
                        .map(|u| u.urn == m.urn)
                        .unwrap_or(false)
                })
                .unwrap_or(false);
        if self.sharing.is_none() {
            self.sharing = Some(
                playlist
                    .sharing
                    .clone()
                    .unwrap_or_else(|| "private".to_string()),
            );
        }
        let sharing = self.sharing.clone().unwrap_or_else(|| "private".into());
        let pinned = settings
            .pinned_playlists
            .iter()
            .any(|p| p.urn == playlist.urn);
        // React (`rawPlaylistCover`) 同様、カバー欠損時は先頭トラックから借用。
        let cover = playlist
            .artwork("t500x500")
            .or_else(|| tracks.first().and_then(|t| t.artwork("t500x500")));

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
                        action = PlaylistAction::Navigate(Route::User, Some(user.urn.clone()));
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
                    if ui.button("Shuffle").clicked() && !tracks.is_empty() {
                        action = PlaylistAction::ShufflePlay(tracks.clone());
                    }
                    if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                        action =
                            PlaylistAction::TogglePin(playlist.urn.clone(), playlist.title.clone());
                    }
                });
                if is_owner {
                    ui.horizontal(|ui| {
                        ui.label("Sharing:");
                        let mut new_sharing: Option<&'static str> = None;
                        if ui
                            .selectable_label(sharing == "private", "Private")
                            .clicked()
                        {
                            new_sharing = Some("private");
                        }
                        if ui.selectable_label(sharing == "public", "Public").clicked() {
                            new_sharing = Some("public");
                        }
                        if let Some(sh) = new_sharing {
                            if sh != sharing {
                                self.sharing = Some(sh.to_string());
                                let api = api_owned.clone();
                                let path =
                                    format!("/playlists/{}/sharing", urlencoding::encode(urn));
                                let body = serde_json::json!({ "sharing": sh });
                                rt.spawn(async move {
                                    let _ = api.request_json("PUT", &path, Some(&body)).await;
                                });
                            }
                        }
                        if ui.button("Delete playlist").clicked() {
                            let api = api_owned.clone();
                            let path = format!("/playlists/{}", urlencoding::encode(urn));
                            rt.spawn(async move {
                                let _ = api.request_json("DELETE", &path, None).await;
                            });
                            action = PlaylistAction::Navigate(Route::Library, None);
                        }
                    });
                }
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
        let mut remove_at: Option<usize> = None;
        let mut move_pair: Option<(usize, usize)> = None;
        for (i, track) in tracks.iter().enumerate() {
            if is_owner {
                let row_id = egui::Id::new(("pl-row", urn, i));
                let (zone, dropped) = ui.dnd_drop_zone::<usize, _>(egui::Frame::NONE, |ui| {
                    ui.horizontal(|ui| {
                        let removed = ui.small_button("x").clicked();
                        let hit = ui
                            .dnd_drag_source(row_id, i, |ui| {
                                Self::track_row(ui, rt, images, player, track, accent)
                            })
                            .inner;
                        (hit, removed)
                    })
                    .inner
                });
                let (hit, removed) = zone.inner;
                match hit {
                    widgets::RowHit::Clicked => {
                        action = PlaylistAction::PlayList(tracks.clone(), i);
                    }
                    widgets::RowHit::Menu => {
                        action = PlaylistAction::OpenMenu(track.clone());
                    }
                    widgets::RowHit::None => {}
                }
                if removed {
                    remove_at = Some(i);
                }
                if let Some(from) = dropped {
                    move_pair = Some((*from, i));
                }
            } else {
                match Self::track_row(ui, rt, images, player, track, accent) {
                    widgets::RowHit::Clicked => {
                        action = PlaylistAction::PlayList(tracks.clone(), i);
                    }
                    widgets::RowHit::Menu => {
                        action = PlaylistAction::OpenMenu(track.clone());
                    }
                    widgets::RowHit::None => {}
                }
            }
        }
        let mut edit_changed = false;
        if let Some(i) = remove_at {
            if i < tracks.len() {
                tracks.remove(i);
                edit_changed = true;
            }
        }
        if let Some((from, to)) = move_pair {
            if from != to && from < tracks.len() && to < tracks.len() {
                let t = tracks.remove(from);
                tracks.insert(to, t);
                edit_changed = true;
            }
        }
        if edit_changed {
            self.edit_tracks = Some(tracks.clone());
            let urns: Vec<String> = tracks.iter().map(|t| t.urn.clone()).collect();
            let api = api_owned.clone();
            let path = format!("/playlists/{}/tracks", urlencoding::encode(urn));
            let body = serde_json::json!({ "order": urns });
            rt.spawn(async move {
                let _ = api.request_json("POST", &path, Some(&body)).await;
            });
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
    ) -> widgets::RowHit {
        let playing = widgets::is_currently_playing(player, track);
        let dur = fmt_dur(track.duration);
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
}
