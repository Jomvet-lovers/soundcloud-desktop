//! Phase 3: Playlist — カバー + タイトル + トラック行リスト。
//! 対応: `desktop/src/pages/PlaylistPage.tsx` (+ `components/playlist/` の
//! `PlaylistHero` / `SequenceList` / `SequenceRow`)。
//! D&D 並替・トラック追加/削除・pin/delete・MoreCrates・無限スクロールは見送り。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{LikedFlag, Playlist, ScUser, Track, tracks_from_value};
use crate::images::Images;
use crate::pager::ListPage;
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
    TogglePin(String, String, Option<String>),
    Navigate(Route, Option<String>),
}

#[derive(Default)]
pub struct PlaylistView {
    detail: Query<Playlist>,
    tracks: Query<Vec<Track>>,
    last_urn: Option<String>,
    liked: Option<bool>,
    like_count: Option<i64>,
    /// Like ボタンの pulse 開始時刻 (egui time、560ms)。
    like_pulse_at: Option<f64>,
    likes_status: Query<LikedFlag>,
    me: Query<ScUser>,
    /// 編集 (削除/並替) 後のローカル上書き。
    edit_tracks: Option<Vec<Track>>,
    /// 公開範囲 (owner のみ変更可)。
    sharing: Option<String>,
    /// More crates (キュレーターの他プレイリスト。Tauri: `MoreCrates`)。
    more: Query<Vec<Playlist>>,
    more_for: Option<String>,
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

