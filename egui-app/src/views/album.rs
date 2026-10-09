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
use crate::widgets;

pub enum AlbumAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (アルバム全曲がキューになる)。
    PlayList(Vec<Track>, usize),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
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
        accent: egui::Color32,
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
            widgets::loading(ui);
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
            widgets::loading(ui);
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
                        action = AlbumAction::PlayList(tracks.clone(), 0);
                    }
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&album.title)
                                .font(crate::theme::semibold(28.0)),
                        )
                        .wrap(),
                    );
                });
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
                ui.label(
                    egui::RichText::new(format!(
                        "Album · {} {} — {}",
                        tracks.len(),
                        if tracks.len() == 1 { "track" } else { "tracks" },
                        fmt_dur(total_ms)
                    ))
                    .size(12.0)
                    .weak(),
                );
                if let Some(desc) = album.description.as_ref() {
                    if !desc.is_empty() {
                        ui.label(desc);
                    }
                }
            });
        });

        // Tracklist (対応: `AlbumTrackList` / `AlbumTrackRow`)。
        ui.separator();
        widgets::section_header(ui, "Tracks", Some(tracks.len()));
        if tracks.is_empty() {
            widgets::empty_note(ui, "No tracks indexed yet");
            return action;
        }
        for (i, track) in tracks.iter().enumerate() {
            let hit = Self::track_row(ui, rt, images, player, track, accent);
            if hit.play_clicked() {
                action = AlbumAction::PlayList(tracks.clone(), i);
            } else if hit.title_clicked() {
                action = AlbumAction::Navigate(Route::Track, Some(track.urn.clone()));
            } else if hit.artist_clicked() {
                if let Some(u) = track.user.as_ref() {
                    action = AlbumAction::Navigate(Route::User, Some(u.urn.clone()));
                }
            } else if hit.menu_clicked() {
                action = AlbumAction::OpenMenu(track.clone());
            }
        }

        action
    }

    /// 1行描画。戻り値は操作結果 (対応: `AlbumTrackRow`)。
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
