//! Phase 3: Tag — タグ名 (`param`) のトラックフィード。
//! 対応: `desktop/src/pages/TagPage.tsx`。
//! エンドポイント: `GET /tags/{tag}/tracks?limit=&page=&sort=`
//! (`egui-app/src/backend/direct/routes/tracks.rs` の `["tags", tag, "tracks"]`)。
//! 番号ページング + ソート切替。表示優先でカード装飾は簡略化。

use std::sync::Arc;

use serde::Deserialize;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::Track;
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum TagAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (表示中ページ全体がキューになる)。
    PlayList(Vec<Track>, usize),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    Navigate(Route, Option<String>),
}

/// `TagSort` (`desktop/src/lib/hooks.ts`)。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TagSort {
    #[default]
    Newest,
    Plays,
    Likes,
}

impl TagSort {
    const ALL: [TagSort; 3] = [TagSort::Newest, TagSort::Plays, TagSort::Likes];

    fn label(self) -> &'static str {
        match self {
            TagSort::Newest => "Newest",
            TagSort::Plays => "Plays",
            TagSort::Likes => "Likes",
        }
    }

    fn param(self) -> &'static str {
        match self {
            TagSort::Newest => "newest",
            TagSort::Plays => "plays",
            TagSort::Likes => "likes",
        }
    }
}

/// `PagedResponse<Track>` の subset (`page`/`page_size` は使わない)。
#[derive(Clone, Default, Deserialize)]
struct Page {
    #[serde(default)]
    collection: Vec<Track>,
    #[serde(default)]
    has_more: bool,
}

const PAGE_LIMIT: u32 = 30;

#[derive(Default)]
pub struct TagView {
    last_key: String,
    sort: TagSort,
    page: u32,
    tracks: Query<Page>,
}

impl TagView {
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
    ) -> TagAction {
        let _ = (audio, cache);
        let Some(api) = api else {
            ui.label("backend not running");
            return TagAction::None;
        };
        let tag = param.unwrap_or("").trim();
        if tag.is_empty() {
            widgets::empty_note(ui, "No tag selected");
            return TagAction::None;
        }
        let mut action = TagAction::None;

        widgets::section_header(ui, &format!("#{tag}"), None);

        ui.horizontal(|ui| {
            for sort in TagSort::ALL {
                if ui
                    .selectable_label(self.sort == sort, sort.label())
                    .clicked()
                {
                    self.sort = sort;
                    self.page = 0;
                }
            }
        });

        let key = format!("{tag}\u{1}{}\u{1}{}", self.sort as u8, self.page);
        if key != self.last_key {
            self.last_key = key;
            let api_owned = api.clone();
            let url = format!(
                "/tags/{}/tracks?limit={PAGE_LIMIT}&page={}&sort={}",
                urlencoding::encode(tag),
                self.page,
                self.sort.param()
            );
            self.tracks.request(rt, async move {
                let v = api_owned.get_json(&url).await?;
                serde_json::from_value(v).map_err(|e| e.to_string())
            });
        }

        if self.tracks.poll() || self.tracks.loading {
            ui.ctx().request_repaint();
        }

        if self.tracks.loading && self.tracks.data.is_none() {
            widgets::loading(ui);
        } else if let Some(err) = self.tracks.error.as_ref() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Tag unavailable: {err}"),
            );
        } else if let Some(page) = self.tracks.data.as_ref() {
            if page.collection.is_empty() {
                widgets::empty_note(ui, "No tracks found");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for (i, track) in page.collection.iter().enumerate() {
                        let hit = Self::track_card(ui, rt, images, player, track, accent);
                        if hit.play_clicked() {
                            action = TagAction::PlayList(page.collection.clone(), i);
                        } else if hit.title_clicked() {
                            action = TagAction::Navigate(Route::Track, Some(track.urn.clone()));
                        } else if hit.artist_clicked() {
                            if let Some(u) = track.user.as_ref() {
                                action =
                                    TagAction::Navigate(Route::User, Some(u.urn.clone()));
                            }
                        } else if hit.menu_clicked() {
                            action = TagAction::OpenMenu(track.clone());
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.page > 0, egui::Button::new("Prev"))
                    .clicked()
                {
                    self.page -= 1;
                }
                ui.label(format!("Page {}", self.page + 1));
                if ui
                    .add_enabled(page.has_more, egui::Button::new("Next"))
                    .clicked()
                {
                    self.page += 1;
                }
            });
        }

        action
    }

    /// 1カード描画。戻り値は操作結果。
    fn track_card(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        track: &Track,
        accent: egui::Color32,
    ) -> widgets::CardHit {
        let playing = widgets::is_currently_playing(player, track);
        widgets::track_card(ui, rt, images, track, 132.0, playing, accent)
    }
}