    /// 追加/並替後などに曲リストを取り直す (Tauri: invalidateQueries 相当)。
    pub fn invalidate_tracks(&mut self) {
        self.detail = Query::default();
        self.tracks = Query::default();
        self.edit_tracks = None;
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
            self.like_pulse_at = None;
            self.likes_status = Query::default();
            self.me = Query::default();
            self.edit_tracks = None;
            self.sharing = None;
            self.more = Query::default();
            self.more_for = None;
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
        changed |= self.more.poll();
        if changed || self.detail.loading || self.tracks.loading {
            ui.ctx().request_repaint();
        }

        let mut action = PlaylistAction::None;

        if self.detail.loading && self.detail.data.is_none() {
            widgets::loading(ui);
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
            widgets::loading(ui);
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
        // More crates: キュレーターの他プレイリスト (Tauri: `MoreCrates`)。
        if let Some(curator) = playlist.user.as_ref() {
            if self.more_for.as_deref() != Some(curator.urn.as_str()) {
                self.more_for = Some(curator.urn.clone());
                self.more = Query::default();
                let api = api_owned.clone();
                let path = format!(
                    "/users/{}/playlists?limit=30&page=0",
                    urlencoding::encode(&curator.urn)
                );
                self.more.request(rt, async move {
                    let v = api.get_json(&path).await?;
                    let page: ListPage<Playlist> =
                        serde_json::from_value(v).map_err(|e| e.to_string())?;
                    Ok(page.collection)
                });
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
        ui.horizontal_top(|ui| {
            images.show(ui, rt, cover.as_deref(), 180.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if crate::widgets::hero_play_button(
                        ui,
                        false,
                        !tracks.is_empty(),
                        56.0,
                    )
                    .clicked()
                        && !tracks.is_empty()
                    {
                        action = PlaylistAction::PlayList(tracks.clone(), 0);
                    }
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&playlist.title)
                                .font(crate::theme::semibold(40.0)),
                        )
                        .wrap(),
                    );
                });
                if let Some(user) = playlist.user.as_ref() {
                    if ui
                        .selectable_label(false, format!("Curated by {}", user.username))
                        .clicked()
                    {
                        action = PlaylistAction::Navigate(Route::User, Some(user.urn.clone()));
                    }
                }
                let total_ms: i64 = tracks.iter().map(|t| t.duration).sum();
                ui.label(
                    egui::RichText::new(format!(
                        "Set · {} tracks — {}",
                        tracks.len(),
                        fmt_dur(total_ms)
                    ))
                    .size(12.0)
                    .weak(),
                );
                if let Some(desc) = playlist.description.as_ref() {
                    if !desc.is_empty() {
                        ui.label(desc);
                    }
                }
                // Like ボタンの pulse (Tauri: usePulseHeart、560ms)。
                let now = ui.input(|i| i.time);
                let pulse_t = self
                    .like_pulse_at
                    .map(|t0| (((now - t0) / 0.560) as f32).clamp(0.0, 1.0));
                if let Some(t) = pulse_t {
                    if t >= 1.0 {
                        self.like_pulse_at = None;
                    } else {
                        ui.ctx().request_repaint();
                    }
                }
                let (heart_scale, pill_scale) = crate::widgets::pulse_scales(pulse_t);
                ui.horizontal(|ui| {
                    let liked = self.liked.unwrap_or(false);
                    // pulse 中は色の切替を最小フレーム (40%) まで遅らせる。
                    let shown_liked = if pulse_t.map(|t| t < 0.4).unwrap_or(false) {
                        !liked
                    } else {
                        liked
                    };
                    let count = self.like_count.unwrap_or(0);
                    if crate::widgets::like_ghost(
                        ui,
                        shown_liked,
                        count,
                        accent,
                        heart_scale,
                        pill_scale,
                    )
                    .clicked()
                    {
                        // ローカル即時反映 + writer で best-effort 同期。
                        let next = !liked;
                        self.like_pulse_at = Some(now);
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
                    if ui.button("Shuffle").clicked() && !tracks.is_empty() {
                        action = PlaylistAction::ShufflePlay(tracks.clone());
                    }
                    if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                        action =
                            PlaylistAction::TogglePin(
                                playlist.urn.clone(),
                                playlist.title.clone(),
                                playlist.artwork("t200x200"),
                            );
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
                widgets::loading_text(ui, "Loading tracks...");
            } else {
                widgets::empty_note(ui, "This crate is empty — start digging");
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
                if hit.play_clicked() {
                    action = PlaylistAction::PlayList(tracks.clone(), i);
                } else if hit.title_clicked() {
                    action = PlaylistAction::Navigate(Route::Track, Some(track.urn.clone()));
                } else if hit.artist_clicked() {
                    if let Some(u) = track.user.as_ref() {
                        action = PlaylistAction::Navigate(Route::User, Some(u.urn.clone()));
                    }
                } else if hit.menu_clicked() {
                    action = PlaylistAction::OpenMenu(track.clone());
                }
                if removed {
                    remove_at = Some(i);
                }
                if let Some(from) = dropped {
                    move_pair = Some((*from, i));
                }
            } else {
                let hit = Self::track_row(ui, rt, images, player, track, accent);
                if hit.play_clicked() {
                    action = PlaylistAction::PlayList(tracks.clone(), i);
                } else if hit.title_clicked() {
                    action = PlaylistAction::Navigate(Route::Track, Some(track.urn.clone()));
                } else if hit.artist_clicked() {
                    if let Some(u) = track.user.as_ref() {
                        action = PlaylistAction::Navigate(Route::User, Some(u.urn.clone()));
                    }
                } else if hit.menu_clicked() {
                    action = PlaylistAction::OpenMenu(track.clone());
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

        // More crates by {curator} (Tauri: `MoreCrates`)。
        if let Some(curator) = playlist.user.as_ref() {
            if let Some(list) = self.more.data.as_ref() {
                let others: Vec<Playlist> = list
                    .iter()
                    .filter(|p| p.urn != playlist.urn)
                    .take(12)
                    .cloned()
                    .collect();
                if !others.is_empty() {
                    ui.separator();
                    widgets::section_header(
                        ui,
                        &format!("More crates by {}", curator.username),
                        None,
                    );
                    egui::ScrollArea::horizontal()
                        .id_salt("playlist:more")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for p in &others {
                                    let clicked = ui
                                        .vertical(|ui| {
                                            let art = p.artwork("t200x200");
                                            let img =
                                                images.show(ui, rt, art.as_deref(), 96.0);
                                            let lbl = ui.add(
                                                egui::Label::new(&p.title)
                                                    .truncate()
                                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                                    .sense(egui::Sense::click()),
                                            );
                                            img.clicked() || lbl.clicked()
                                        })
                                        .inner;
                                    if clicked {
                                        action = PlaylistAction::Navigate(
                                            Route::Playlist,
                                            Some(p.urn.clone()),
                                        );
                                    }
                                }
                            });
                        });
                }
            }
        }

        action
    }

    /// 1行描画。戻り値は操作結果 (対応: `SequenceRow`)。
    fn track_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        track: &Track,
        accent: egui::Color32,
    ) -> widgets::RowParts {
        let playing = widgets::is_currently_playing(player, track);
        let dur = fmt_dur(track.duration);
        widgets::track_row(
            ui,
            rt,
            images,
            track,
            playing,
            accent,
            Some(&dur),
        )
    }
}
