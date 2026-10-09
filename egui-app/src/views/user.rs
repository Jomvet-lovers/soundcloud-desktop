//! User profile view — egui port of `desktop/src/pages/UserPage.tsx`
//! (+ `desktop/src/components/user/{IdentityHub,TabDock,UserTabs,FollowBtn}.tsx`).
//! Pattern follows `views/home.rs`: `Query` + `ApiClient` + `Images` + `Action`.
//!
//! Tabs are simple switches: Popular / Tracks / Playlists / Likes /
//! Followers / Following. Page-based lists use the shared `Pager`
//! (infinite scroll), likes use Prev/Next over the cursor envelope.

use std::sync::Arc;

use serde::Deserialize;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::{Playlist, ScUser, Track};
use crate::images::Images;
use crate::pager::{ListPage, Pager};
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum UserAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (表示中の一覧がキューになる)。
    PlayList(Vec<Track>, usize),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    Navigate(Route, Option<String>),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum UserTab {
    #[default]
    Popular,
    Tracks,
    Playlists,
    Likes,
    Followers,
    Following,
}

impl UserTab {
    const ALL: &[(UserTab, &str)] = &[
        (UserTab::Popular, "Popular"),
        (UserTab::Tracks, "Tracks"),
        (UserTab::Playlists, "Playlists"),
        (UserTab::Likes, "Likes"),
        (UserTab::Followers, "Followers"),
        (UserTab::Following, "Following"),
    ];
}

/// Full profile (`desktop/src/lib/hooks.ts` `UserProfile`). All fields are
/// tolerant: the backend may omit any of them.
#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct UserProfile {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub urn: String,
    #[serde(default)]
    pub username: String,
    pub avatar_url: Option<String>,
    pub permalink_url: Option<String>,
    pub full_name: Option<String>,
    pub description: Option<String>,
    pub verified: Option<bool>,
    pub plan: Option<String>,
    pub created_at: Option<String>,
    pub city: Option<String>,
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub followers_count: Option<i64>,
    pub followings_count: Option<i64>,
    pub track_count: Option<i64>,
    pub public_favorites_count: Option<i64>,
    pub likes_count: Option<i64>,
    pub playlist_count: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct WebProfile {
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
}

/// Connection row (`followers` / `followings`). `models::ScUser` lacks the
/// count fields, so a local struct keeps them for display.
#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct ConnUser {
    #[serde(default)]
    pub urn: String,
    #[serde(default)]
    pub username: String,
    pub avatar_url: Option<String>,
    pub followers_count: Option<i64>,
}

/// Cursor page for `GET /users/{urn}/likes/tracks` (`limit` + `cursor`).
#[derive(Clone, Debug, Default, Deserialize)]
struct LikesPage {
    #[serde(default)]
    pub collection: Vec<Track>,
    pub next_cursor: Option<String>,
}

#[derive(Default)]
pub struct UserView {
    urn: Option<String>,
    profile: Query<UserProfile>,
    me: Query<ScUser>,
    follow: Query<bool>,
    web: Query<Vec<WebProfile>>,
    tab: UserTab,
    search: String,
    popular: Query<Vec<Track>>,
    tracks: Pager<Track>,
    playlists: Pager<Playlist>,
    likes: Query<LikesPage>,
    likes_cursors: Vec<Option<String>>,
    likes_page: usize,
    likes_started: bool,
    followers: Pager<ConnUser>,
    followings: Pager<ConnUser>,
}

