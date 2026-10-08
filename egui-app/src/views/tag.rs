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

pub enum TagAction {
    None,
    PlayTrack(Track),
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
        ui: &mut egui::Ui,
    ) -> TagAction {
        let _ = (audio, cache);
        let Some(api) = api else {
            ui.label("backend not running");
            return TagAction::None;
        };
        let tag = param.unwrap_or("").trim();
        if tag.is_empty() {
            ui.label("No tag selected");
            return TagAction::None;
        }
        let mut action = TagAction::None;

        ui.heading(format!("#{tag}"));

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
            ui.label("Loading...");
        } else if let Some(err) = self.tracks.error.as_ref() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Tag unavailable: {err}"),
            );
        } else if let Some(page) = self.tracks.data.as_ref() {
            if page.collection.is_empty() {
                ui.label("No tracks found");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for track in &page.collection {
                        if Self::track_card(ui, rt, images, player, track) {
                            action = TagAction::PlayTrack(track.clone());
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

    /// 1カード描画 (`views/home.rs` の `track_card` と同形)。戻り値はクリックされたか。
    fn track_card(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        track: &Track,
    ) -> bool {
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
