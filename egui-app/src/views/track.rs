//! Phase 3: Track — アートワーク + 再生 + 統計 + コメント + 関連トラック。
//! 対応: `desktop/src/pages/TrackPage.tsx` (+ `components/track/` の
//! RoomHero / comments / actions / RelatedRow / RoomSleeve の subset)。
//! `param` (= track urn) をキーに取得し、param 変化で再取得する。
//! 見送り: 波形のコメントレーン・オーラ・ライナーノーツ・無限スクロール (先頭ページのみ)。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::engine;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Comment, LikedFlag, Paged, ScUser, Track, tracks_from_value};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::views::waveform;

pub enum TrackAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (関連トラック一覧がキューになる)。
    PlayList(Vec<Track>, usize),
    /// 次の再生位置に追加 (Tauri 版 `addToQueueNext`)。
    AddNextUp(Track),
    /// 「プレイリストに追加」ダイアログを開く。
    AddToPlaylist(Track),
    /// ダウンロードダイアログを開く。
    OpenDownload(Track),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    Seek(f32),
    Navigate(Route, Option<String>),
}

#[derive(Default)]
pub struct TrackView {
    track: Query<Track>,
    related: Query<Paged<Track>>,
    comments: Query<Paged<Comment>>,
    favoriters: Query<Paged<ScUser>>,
    post: Query<Comment>,
    waveform: Query<Vec<f32>>,
    wave_key: Option<String>,
    last_param: Option<String>,
    comment_draft: String,
    sort_timeline: bool,
    liked: Option<bool>,
    like_count: Option<i64>,
    likes_status: Query<LikedFlag>,
    me: Query<ScUser>,
    /// Uploader follow state (Tauri: RoomSleeve の FollowBtn)。
    follow: Query<bool>,
    follow_urn: Option<String>,
    /// FloatingComments: 再生位置が通過したコメントのピル。
    pills: Vec<FloatingPill>,
    /// 前フレームの再生位置 (自然進行の検出用)。
    last_pos: Option<f64>,
}

/// フローティングコメントのピル (Tauri: FloatingComments)。
struct FloatingPill {
    body: String,
    /// 波形上の位置 (0..1)。
    pct: f32,
    /// 消える時刻 (egui time)。
    until: f64,
}