fn fmt_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn fmt_dur_ms(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn track_matches(track: &Track, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    track.display_title().to_lowercase().contains(needle)
        || track.artist_name().to_lowercase().contains(needle)
}

impl UserView {
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
    ) -> UserAction {
        let _ = audio;
        let _ = cache;
        let Some(api) = api else {
            ui.label("backend not running");
            return UserAction::None;
        };
        let Some(urn_param) = param.filter(|s| !s.is_empty()) else {
            ui.label("no user selected");
            return UserAction::None;
        };
        if self.urn.as_deref() != Some(urn_param) {
            *self = Self::default();
            self.urn = Some(urn_param.to_string());
            self.likes_cursors = vec![None];
        }
        let urn = self.urn.clone().unwrap_or_default();
        let enc = urlencoding::encode(&urn).into_owned();

        // ---- requests ----
        if !self.profile.requested() {
            self.request_profile(rt, api, &enc);
        }
        if !self.me.requested() {
            let api = api.clone();
            self.me.request(rt, async move {
                api.get_json("/me/cold")
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
        if !self.web.requested() {
            self.request_web(rt, api, &enc);
        }
        let req_tab = self.tab;
        match req_tab {
            UserTab::Popular => {
                if !self.popular.requested() {
                    self.request_popular(rt, api, &enc);
                }
            }
            UserTab::Tracks => {
                let base = format!("/users/{enc}/tracks");
                self.tracks.ensure(rt, api, &base);
            }
            UserTab::Playlists => {
                let base = format!("/users/{enc}/playlists");
                self.playlists.ensure(rt, api, &base);
            }
            UserTab::Likes => {
                if !self.likes_started {
                    self.request_likes(rt, api, &enc, None);
                }
            }
            UserTab::Followers => {
                let base = format!("/users/{enc}/followers");
                self.followers.ensure(rt, api, &base);
            }
            UserTab::Following => {
                let base = format!("/users/{enc}/followings");
                self.followings.ensure(rt, api, &base);
            }
        }
        // Follow state needs our own urn first
        // (`GET /users/{me}/followings/{target}` → bool, cf. `FollowBtn.tsx`).
        let me_urn = self.me.data.as_ref().map(|u| u.urn.clone());
        if let Some(m) = me_urn.clone() {
            if m != urn && !self.follow.requested() {
                let api = api.clone();
                let url = format!(
                    "/users/{}/followings/{enc}",
                    urlencoding::encode(&m)
                );
                self.follow.request(rt, async move {
                    api.get_json(&url)
                        .await
                        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                });
            }
        }

        // ---- poll ----
        let mut repaint = self.profile.poll();
        repaint |= self.me.poll();
        repaint |= self.follow.poll();
        repaint |= self.web.poll();
        repaint |= self.popular.poll();
        repaint |= self.tracks.poll();
        repaint |= self.playlists.poll();
        repaint |= self.likes.poll();
        repaint |= self.followers.poll();
        repaint |= self.followings.poll();
        if repaint {
            ui.ctx().request_repaint();
        }

        let mut action = UserAction::None;

        if self.profile.loading && self.profile.data.is_none() {
            ui.horizontal(|ui| {
                ui.spinner();
                widgets::loading_text(ui, "Loading user...");
            });
            return action;
        }
        if let Some(err) = self.profile.error.clone() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("User unavailable: {err}"),
            );
            if ui.button("Retry").clicked() {
                self.request_profile(rt, api, &enc);
            }
            return action;
        }
        let Some(user) = self.profile.data.clone() else {
            ui.label("no user selected");
            return action;
        };

        // ---- header (cf. `IdentityHub.tsx`) ----
        ui.horizontal(|ui| {
            let art = user.avatar_url.as_deref();
            images.show(ui, rt, art, 96.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(
                        egui::RichText::new(&user.username)
                            .font(crate::theme::semibold(30.0)),
                    ));
                    if user.verified.unwrap_or(false) {
                        ui.label("✔ Verified");
                    }
                    if let Some(plan) = user.plan.as_deref() {
                        if !plan.is_empty() && plan != "Free" {
                            ui.label(format!("Pro ({plan})"));
                        }
                    }
                });
                if let Some(full) = user.full_name.as_deref() {
                    if !full.is_empty() && full != user.username {
                        ui.label(full);
                    }
                }
                let place: Vec<&str> = [
                    user.city.as_deref().unwrap_or(""),
                    user.country_code
                        .as_deref()
                        .or(user.country.as_deref())
                        .unwrap_or(""),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
                if !place.is_empty() {
                    ui.label(place.join(", "));
                }
                if let Some(desc) = user.description.as_deref() {
                    if !desc.is_empty() {
                        ui.label(desc.chars().take(300).collect::<String>());
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} Followers",
                        user.followers_count.map(fmt_count).unwrap_or_else(|| "—".into())
                    ));
                    ui.label(format!(
                        "{} Following",
                        user.followings_count
                            .map(fmt_count)
                            .unwrap_or_else(|| "—".into())
                    ));
                    ui.label(format!(
                        "{} Tracks",
                        user.track_count.map(fmt_count).unwrap_or_else(|| "—".into())
                    ));
                });
                ui.horizontal(|ui| {
                    let is_own = me_urn.as_deref() == Some(urn.as_str());
                    if !is_own {
                        match self.follow.data {
                            Some(following) => {
                                let label = if following { "Following" } else { "Follow" };
                                if ui.button(label).clicked() {
                                    // cf. `FollowBtn.tsx`: PUT / DELETE `/me/followings/{urn}`.
                                    let api_c = api.clone();
                                    let path = format!("/me/followings/{enc}");
                                    let next = !following;
                                    rt.spawn(async move {
                                        let method = if next { "PUT" } else { "DELETE" };
                                        let _ =
                                            api_c.request_json(method, &path, None).await;
                                    });
                                    self.follow.data = Some(next);
                                }
                            }
                            None => {
                                if self.follow.loading {
                                    ui.spinner();
                                }
                            }
                        }
                    }
                    if let Some(link) = user.permalink_url.clone() {
                        if ui.button("Copy link").clicked() {
                            ui.ctx().copy_text(link.clone());
                        }
                        ui.hyperlink_to("SoundCloud ↗", &link);
                    }
                });
            });
        });
        if let Some(web) = self.web.data.as_ref() {
            if !web.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for w in web.iter().take(12) {
                        let title = if w.title.is_empty() {
                            w.service.clone()
                        } else {
                            w.title.clone()
                        };
                        ui.hyperlink_to(title, &w.url);
                    }
                });
            }
        }

        ui.separator();

        // ---- tabs (cf. `TabDock.tsx`, simplified) ----
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in UserTab::ALL {
                let count: Option<String> = match *tab {
                    UserTab::Tracks => user.track_count.map(fmt_count),
                    UserTab::Followers => user.followers_count.map(fmt_count),
                    UserTab::Following => user.followings_count.map(fmt_count),
                    UserTab::Likes => user
                        .public_favorites_count
                        .or(user.likes_count)
                        .map(fmt_count),
                    UserTab::Playlists => user.playlist_count.map(fmt_count),
                    UserTab::Popular => None,
                };
                let text = match count {
                    Some(c) => format!("{label} · {c}"),
                    None => label.to_string(),
                };
                if widgets::tab_button(ui, &text, self.tab == *tab).clicked() {
                    self.tab = *tab;
                }
            }
        });
        // Inline search over the loaded tracks/playlists (client-side;
        // the React page searches the local DB per scope).
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search in their Tracks...")
                    .desired_width(280.0),
            );
        });

        let needle = self.search.trim().to_lowercase();

        let cur_tab = self.tab;
        match cur_tab {
            UserTab::Popular => {
                if self.popular.loading && self.popular.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(err) = self.popular.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Tracks unavailable: {err}"),
                    );
                    if ui.button("Retry").clicked() {
                        self.request_popular(rt, api, &enc);
                    }
                } else if let Some(tracks) = self.popular.data.as_ref() {
                    let rows: Vec<Track> = tracks
                        .iter()
                        .filter(|t| track_matches(t, &needle))
                        .cloned()
                        .collect();
                    for (i, t) in rows.iter().enumerate() {
                        let hit = Self::track_row(ui, rt, images, player, t, accent);
                        if hit.play_clicked() {
                            action = UserAction::PlayList(rows.clone(), i);
                        } else if hit.title_clicked() {
                            action = UserAction::Navigate(Route::Track, Some(t.urn.clone()));
                        } else if hit.artist_clicked() {
                            if let Some(u) = t.user.as_ref() {
                                action =
                                    UserAction::Navigate(Route::User, Some(u.urn.clone()));
                            }
                        } else if hit.menu_clicked() {
                            action = UserAction::OpenMenu(t.clone());
                        }
                    }
                    if rows.is_empty() {
                        widgets::empty_note(ui, "Nothing here yet");
                    }
                }
            }
            UserTab::Tracks => {
                let rows: Vec<Track> = self
                    .tracks
                    .items
                    .iter()
                    .filter(|t| track_matches(t, &needle))
                    .cloned()
                    .collect();
                for (i, t) in rows.iter().enumerate() {
                    let hit = Self::track_row(ui, rt, images, player, t, accent);
                    if hit.play_clicked() {
                        action = UserAction::PlayList(rows.clone(), i);
                    } else if hit.title_clicked() {
                        action = UserAction::Navigate(Route::Track, Some(t.urn.clone()));
                    } else if hit.artist_clicked() {
                        if let Some(u) = t.user.as_ref() {
                            action = UserAction::Navigate(Route::User, Some(u.urn.clone()));
                        }
                    } else if hit.menu_clicked() {
                        action = UserAction::OpenMenu(t.clone());
                    }
                }
                if self.tracks.q.loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        widgets::loading(ui);
                    });
                } else if rows.is_empty() && !self.tracks.items.is_empty() {
                    widgets::empty_note(ui, "No matches in this user's content");
                } else if rows.is_empty() {
                    widgets::empty_note(ui, "Nothing here yet");
                }
                if let Some(err) = self.tracks.q.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Tracks unavailable: {err}"),
                    );
                }
                if self.tracks.has_more && !self.tracks.q.loading {
                    let base = format!("/users/{enc}/tracks");
                    if ui.button("More").clicked() {
                        self.tracks.fetch(rt, api, &base);
                    }
                }
            }
            UserTab::Playlists => {
                if self.playlists.q.loading && self.playlists.items.is_empty() {
                    widgets::loading(ui);
                } else if self.playlists.items.is_empty() {
                    if let Some(err) = self.playlists.q.error.clone() {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 150, 150),
                            format!("Playlists unavailable: {err}"),
                        );
                    } else {
                        widgets::empty_note(ui, "Nothing here yet");
                    }
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for p in self.playlists.items.iter() {
                            if !needle.is_empty()
                                && !p.title.to_lowercase().contains(&needle)
                            {
                                continue;
                            }
                            if Self::playlist_card(ui, rt, images, p) {
                                action =
                                    UserAction::Navigate(Route::Playlist, Some(p.urn.clone()));
                            }
                        }
                    });
                    if crate::pager::auto_load(
                        ui,
                        self.playlists.q.loading,
                        self.playlists.has_more,
                    ) {
                        let base = format!("/users/{enc}/playlists");
                        self.playlists.fetch(rt, api, &base);
                    }
                }
            }
            UserTab::Likes => {
                if self.likes.loading && self.likes.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(page) = self.likes.data.as_ref() {
                    for (i, t) in page.collection.iter().enumerate() {
                        let hit = Self::track_row(ui, rt, images, player, t, accent);
                        if hit.play_clicked() {
                            action = UserAction::PlayList(page.collection.clone(), i);
                        } else if hit.title_clicked() {
                            action = UserAction::Navigate(Route::Track, Some(t.urn.clone()));
                        } else if hit.artist_clicked() {
                            if let Some(u) = t.user.as_ref() {
                                action =
                                    UserAction::Navigate(Route::User, Some(u.urn.clone()));
                            }
                        } else if hit.menu_clicked() {
                            action = UserAction::OpenMenu(t.clone());
                        }
                    }
                    let empty = page.collection.is_empty();
                    let next_cursor = page.next_cursor.clone();
                    if empty {
                        widgets::empty_note(ui, "Nothing here yet");
                    }
                    ui.horizontal(|ui| {
                        if self.likes_page > 0 && ui.button("← Prev").clicked() {
                            self.likes_page -= 1;
                            let cur = self.likes_cursors[self.likes_page].clone();
                            self.request_likes(rt, api, &enc, cur.as_deref());
                        }
                        ui.label(format!("Page {}", self.likes_page + 1));
                        // NB: the backend may report a cursor on the last page;
                        // an empty result then simply shows "Nothing here yet".
                        if next_cursor.is_some() && ui.button("Next →").clicked() {
                            let nxt = next_cursor.clone().unwrap_or_default();
                            self.likes_cursors.truncate(self.likes_page + 1);
                            self.likes_cursors.push(Some(nxt.clone()));
                            self.likes_page += 1;
                            self.request_likes(rt, api, &enc, Some(&nxt));
                        }
                        if self.likes.loading {
                            ui.spinner();
                        }
                    });
                } else if let Some(err) = self.likes.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Likes unavailable: {err}"),
                    );
                    if ui.button("Retry").clicked() {
                        self.request_likes(rt, api, &enc, None);
                    }
                }
            }
            UserTab::Followers => {
                Self::connections(
                    ui,
                    rt,
                    images,
                    api,
                    &mut self.followers,
                    &enc,
                    "followers",
                    "No followers found.",
                    &mut action,
                );
            }
            UserTab::Following => {
                Self::connections(
                    ui,
                    rt,
                    images,
                    api,
                    &mut self.followings,
                    &enc,
                    "followings",
                    "No followings found.",
                    &mut action,
                );
            }
        }

        action
    }

    #[allow(clippy::too_many_arguments)]
    fn connections(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        api: &ApiClient,
        pager: &mut Pager<ConnUser>,
        enc: &str,
        which: &str,
        empty_text: &str,
        action: &mut UserAction,
    ) {
        if pager.q.loading && pager.items.is_empty() {
            widgets::loading(ui);
            return;
        }
        if pager.items.is_empty() {
            if let Some(err) = pager.q.error.clone() {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 150, 150),
                    format!("Unavailable: {err}"),
                );
            } else {
                ui.label(empty_text);
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for u in pager.items.iter() {
                ui.vertical(|ui| {
                    ui.set_max_width(120.0);
                    if images
                        .show(ui, rt, u.avatar_url.as_deref(), 64.0)
                        .clicked()
                    {
                        *action = UserAction::Navigate(Route::User, Some(u.urn.clone()));
                    }
                    if ui.button(&u.username).clicked() {
                        *action = UserAction::Navigate(Route::User, Some(u.urn.clone()));
                    }
                    if let Some(c) = u.followers_count {
                        ui.label(format!("{} followers", fmt_count(c)));
                    }
                });
            }
        });
        if crate::pager::auto_load(ui, pager.q.loading, pager.has_more) {
            let base = format!("/users/{enc}/{which}");
            pager.fetch(rt, api, &base);
        }
    }

    fn request_profile(&mut self, rt: &tokio::runtime::Handle, api: &ApiClient, enc: &str) {
        let api = api.clone();
        let url = format!("/users/{enc}");
        self.profile.request(rt, async move {
            api.get_json(&url)
                .await
                .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
        });
    }

    fn request_web(&mut self, rt: &tokio::runtime::Handle, api: &ApiClient, enc: &str) {
        let api = api.clone();
        let url = format!("/users/{enc}/web-profiles");
        self.web.request(rt, async move {
            api.get_json(&url)
                .await
                .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
        });
    }

    fn request_popular(&mut self, rt: &tokio::runtime::Handle, api: &ApiClient, enc: &str) {
        let api = api.clone();
        let enc = enc.to_string();
        self.popular.request(rt, async move {
            // cf. `useUserPopularTracks`: walk pages, then sort by plays.
            // Capped at 5×100 to bound the fetch.
            let mut all: Vec<Track> = Vec::new();
            for page in 0..5 {
                let v = api
                    .get_json(&format!("/users/{enc}/tracks?limit=100&page={page}"))
                    .await?;
                let chunk: ListPage<Track> =
                    serde_json::from_value(v).map_err(|e| e.to_string())?;
                let more = chunk.has_more;
                all.extend(chunk.collection);
                if !more {
                    break;
                }
            }
            all.sort_by(|a, b| {
                b.playback_count
                    .unwrap_or(0)
                    .cmp(&a.playback_count.unwrap_or(0))
            });
            Ok(all)
        });
    }

    fn request_likes(
        &mut self,
        rt: &tokio::runtime::Handle,
        api: &ApiClient,
        enc: &str,
        cursor: Option<&str>,
    ) {
        let api = api.clone();
        let enc = enc.to_string();
        let cur = cursor.map(str::to_string);
        self.likes_started = true;
        self.likes.request(rt, async move {
            let mut url = format!("/users/{enc}/likes/tracks?limit=30");
            if let Some(c) = cur {
                url.push_str(&format!("&cursor={}", urlencoding::encode(&c)));
            }
            let v = api.get_json(&url).await?;
            serde_json::from_value(v).map_err(|e| e.to_string())
        });
    }

    /// One track row. Returns true when playback was requested.
    /// (`plays` 数は右端の duration 表示に畳んで温存する。)
    fn track_row(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        track: &Track,
        accent: egui::Color32,
    ) -> widgets::RowParts {
        let playing = widgets::is_currently_playing(player, track);
        let meta = format!(
            "{} plays · {}",
            track.playback_count.unwrap_or(0),
            fmt_dur_ms(track.duration),
        );
        widgets::track_row(
            ui,
            rt,
            images,
            track,
            playing,
            accent,
            Some(&meta),
        )
    }

    /// One playlist card. Returns true when navigation was requested.
    fn playlist_card(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        playlist: &Playlist,
    ) -> bool {
        let mut open = false;
        ui.vertical(|ui| {
            ui.set_max_width(140.0);
            let art = playlist.artwork("t300x300");
            if images.show(ui, rt, art.as_deref(), 132.0).clicked() {
                open = true;
            }
            if ui
                .add(
                    egui::Label::new(&playlist.title)
                        .truncate()
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                open = true;
            }
            ui.label(format!("{} tracks", playlist.track_list().len()));
        });
        open
    }
}
