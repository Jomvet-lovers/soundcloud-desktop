//! Phase 3: Search — 検索ボックス + 種別タブ + 結果 + ジャンルウォール。
//! 対応: `desktop/src/pages/Search.tsx` (+ `GenreGrid`, `DiscoverSections` は対象外)。
//! 空クエリではジャンルウォール (タグ送り) を表示する。
//! 全タブの 0 ページ目を先読みし、タブ名に件数 (現ページ件数) を表示する。
//! Vibe/Lyrics 検索 (direct backend では空応答)・Discover ミックス再生は見送り。

use std::sync::Arc;

use serde::Deserialize;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Playlist, ScUser, Track};
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum SearchAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (検索結果ページ全体がキューになる)。
    PlayList(Vec<Track>, usize),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    Navigate(Route, Option<String>),
}

/// 検索履歴を更新する (最大 10 件、新しい順、永続化)。
fn record_search(settings: &mut crate::state::SettingsState, query: &str) {
    settings.search_history.retain(|q| q != query);
    settings.search_history.insert(0, query.to_string());
    settings.search_history.truncate(10);
    let _ = crate::backend::prefs::save(settings);
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SearchTab {
    #[default]
    Tracks,
    Users,
    Playlists,
    Albums,
}

impl SearchTab {
    const ALL: [SearchTab; 4] = [
        SearchTab::Tracks,
        SearchTab::Users,
        SearchTab::Playlists,
        SearchTab::Albums,
    ];

    fn label(self) -> &'static str {
        match self {
            SearchTab::Tracks => "Tracks",
            SearchTab::Users => "Users",
            SearchTab::Playlists => "Playlists",
            SearchTab::Albums => "Albums",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TrackSort {
    #[default]
    Relevance,
    Plays,
    Newest,
    Likes,
}

impl TrackSort {
    const ALL: [TrackSort; 4] = [
        TrackSort::Relevance,
        TrackSort::Plays,
        TrackSort::Newest,
        TrackSort::Likes,
    ];

    fn label(self) -> &'static str {
        match self {
            TrackSort::Relevance => "Relevance",
            TrackSort::Plays => "Plays",
            TrackSort::Newest => "Newest",
            TrackSort::Likes => "Likes",
        }
    }

    /// `searchDbExtra()` の `sort` パラメータ。relevance は省略。
    fn param(self) -> Option<&'static str> {
        match self {
            TrackSort::Relevance => None,
            TrackSort::Plays => Some("plays"),
            TrackSort::Newest => Some("newest"),
            TrackSort::Likes => Some("likes"),
        }
    }
}

/// `PagedResponse<T>` (`desktop/src/lib/hooks.ts`) の subset。
#[derive(Clone, Default, Deserialize)]
struct Page<T> {
    #[serde(default)]
    collection: Vec<T>,
    #[serde(default)]
    has_more: bool,
}

/// egui 側 `models.rs` に `CatalogAlbum` が無いためローカル定義。
/// 対応: `hooks.ts` の `CatalogAlbum` (`id`/`title`/`cover_url`)。
#[derive(Clone, Debug, Default, Deserialize)]
struct AlbumHit {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    cover_url: Option<String>,
}

/// `desktop/src/components/search/utils.ts` の `GENRES`。
const GENRES: [(&str, &str); 12] = [
    ("Lo-fi", "lofi"),
    ("House", "house"),
    ("Phonk", "phonk"),
    ("Ambient", "ambient"),
    ("R&B", "rnb"),
    ("Trap", "trap"),
    ("Jazz", "jazz"),
    ("Techno", "techno"),
    ("Indie", "indie"),
    ("Soul", "soul"),
    ("DnB", "dnb"),
    ("Hyperpop", "hyperpop"),
];

const PAGE_LIMIT: u32 = 30;

#[derive(Default)]
pub struct SearchView {
    input: String,
    last_key: String,
    /// グローバル検索から渡された最後のクエリ。
    last_param: Option<String>,
    /// query/tab/sort (ページを除く) の前回値。変化でページを先頭へ戻す。
    last_base_key: String,
    /// 非アクティブタブ先読み (件数表示) のキー (query + sort)。
    prefetch_key: String,
    tab: SearchTab,
    sort: TrackSort,
    page: u32,
    tracks: Query<Page<Track>>,
    users: Query<Page<ScUser>>,
    playlists: Query<Page<Playlist>>,
    albums: Query<Page<AlbumHit>>,
}

/// タブ名の件数表示用 (現ページの件数。React の `tabs` memo と同じ)。
fn page_len<T>(q: &Query<Page<T>>) -> Option<usize> {
    q.data.as_ref().map(|p| p.collection.len())
}

fn search_url(kind: &str, q: &str, page: u32, sort: Option<&str>) -> String {
    let mut url = format!(
        "/search/db/{kind}?limit={PAGE_LIMIT}&page={page}&q={}",
        urlencoding::encode(q)
    );
    if let Some(s) = sort {
        url.push_str("&sort=");
        url.push_str(s);
    }
    url
}

impl SearchView {
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
        settings: &mut crate::state::SettingsState,
        ui: &mut egui::Ui,
    ) -> SearchAction {
        let _ = (audio, cache);
        // グローバル検索 (タイトルバー) から param で渡されたクエリを反映する。
        if let Some(q) = param {
            if self.last_param.as_deref() != Some(q) {
                self.input = q.to_string();
                self.last_param = Some(q.to_string());
                self.page = 0;
            }
        }
        let Some(api) = api else {
            ui.label("backend not running");
            return SearchAction::None;
        };
        let mut action = SearchAction::None;

        let focus_now = ui
            .ctx()
            .memory_mut(|m| {
                m.data
                    .remove_temp::<bool>(egui::Id::new(crate::shell::FOCUS_SEARCH_ID))
            })
            .unwrap_or(false);
        ui.horizontal(|ui| {
            ui.label("Search");
            let resp = ui.text_edit_singleline(&mut self.input);
            if focus_now {
                resp.request_focus();
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                // Enter 確定: 下の差分検出で再取得される。
            }
            if ui.button("Clear").clicked() {
                self.input.clear();
            }
        });

        let query = self.input.trim().to_string();

        if query.is_empty() {
            if !settings.search_history.is_empty() {
                widgets::section_header(ui, "Recent searches", None);
                ui.horizontal_wrapped(|ui| {
                    let items: Vec<String> = settings.search_history.clone();
                    for q in items {
                        if ui.selectable_label(false, &q).clicked() {
                            self.input = q;
                        }
                    }
                });
                ui.separator();
            }
            // ジャンルウォール (React: 空クエリ → `<GenreGrid/>`)。
            widgets::section_header(ui, "Browse all genres", None);
            egui::Grid::new("genre_wall")
                .num_columns(4)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    for (i, (label, _key)) in GENRES.iter().enumerate() {
                        if ui.button(*label).clicked() {
                            // React: `/tag/{name}` (ラベルそのまま)。
                            action = SearchAction::Navigate(
                                Route::Tag,
                                Some(label.to_string()),
                            );
                        }
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
            return action;
        }

        // タブ切替。
        ui.horizontal(|ui| {
            for tab in SearchTab::ALL {
                let selected = self.tab == tab;
                let count = match tab {
                    SearchTab::Tracks => page_len(&self.tracks),
                    SearchTab::Users => page_len(&self.users),
                    SearchTab::Playlists => page_len(&self.playlists),
                    SearchTab::Albums => page_len(&self.albums),
                };
                let label = match count {
                    Some(n) => format!("{} ({n})", tab.label()),
                    None => tab.label().to_string(),
                };
                if ui.selectable_label(selected, label).clicked() {
                    self.tab = tab;
                    self.page = 0;
                }
            }
        });

        // Tracks タブのみソート切替 (React と同じ)。
        if self.tab == SearchTab::Tracks {
            ui.horizontal(|ui| {
                for sort in TrackSort::ALL {
                    if ui
                        .selectable_label(self.sort == sort, sort.label())
                        .clicked()
                    {
                        self.sort = sort;
                        self.page = 0;
                    }
                }
            });
        }

        // query/tab/sort が変わったらページを先頭へ (React の pagerKey と同じ)。
        let base_key = format!(
            "{query}\u{1}{}\u{1}{:?}",
            self.tab.label(),
            self.sort as u8
        );
        if base_key != self.last_base_key {
            self.last_base_key = base_key.clone();
            self.page = 0;
        }
        // 非アクティブタブの 0 ページ目を先読み (件数表示用。React と同じ)。
        if base_key != self.prefetch_key {
            self.prefetch_key = base_key.clone();
            if self.tab != SearchTab::Tracks {
                let api = api.clone();
                let url = search_url("tracks", &query, 0, self.sort.param());
                self.tracks.request(rt, async move {
                    let v = api.get_json(&url).await?;
                    serde_json::from_value(v).map_err(|e| e.to_string())
                });
            }
            if self.tab != SearchTab::Users {
                let api = api.clone();
                let url = search_url("users", &query, 0, None);
                self.users.request(rt, async move {
                    let v = api.get_json(&url).await?;
                    serde_json::from_value(v).map_err(|e| e.to_string())
                });
            }
            if self.tab != SearchTab::Playlists {
                let api = api.clone();
                let url = search_url("playlists", &query, 0, None);
                self.playlists.request(rt, async move {
                    let v = api.get_json(&url).await?;
                    serde_json::from_value(v).map_err(|e| e.to_string())
                });
            }
            if self.tab != SearchTab::Albums {
                let api = api.clone();
                let url = search_url("albums", &query, 0, None);
                self.albums.request(rt, async move {
                    let v = api.get_json(&url).await?;
                    serde_json::from_value(v).map_err(|e| e.to_string())
                });
            }
        }
        let key = format!("{base_key}\u{1}{}", self.page);
        if key != self.last_key {
            self.last_key = key;
            if self.page == 0 {
                record_search(settings, &query);
            }
            let api_owned = api.clone();
            match self.tab {
                SearchTab::Tracks => {
                    let url = search_url("tracks", &query, self.page, self.sort.param());
                    self.tracks.request(rt, async move {
                        let v = api_owned.get_json(&url).await?;
                        serde_json::from_value(v).map_err(|e| e.to_string())
                    });
                }
                SearchTab::Users => {
                    let url = search_url("users", &query, self.page, None);
                    self.users.request(rt, async move {
                        let v = api_owned.get_json(&url).await?;
                        serde_json::from_value(v).map_err(|e| e.to_string())
                    });
                }
                SearchTab::Playlists => {
                    let url = search_url("playlists", &query, self.page, None);
                    self.playlists.request(rt, async move {
                        let v = api_owned.get_json(&url).await?;
                        serde_json::from_value(v).map_err(|e| e.to_string())
                    });
                }
                SearchTab::Albums => {
                    let url = search_url("albums", &query, self.page, None);
                    self.albums.request(rt, async move {
                        let v = api_owned.get_json(&url).await?;
                        serde_json::from_value(v).map_err(|e| e.to_string())
                    });
                }
            }
        }

        let mut changed = self.tracks.poll();
        changed |= self.users.poll();
        changed |= self.playlists.poll();
        changed |= self.albums.poll();
        let loading = match self.tab {
            SearchTab::Tracks => self.tracks.loading,
            SearchTab::Users => self.users.loading,
            SearchTab::Playlists => self.playlists.loading,
            SearchTab::Albums => self.albums.loading,
        };
        if changed || loading {
            ui.ctx().request_repaint();
        }

        match self.tab {
            SearchTab::Tracks => {
                if self.tracks.loading && self.tracks.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(err) = self.tracks.error.as_ref() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Search unavailable: {err}"),
                    );
                } else if let Some(page) = self.tracks.data.as_ref() {
                    if page.collection.is_empty() {
                        widgets::empty_note(ui, "No results found");
                    } else {
                        for (i, track) in page.collection.iter().enumerate() {
                            match Self::track_row(ui, rt, images, player, track, accent) {
                                widgets::RowHit::Clicked => {
                                    action = SearchAction::PlayList(page.collection.clone(), i);
                                }
                                widgets::RowHit::Menu => {
                                    action = SearchAction::OpenMenu(track.clone());
                                }
                                widgets::RowHit::None => {}
                            }
                        }
                    }
                    Self::pager(ui, self.page, page.has_more, &mut self.page);
                }
            }
            SearchTab::Users => {
                if self.users.loading && self.users.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(err) = self.users.error.as_ref() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Search unavailable: {err}"),
                    );
                } else if let Some(page) = self.users.data.as_ref() {
                    if page.collection.is_empty() {
                        widgets::empty_note(ui, "No results found");
                    } else {
                        for user in &page.collection {
                            ui.horizontal(|ui| {
                                let art = user
                                    .avatar_url
                                    .as_deref()
                                    .map(|u| u.replace("-large", "-t120x120"));
                                images.show(ui, rt, art.as_deref(), 36.0);
                                let name = if user.username.is_empty() {
                                    "(unknown)"
                                } else {
                                    &user.username
                                };
                                if ui.button(name).clicked() && !user.urn.is_empty() {
                                    action = SearchAction::Navigate(
                                        Route::User,
                                        Some(user.urn.clone()),
                                    );
                                }
                            });
                        }
                    }
                    Self::pager(ui, self.page, page.has_more, &mut self.page);
                }
            }
            SearchTab::Playlists => {
                if self.playlists.loading && self.playlists.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(err) = self.playlists.error.as_ref() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Search unavailable: {err}"),
                    );
                } else if let Some(page) = self.playlists.data.as_ref() {
                    if page.collection.is_empty() {
                        widgets::empty_note(ui, "No results found");
                    } else {
                        for playlist in &page.collection {
                            ui.horizontal(|ui| {
                                images.show(
                                    ui,
                                    rt,
                                    playlist.artwork("t120x120").as_deref(),
                                    36.0,
                                );
                                let title = if playlist.title.is_empty() {
                                    "(untitled)"
                                } else {
                                    &playlist.title
                                };
                                if ui.button(title).clicked() && !playlist.urn.is_empty() {
                                    action = SearchAction::Navigate(
                                        Route::Playlist,
                                        Some(playlist.urn.clone()),
                                    );
                                }
                            });
                        }
                    }
                    Self::pager(ui, self.page, page.has_more, &mut self.page);
                }
            }
            SearchTab::Albums => {
                if self.albums.loading && self.albums.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(err) = self.albums.error.as_ref() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Search unavailable: {err}"),
                    );
                } else if let Some(page) = self.albums.data.as_ref() {
                    if page.collection.is_empty() {
                        widgets::empty_note(ui, "No results found");
                    } else {
                        for album in &page.collection {
                            ui.horizontal(|ui| {
                                let art = album
                                    .cover_url
                                    .as_deref()
                                    .map(|u| u.replace("-large", "-t120x120"));
                                images.show(ui, rt, art.as_deref(), 36.0);
                                let title = if album.title.is_empty() {
                                    "(untitled)"
                                } else {
                                    &album.title
                                };
                                if ui.button(title).clicked() && !album.id.is_empty() {
                                    action = SearchAction::Navigate(
                                        Route::Album,
                                        Some(album.id.clone()),
                                    );
                                }
                            });
                        }
                    }
                    Self::pager(ui, self.page, page.has_more, &mut self.page);
                }
            }
        }

        action
    }

    /// Track 行。戻り値はクリックされたか (アートのクリック応答)。
    fn track_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        track: &Track,
        accent: egui::Color32,
    ) -> widgets::RowHit {
        let playing = widgets::is_currently_playing(player, track);
        let s = (track.duration.max(0) / 1000) as u64;
        let dur = format!("{}:{:02}", s / 60, s % 60);
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

    /// 番号なし Prev/Next ページャ (React の `<Pager/>` の簡略版)。
    fn pager(ui: &mut egui::Ui, page: u32, has_more: bool, out_page: &mut u32) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(page > 0, egui::Button::new("Prev"))
                .clicked()
            {
                *out_page = page.saturating_sub(1);
            }
            ui.label(format!("Page {}", page + 1));
            if ui.add_enabled(has_more, egui::Button::new("Next")).clicked() {
                *out_page = page + 1;
            }
        });
    }
}