/// ミリ秒 → `m:ss`。対応: `desktop/src/lib/formatters.ts` の `dur`。
fn fmt_ms(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// 統計の 1 列 (値 + ラベル)。Tauri: RoomSleeve の plays/likes/reposts。
fn stat_column(ui: &mut egui::Ui, value: String, label: &str) {
    ui.vertical(|ui| {
        ui.add(egui::Label::new(
            egui::RichText::new(value).font(crate::theme::semibold(15.0)),
        ));
        ui.label(egui::RichText::new(label).size(10.5).weak());
    });
    ui.add_space(14.0);
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
        accent: egui::Color32,
        floating_comments: bool,
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
            self.waveform = Query::default();
            self.wave_key = None;
            self.comment_draft.clear();
            self.liked = None;
            self.like_count = None;
            self.likes_status = Query::default();
            self.me = Query::default();
            self.follow = Query::default();
            self.follow_urn = None;
            self.pills.clear();
            self.last_pos = None;
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
        // Like 状態 (ローカルストア。cf. `LikeButton.tsx` の `useLiked`)。
        if !self.likes_status.requested() {
            let api = api_owned.clone();
            let path = format!("/likes/tracks/{enc}");
            self.likes_status.request(rt, async move {
                api.get_json(&path)
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
        // 自分のユーザ (コメント所有判定に使う)。
        if !self.me.requested() {
            let api = api_owned.clone();
            self.me.request(rt, async move {
                api.get_json("/me/cold")
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }

        let mut changed = self.track.poll();
        changed |= self.related.poll();
        changed |= self.comments.poll();
        changed |= self.favoriters.poll();
        changed |= self.post.poll();
        changed |= self.waveform.poll();
        changed |= self.likes_status.poll();
        changed |= self.me.poll();
        changed |= self.follow.poll();
        if changed
            || self.track.loading
            || self.related.loading
            || self.comments.loading
            || self.favoriters.loading
            || self.waveform.loading
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
            crate::widgets::loading(ui);
            return action;
        };
        // Like 状態の初期化: ローカルストア優先、無ければ API の user_favorite。
        if self.like_count.is_none() {
            self.like_count = track.likes_count;
        }
        if self.liked.is_none() {
            if let Some(st) = self.likes_status.data {
                self.liked = Some(st.liked || track.user_favorite.unwrap_or(false));
            } else if self.likes_status.error.is_some() {
                self.liked = Some(track.user_favorite.unwrap_or(false));
            }
        }
        // Follow 状態 (Tauri: RoomSleeve の FollowBtn、自分の曲は除外)。
        if let (Some(user), Some(me)) = (track.user.as_ref(), self.me.data.as_ref()) {
            if me.urn != user.urn && self.follow_urn.as_deref() != Some(user.urn.as_str()) {
                self.follow_urn = Some(user.urn.clone());
                self.follow = Query::default();
                let api = api_owned.clone();
                let path = format!(
                    "/users/{}/followings/{}",
                    urlencoding::encode(&me.urn),
                    urlencoding::encode(&user.urn)
                );
                self.follow.request(rt, async move {
                    api.get_json(&path)
                        .await
                        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                });
            }
        }

        // 対応: RoomHero (左: 円形再生 + 大タイトル + メタ + アクション / 右: アートワーク)。
        let is_current = player
            .current_title
            .as_deref()
            .map(|t| t == track.display_title())
            .unwrap_or(false)
            && player.is_playing;
        let total_w = ui.available_width();
        let art_w = 220.0_f32.min((total_w * 0.3).max(140.0));
        let left_w = (total_w - art_w - 18.0).max(280.0);
        ui.horizontal_top(|ui| {
            let _ = ui.allocate_ui_with_layout(
                egui::vec2(left_w, 10.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(left_w);
                    ui.horizontal(|ui| {
                        if crate::widgets::hero_play_button(ui, is_current, true, 56.0).clicked()
                        {
                            action = TrackAction::PlayTrack(track.clone());
                        }
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(track.display_title())
                                    .font(crate::theme::semibold(32.0)),
                            )
                            .wrap(),
                        );
                    });
                    // メタ行: アーティスト · 経過 · ジャンル (Tauri: RoomHero)。
                    ui.horizontal_wrapped(|ui| {
                        if let Some(user) = track.user.as_ref() {
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(&user.username).size(13.5),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                action = TrackAction::Navigate(
                                    Route::User,
                                    Some(user.urn.clone()),
                                );
                            }
                        }
                        let age = crate::widgets::age_text(track.created_at.as_deref());
                        if !age.is_empty() {
                            ui.label(egui::RichText::new("·").weak());
                            ui.label(egui::RichText::new(age).size(12.5).weak());
                        }
                        if let Some(genre) = track.genre.as_deref() {
                            if !genre.is_empty() {
                                ui.label(egui::RichText::new("·").weak());
                                if ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(genre)
                                                .size(12.5)
                                                .color(crate::widgets::genre_color(genre)),
                                        )
                                        .sense(egui::Sense::click()),
                                    )
                                    .clicked()
                                {
                                    action = TrackAction::Navigate(
                                        Route::Tag,
                                        Some(genre.to_string()),
                                    );
                                }
                            }
                        }
                    });
                    // アクション行 (Next up / Add / Download / Liked)。
                    ui.horizontal(|ui| {
                        if ui.button("+ Next up").clicked() {
                            action = TrackAction::AddNextUp(track.clone());
                        }
                        if ui.button("Add to playlist").clicked() {
                            action = TrackAction::AddToPlaylist(track.clone());
                        }
                        if ui.button("Download...").clicked() {
                            action = TrackAction::OpenDownload(track.clone());
                        }
                        let liked = self.liked.unwrap_or(false);
                        if crate::widgets::like_button(ui, liked, accent) {
                            // ローカル即時反映 + writer で best-effort 同期 (cf. LikeButton.tsx)。
                            let next = !liked;
                            self.liked = Some(next);
                            if let Some(c) = self.like_count.as_mut() {
                                *c = (*c + if next { 1 } else { -1 }).max(0);
                            }
                            let api = api_owned.clone();
                            let path = format!("/likes/tracks/{enc}");
                            rt.spawn(async move {
                                let method = if next { "POST" } else { "DELETE" };
                                let _ = api.request_json(method, &path, None).await;
                            });
                        }
                    });
                    ui.add_space(2.0);
                },
            );
            // 右カラム: アートワーク + アップローダー (Follow) + 統計。
            let _ = ui.allocate_ui_with_layout(
                egui::vec2(art_w, 10.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(art_w);
                    let art = track.artwork("t500x500");
                    images.show(ui, rt, art.as_deref(), art_w);
                    if let Some(user) = track.user.as_ref() {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            images.show_rounded(
                                ui,
                                rt,
                                user.avatar_url.as_deref(),
                                40.0,
                                egui::CornerRadius::same(20),
                            );
                            ui.vertical(|ui| {
                                if ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(&user.username)
                                                .font(crate::theme::semibold(13.0)),
                                        )
                                        .sense(egui::Sense::click()),
                                    )
                                    .clicked()
                                {
                                    action = TrackAction::Navigate(
                                        Route::User,
                                        Some(user.urn.clone()),
                                    );
                                }
                                let is_own = self
                                    .me
                                    .data
                                    .as_ref()
                                    .map(|m| m.urn == user.urn)
                                    .unwrap_or(false);
                                if !is_own {
                                    if let Some(following) = self.follow.data {
                                        let label =
                                            if following { "Following" } else { "Follow" };
                                        if ui.small_button(label).clicked() {
                                            let next = !following;
                                            self.follow.data = Some(next);
                                            let api = api_owned.clone();
                                            let path = format!(
                                                "/me/followings/{}",
                                                urlencoding::encode(&user.urn)
                                            );
                                            rt.spawn(async move {
                                                let method =
                                                    if next { "PUT" } else { "DELETE" };
                                                let _ =
                                                    api.request_json(method, &path, None).await;
                                            });
                                        }
                                    }
                                }
                            });
                        });
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        stat_column(ui, count(track.playback_count), "plays");
                        stat_column(ui, count(self.like_count.or(track.likes_count)), "likes");
                        stat_column(ui, count(track.comment_count), "comments");
                    });
                },
            );
        });

        // 波形シーク (RoomFloor 相当。コメントレーンは未対応)。
        if let Some(wave_url) = track.waveform_url.clone() {
            if self.wave_key.as_deref() != Some(wave_url.as_str()) {
                self.waveform = Query::default();
                self.wave_key = Some(wave_url.clone());
                self.waveform.request(rt, async move {
                    waveform::fetch_samples(&wave_url).await
                });
            }
        }
        let duration = player
            .duration_secs
            .unwrap_or_else(|| track.duration_secs());
        let pos_secs = audio.map(|a| engine::get_position(a)).unwrap_or(0.0);
        let progress = if duration > 0.0 {
            (pos_secs / duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        if self.wave_key.is_some() {
            ui.separator();
            // コメント点 (Tauri: WaveVoices)。
            let duration_ms = duration * 1000.0;
            let voices: Vec<waveform::WaveVoice> = self
                .comments
                .data
                .as_ref()
                .map(|paged| {
                    paged
                        .collection
                        .iter()
                        .filter(|c| c.timestamp.is_some() && !c.text().is_empty())
                        .map(|c| waveform::WaveVoice {
                            timestamp_ms: c.timestamp.unwrap_or(0) as f64,
                            body: c.text().to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let hit = waveform::show(
                ui,
                &self.waveform,
                progress,
                96.0,
                accent,
                &voices,
                duration_ms,
            );
            if let Some(frac) = hit.seek {
                action = TrackAction::Seek(frac);
            }
            if let Some(ms) = hit.comment_seek_ms {
                if duration > 0.0 {
                    action = TrackAction::Seek((ms / 1000.0 / duration) as f32);
                }
            }
            // フローティングコメント (Tauri: FloatingComments)。
            let now = ui.input(|i| i.time);
            let is_current = player.is_playing
                && player
                    .current_title
                    .as_deref()
                    .map(|t| t == track.display_title())
                    .unwrap_or(false);
            if !floating_comments || !is_current {
                self.pills.clear();
                self.last_pos = None;
            } else if duration > 0.0 {
                if let Some(prev) = self.last_pos {
                    let delta = pos_secs - prev;
                    // 自然進行のみ (シークの飛びは無視)。
                    if delta > 0.0 && delta < duration * 0.03 {
                        for v in &voices {
                            let ts = v.timestamp_ms / 1000.0;
                            if ts > prev && ts <= pos_secs {
                                self.pills.push(FloatingPill {
                                    body: v.body.clone(),
                                    pct: (v.timestamp_ms / duration_ms) as f32,
                                    until: now + 5.4,
                                });
                            }
                        }
                    }
                }
                self.last_pos = Some(pos_secs);
            }
            self.pills.retain(|p| p.until > now);
            if self.pills.len() > 3 {
                let drop_n = self.pills.len() - 3;
                self.pills.drain(0..drop_n);
            }
            if !self.pills.is_empty() && hit.rect != egui::Rect::NOTHING {
                let painter = ui.painter();
                let font = egui::FontId::proportional(11.5);
                let text_color = egui::Color32::from_white_alpha(225);
                for p in &self.pills {
                    let galley = painter.layout(p.body.clone(), font.clone(), text_color, 300.0);
                    let pad = egui::vec2(9.0, 6.0);
                    let size = galley.size() + pad * 2.0;
                    let x = (hit.rect.left() + p.pct * hit.rect.width() - size.x / 2.0).clamp(
                        hit.rect.left(),
                        (hit.rect.right() - size.x).max(hit.rect.left()),
                    );
                    let rect = egui::Rect::from_min_size(
                        egui::Pos2::new(x, hit.rect.bottom() + 8.0),
                        size,
                    );
                    painter.rect_filled(rect, 7.0, egui::Color32::from_rgb(27, 27, 31));
                    painter.rect_stroke(
                        rect,
                        7.0,
                        egui::Stroke::new(0.5, egui::Color32::from_white_alpha(26)),
                        egui::StrokeKind::Inside,
                    );
                    painter.galley(rect.min + pad, galley, text_color);
                }
                // ピルの寿命で再描画を維持する。
                ui.ctx().request_repaint();
            }
        }

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
            crate::widgets::loading(ui);
        } else if let Some(err) = self.related.error.as_ref() {
            ui.label(format!("Related unavailable: {err}"));
        } else if let Some(paged) = self.related.data.as_ref() {
            if paged.collection.is_empty() {
                crate::widgets::empty_note(ui, "No related tracks");
            } else {
                for (i, rel) in paged.collection.iter().enumerate() {
                    let row = ui
                        .horizontal(|ui| {
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
                                        action =
                                            TrackAction::PlayList(paged.collection.clone(), i);
                                    }
                                    ui.label(fmt_ms(rel.duration));
                                },
                            );
                        })
                        .response
                        .interact(egui::Sense::click());
                    if row.secondary_clicked() {
                        action = TrackAction::OpenMenu(rel.clone());
                    }
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
            crate::widgets::loading(ui);
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
                crate::widgets::empty_note(ui, "No comments yet");
            } else {
                let me_urn = self.me.data.as_ref().map(|u| u.urn.clone());
                let mut delete_id: Option<i64> = None;
                for c in &list {
                    let is_mine = matches!(
                        (&me_urn, c.user.as_ref()),
                        (Some(m), Some(u)) if m == &u.urn
                    );
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
                        if is_mine {
                            if let Some(id) = c.id {
                                if ui.small_button("Delete").clicked() {
                                    delete_id = Some(id);
                                }
                            }
                        }
                    });
                    ui.separator();
                }
                // 一覧から即座に消し、writer で best-effort 同期
                // (cf. `comments.tsx` の `remove` → DELETE /comments/:id)。
                if let Some(id) = delete_id {
                    if let Some(paged) = self.comments.data.as_mut() {
                        paged.collection.retain(|c| c.id != Some(id));
                    }
                    let api = api_owned.clone();
                    let path = format!("/comments/{id}");
                    rt.spawn(async move {
                        let _ = api.request_json("DELETE", &path, None).await;
                    });
                }
            }
        }

        action
    }
}
