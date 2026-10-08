//! Phase 3: Track — アートワーク + 再生 + 統計 + コメント + 関連トラック。
//! 対応: `desktop/src/pages/TrackPage.tsx` (+ `components/track/` の
//! RoomHero / comments / actions / RelatedRow / RoomSleeve の subset)。
//! `param` (= track urn) をキーに取得し、param 変化で再取得する。
//! 見送り: 波形 (RoomFloor)・オーラ・ライナーノーツ・like/download/follow
//! mutation・コメント削除・無限スクロール (先頭ページのみ)。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Comment, Paged, ScUser, Track, tracks_from_value};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};

pub enum TrackAction {
    None,
    PlayTrack(Track),
    Navigate(Route, Option<String>),
}

#[derive(Default)]
pub struct TrackView {
    track: Query<Track>,
    related: Query<Paged<Track>>,
    comments: Query<Paged<Comment>>,
    favoriters: Query<Paged<ScUser>>,
    post: Query<Comment>,
    last_param: Option<String>,
    comment_draft: String,
    sort_timeline: bool,
}

/// ミリ秒 → `m:ss`。対応: `desktop/src/lib/formatters.ts` の `dur`。
fn fmt_ms(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn count(v: Option<i64>) -> String {
    v.map(|c| c.to_string()).unwrap_or_else(|| "—".to_string())
}

impl TrackView {
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
    ) -> TrackAction {
        let _ = audio;
        let _ = cache;
        let Some(api) = api else {
            ui.label("backend not running");
            return TrackAction::None;
        };
        // param (= track urn) が変わったら再取得。前回 param を保持して比較。
        if self.last_param.as_deref() != param {
            self.track = Query::default();
            self.related = Query::default();
            self.comments = Query::default();
            self.favoriters = Query::default();
            self.post = Query::default();
            self.comment_draft.clear();
            self.last_param = param.map(|s| s.to_string());
        }
        let Some(urn) = param else {
            ui.label("no track selected");
            return TrackAction::None;
        };
        if urn.is_empty() {
            ui.label("no track selected");
            return TrackAction::None;
        }
        let api_owned = api.clone();
        let enc = urlencoding::encode(urn).into_owned();

        // 対応: TrackPage の `api(/tracks/${encodeURIComponent(urn)})`。
        if !self.track.requested() {
            let api = api_owned.clone();
            let path = format!("/tracks/{enc}");
            self.track.request(rt, async move {
                api.get_json(&path)
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
        // 対応: `fetchRelatedTracks` (`/tracks/{urn}/related?limit=10&page=0`)。
        // 404/403/502 は「関連なし」として空扱いにする (React と同様)。
        if !self.related.requested() {
            let api = api_owned.clone();
            let path = format!("/tracks/{enc}/related?limit=10&page=0");
            self.related.request(rt, async move {
                let v = match api.get_json(&path).await {
                    Ok(v) => v,
                    Err(e) => {
                        if e.contains("404") || e.contains("403") || e.contains("502") {
                            return Ok(Paged {
                                collection: Vec::new(),
                                next_href: None,
                            });
                        }
                        return Err(e);
                    }
                };
                Ok(Paged {
                    collection: tracks_from_value(&v),
                    next_href: None,
                })
            });
        }
        // 対応: `useTrackComments` の先頭ページ
        // (`/tracks/{urn}/comments?limit=20&page=0`)。
        if !self.comments.requested() {
            let api = api_owned.clone();
            let path = format!("/tracks/{enc}/comments?limit=20&page=0");
            self.comments.request(rt, async move {
                api.get_json(&path)
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
        // 対応: `useTrackFavoriters`
        // (`/tracks/{urn}/favoriters?limit=12&page=0`)。
        if !self.favoriters.requested() {
            let api = api_owned.clone();
            let path = format!("/tracks/{enc}/favoriters?limit=12&page=0");
            self.favoriters.request(rt, async move {
                api.get_json(&path)
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }

        let mut changed = self.track.poll();
        changed |= self.related.poll();
        changed |= self.comments.poll();
        changed |= self.favoriters.poll();
        changed |= self.post.poll();
        if changed
            || self.track.loading
            || self.related.loading
            || self.comments.loading
            || self.favoriters.loading
        {
            ui.ctx().request_repaint();
        }
        // 投稿成功 → 一覧の先頭に追加して下書きを消す (再取得の代わり)。
        if let Some(comment) = self.post.data.take() {
            if let Some(paged) = self.comments.data.as_mut() {
                paged.collection.insert(0, comment);
            }
            self.comment_draft.clear();
        }

        let mut action = TrackAction::None;

        if let Some(err) = self.track.error.clone() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Failed to load track: {err}"),
            );
            if ui.button("Retry").clicked() {
                self.track = Query::default();
                self.related = Query::default();
                self.comments = Query::default();
                self.favoriters = Query::default();
            }
            return action;
        }
        let Some(track) = self.track.data.clone() else {
            ui.label("Loading...");
            return action;
        };

        // 対応: RoomHero (タイトル/アーティスト/再生ボタン/統計)。
        let is_current = player
            .current_title
            .as_deref()
            .map(|t| t == track.display_title())
            .unwrap_or(false)
            && player.is_playing;
        ui.horizontal(|ui| {
            let art = track.artwork("t500x500");
            images.show(ui, rt, art.as_deref(), 160.0);
            ui.vertical(|ui| {
                ui.heading(track.display_title());
                if let Some(user) = track.user.as_ref() {
                    ui.horizontal(|ui| {
                        ui.label("by");
                        if ui.button(user.username.as_str()).clicked() {
                            action =
                                TrackAction::Navigate(Route::User, Some(user.urn.clone()));
                        }
                    });
                }
                if let Some(genre) = track.genre.as_deref() {
                    if ui.button(format!("# {genre}")).clicked() {
                        action =
                            TrackAction::Navigate(Route::Tag, Some(genre.to_string()));
                    }
                }
                let play_label = if is_current { "⏸ Playing" } else { "▶ Play" };
                if ui.button(play_label).clicked() {
                    action = TrackAction::PlayTrack(track.clone());
                }
                ui.label(format!(
                    "{} plays · {} likes · {} comments · {}",
                    count(track.playback_count),
                    count(track.likes_count),
                    count(track.comment_count),
                    fmt_ms(track.duration),
                ));
            });
        });

        // 対応: RoomSleeve の favoriters (アバター帯。クリックでユーザへ)。
        if let Some(paged) = self.favoriters.data.as_ref() {
            if !paged.collection.is_empty() {
                ui.separator();
                ui.label(format!("{} fans", paged.collection.len()));
                ui.horizontal_wrapped(|ui| {
                    for u in &paged.collection {
                        if images
                            .show(ui, rt, u.avatar_url.as_deref(), 24.0)
                            .on_hover_text(&u.username)
                            .clicked()
                        {
                            action =
                                TrackAction::Navigate(Route::User, Some(u.urn.clone()));
                        }
                    }
                });
            }
        }

        // 対応: RelatedRow (クリックで Track へ遷移、▶で再生)。
        ui.separator();
        ui.heading("Related");
        if self.related.loading && self.related.data.is_none() {
            ui.label("Loading...");
        } else if let Some(err) = self.related.error.as_ref() {
            ui.label(format!("Related unavailable: {err}"));
        } else if let Some(paged) = self.related.data.as_ref() {
            if paged.collection.is_empty() {
                ui.label("No related tracks");
            } else {
                for rel in &paged.collection {
                    ui.horizontal(|ui| {
                        let art = rel.artwork("t200x200");
                        if images.show(ui, rt, art.as_deref(), 48.0).clicked() {
                            action =
                                TrackAction::Navigate(Route::Track, Some(rel.urn.clone()));
                        }
                        ui.vertical(|ui| {
                            if ui.button(rel.display_title()).clicked() {
                                action = TrackAction::Navigate(
                                    Route::Track,
                                    Some(rel.urn.clone()),
                                );
                            }
                            ui.label(rel.artist_name());
                        });
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.button("▶").clicked() {
                                    action = TrackAction::PlayTrack(rel.clone());
                                }
                                ui.label(fmt_ms(rel.duration));
                            },
                        );
                    });
                }
            }
        }

        // 対応: RoomVoices + CommentForm (newest/timeline 切替 + 投稿)。
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Comments");
            if let Some(paged) = self.comments.data.as_ref() {
                ui.label(format!("{} shown", paged.collection.len()));
            }
        });
        // 投稿先は `usePostComment` と同形: POST /tracks/{urn}/comments。
        ui.horizontal(|ui| {
            let sending = self.post.loading;
            ui.add_enabled(
                !sending,
                egui::TextEdit::singleline(&mut self.comment_draft),
            );
            if ui.button("Send").clicked() {
                let body_text = self.comment_draft.trim().to_string();
                if !body_text.is_empty() && !self.post.loading {
                    let api = api_owned.clone();
                    let path = format!("/tracks/{enc}/comments");
                    let body = serde_json::json!({
                        "comment": { "body": body_text, "timestamp": 0 }
                    });
                    self.post.request(rt, async move {
                        api.request_json("POST", &path, Some(&body))
                            .await
                            .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                    });
                }
            }
        });
        if self.post.loading {
            ui.label("Sending...");
        } else if let Some(err) = self.post.error.as_ref() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Post failed: {err}"),
            );
        }
        ui.horizontal(|ui| {
            ui.label("Sort:");
            if ui.selectable_label(!self.sort_timeline, "Newest").clicked() {
                self.sort_timeline = false;
            }
            if ui.selectable_label(self.sort_timeline, "Timeline").clicked() {
                self.sort_timeline = true;
            }
        });
        if self.comments.loading && self.comments.data.is_none() {
            ui.label("Loading...");
        } else if let Some(err) = self.comments.error.as_ref() {
            ui.label(format!("Comments unavailable: {err}"));
        } else {
            let mut list: Vec<Comment> = self
                .comments
                .data
                .as_ref()
                .map(|p| p.collection.clone())
                .unwrap_or_default();
            if self.sort_timeline {
                list.sort_by_key(|c| c.timestamp.unwrap_or(i64::MAX));
            }
            if list.is_empty() {
                ui.label("No comments yet");
            } else {
                for c in &list {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(c.author());
                                if let Some(ts) = c.timestamp {
                                    ui.label(format!("@ {}", fmt_ms(ts)));
                                }
                                if let Some(created) = c.created_at.as_deref() {
                                    ui.label(created);
                                }
                            });
                            ui.label(c.text());
                        });
                    });
                    ui.separator();
                }
            }
        }

        action
    }
}
