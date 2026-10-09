//! Artist page view — egui port of `desktop/src/pages/ArtistPage.tsx`
//! (+ `desktop/src/components/artist/`).
//! Pattern follows `views/home.rs`: `Query` + `ApiClient` + `Images` + `Action`.
//!
//! Tabs are simple switches: Tracks / Appears on / Albums / Related / About.
//! Tracks support a Popular/Recent sort toggle (refetch on change).

use std::sync::Arc;

use serde::Deserialize;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::Track;
use crate::images::Images;
use crate::query::Query;
use crate::state::{PlayerState, Route};
use crate::widgets;

pub enum ArtistAction {
    None,
    PlayTrack(Track),
    /// リスト文脈の再生 (表示中の一覧がキューになる)。
    PlayList(Vec<Track>, usize),
    /// 右クリックメニューを開く。
    OpenMenu(Track),
    Navigate(Route, Option<String>),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ArtistTab {
    #[default]
    Tracks,
    Appears,
    Albums,
    Related,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TracksSort {
    #[default]
    Popular,
    Recent,
}

impl TracksSort {
    fn as_str(self) -> &'static str {
        match self {
            TracksSort::Popular => "popular",
            TracksSort::Recent => "recent",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct ArtistSocial {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct ArtistScAccount {
    #[serde(default)]
    pub sc_user_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct RelatedArtist {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub country: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub weight: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ArtistAlbum {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    pub release_year: Option<i32>,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub role: String,
}

impl ArtistAlbum {
    fn kind_label(&self) -> String {
        let k = self.kind.as_deref().unwrap_or("album").to_lowercase();
        match k.as_str() {
            "album" => "Album".to_string(),
            "ep" => "EP".to_string(),
            "single" => "Single".to_string(),
            "compilation" => "Compilation".to_string(),
            other => other.to_string(),
        }
    }
}

/// `GET /artists/{id}` (cf. `components/artist/types.ts` `ArtistDetail`).
#[derive(Clone, Debug, Default, Deserialize)]
#[allow(dead_code)]
struct ArtistDetail {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub country: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub socials: Vec<ArtistSocial>,
    #[serde(default)]
    pub sc_accounts: Vec<ArtistScAccount>,
    #[serde(default)]
    pub track_count: i64,
    #[serde(default)]
    pub track_count_primary: i64,
    #[serde(default)]
    pub track_count_featured: i64,
    #[serde(default)]
    pub album_count: i64,
    #[serde(default)]
    pub popular_tracks: Vec<Track>,
    #[serde(default)]
    pub related_artists: Vec<RelatedArtist>,
}

/// `GET /artists/{id}/tracks?...` → `{ collection }`.
#[derive(Clone, Debug, Default, Deserialize)]
struct TrackCollection {
    #[serde(default)]
    pub collection: Vec<Track>,
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

fn social_label(kind: &str) -> String {
    match kind.to_lowercase().as_str() {
        "soundcloud" => "SoundCloud".to_string(),
        "spotify" => "Spotify".to_string(),
        "youtube" => "YouTube".to_string(),
        "instagram" => "Instagram".to_string(),
        "twitter" | "x" => "X".to_string(),
        "tiktok" => "TikTok".to_string(),
        "facebook" => "Facebook".to_string(),
        "bandcamp" => "Bandcamp".to_string(),
        other if other.is_empty() => "Link".to_string(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => "Link".to_string(),
            }
        }
    }
}

#[derive(Default)]
pub struct ArtistView {
    id: Option<String>,
    detail: Query<ArtistDetail>,
    related: Query<Vec<RelatedArtist>>,
    primary: Query<Vec<Track>>,
    primary_sort: TracksSort,
    primary_for: Option<TracksSort>,
    featured: Query<Vec<Track>>,
    featured_sort: TracksSort,
    featured_for: Option<TracksSort>,
    albums: Query<Vec<ArtistAlbum>>,
    tab: ArtistTab,
    bio_expanded: bool,
}

impl ArtistView {
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
    ) -> ArtistAction {
        let _ = audio;
        let _ = cache;
        let Some(api) = api else {
            ui.label("backend not running");
            return ArtistAction::None;
        };
        let Some(id_param) = param.filter(|s| !s.is_empty()) else {
            ui.label("no artist selected");
            return ArtistAction::None;
        };
        if self.id.as_deref() != Some(id_param) {
            *self = Self::default();
            self.id = Some(id_param.to_string());
        }
        let id = self.id.clone().unwrap_or_default();
        let enc = urlencoding::encode(&id).into_owned();

        // ---- requests ----
        if !self.detail.requested() {
            self.request_detail(rt, api, &enc);
        }
        // Related is also embedded in the detail payload; a dedicated fetch
        // refreshes it (cf. `useArtistRelated`).
        if !self.related.requested() {
            let api = api.clone();
            let url = format!("/artists/{enc}/related");
            self.related.request(rt, async move {
                let v = api.get_json(&url).await?;
                let arr: Vec<RelatedArtist> = v
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|x| serde_json::from_value(x).ok())
                    .collect();
                Ok(arr)
            });
        }
        if self.primary_for != Some(self.primary_sort) {
            let api = api.clone();
            let url = format!(
                "/artists/{enc}/tracks?role=primary&sort={}&limit=80",
                self.primary_sort.as_str()
            );
            let sort = self.primary_sort;
            self.primary.request(rt, async move {
                let v = api.get_json(&url).await?;
                let list: TrackCollection =
                    serde_json::from_value(v).map_err(|e| e.to_string())?;
                Ok(list.collection)
            });
            self.primary_for = Some(sort);
        }
        if self.featured_for != Some(self.featured_sort) {
            let api = api.clone();
            let url = format!(
                "/artists/{enc}/tracks?role=featured&sort={}&limit=80",
                self.featured_sort.as_str()
            );
            let sort = self.featured_sort;
            self.featured.request(rt, async move {
                let v = api.get_json(&url).await?;
                let list: TrackCollection =
                    serde_json::from_value(v).map_err(|e| e.to_string())?;
                Ok(list.collection)
            });
            self.featured_for = Some(sort);
        }
        if !self.albums.requested() {
            let api = api.clone();
            let url = format!("/artists/{enc}/albums");
            self.albums.request(rt, async move {
                let v = api.get_json(&url).await?;
                let arr: Vec<ArtistAlbum> = v
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|x| serde_json::from_value(x).ok())
                    .collect();
                Ok(arr)
            });
        }

        // ---- poll ----
        let mut repaint = self.detail.poll();
        repaint |= self.related.poll();
        repaint |= self.primary.poll();
        repaint |= self.featured.poll();
        repaint |= self.albums.poll();
        if repaint {
            ui.ctx().request_repaint();
        }

        let mut action = ArtistAction::None;

        if self.detail.loading && self.detail.data.is_none() {
            ui.horizontal(|ui| {
                ui.spinner();
                widgets::loading_text(ui, "Loading artist...");
            });
            return action;
        }
        if let Some(err) = self.detail.error.clone() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Artist unavailable: {err}"),
            );
            if ui.button("Retry").clicked() {
                self.request_detail(rt, api, &enc);
            }
            return action;
        }
        let Some(artist) = self.detail.data.clone() else {
            ui.label("no artist selected");
            return action;
        };
        // Related: dedicated fetch wins, detail payload is the fallback
        // (cf. `ArtistPage`: `relatedQuery.data ?? artist.related_artists`).
        let related: Vec<RelatedArtist> = self
            .related
            .data
            .clone()
            .unwrap_or_else(|| artist.related_artists.clone());

        // ---- hero (cf. `ArtistHero.tsx`) ----
        ui.horizontal(|ui| {
            images.show(ui, rt, artist.avatar_url.as_deref(), 96.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if artist.confidence >= 0.7 {
                        ui.label(format!(
                            "✔ Verified artist ({:.0}%)",
                            artist.confidence * 100.0
                        ));
                    }
                    if let Some(country) = artist.country.as_deref() {
                        ui.label(country);
                    }
                    ui.label("Artist");
                });
                ui.heading(&artist.name);
                if let Some(bio) = artist.bio.as_deref() {
                    if !bio.is_empty() {
                        let text = if self.bio_expanded {
                            bio.to_string()
                        } else {
                            bio.chars().take(220).collect::<String>()
                        };
                        ui.label(text);
                        if bio.chars().count() > 220
                            && ui
                                .button(if self.bio_expanded {
                                    "Collapse"
                                } else {
                                    "Read more"
                                })
                                .clicked()
                        {
                            self.bio_expanded = !self.bio_expanded;
                        }
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    for acc in &artist.sc_accounts {
                        let role = match acc.role.as_str() {
                            "main" => "Main",
                            "demo" => "Demo",
                            r => r,
                        };
                        let label = if acc.verified {
                            format!("SoundCloud {role} ✔")
                        } else {
                            format!("SoundCloud {role}")
                        };
                        if ui.button(label).clicked() {
                            // cf. `ArtistHero.tsx` `ScAccountChip`.
                            action = ArtistAction::Navigate(
                                Route::User,
                                Some(format!("soundcloud:users:{}", acc.sc_user_id)),
                            );
                        }
                    }
                    for s in &artist.socials {
                        ui.hyperlink_to(social_label(&s.kind), &s.url);
                    }
                });
            });
        });
        ui.horizontal(|ui| {
            ui.label(format!("{} Tracks", fmt_count(artist.track_count_primary)));
            ui.label(format!(
                "{} Featured",
                fmt_count(artist.track_count_featured)
            ));
            ui.label(format!("{} Albums", fmt_count(artist.album_count)));
            ui.label(format!("{} Related", related.len()));
        });

        ui.separator();

        // ---- tabs (cf. `TabDock`, simplified) ----
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.tab,
                ArtistTab::Tracks,
                format!("Tracks · {}", fmt_count(artist.track_count_primary)),
            );
            ui.selectable_value(
                &mut self.tab,
                ArtistTab::Appears,
                format!("Appears on · {}", fmt_count(artist.track_count_featured)),
            );
            ui.selectable_value(
                &mut self.tab,
                ArtistTab::Albums,
                format!("Albums · {}", fmt_count(artist.album_count)),
            );
            ui.selectable_value(
                &mut self.tab,
                ArtistTab::Related,
                format!("Related · {}", related.len()),
            );
            ui.selectable_value(&mut self.tab, ArtistTab::About, "About");
        });

        let cur_tab = self.tab;
        match cur_tab {
            ArtistTab::Tracks => {
                Self::tracks_section(
                    ui,
                    rt,
                    images,
                    player,
                    &self.primary,
                    self.primary.loading,
                    &mut self.primary_sort,
                    "No tracks",
                    accent,
                    &mut action,
                );
            }
            ArtistTab::Appears => {
                Self::tracks_section(
                    ui,
                    rt,
                    images,
                    player,
                    &self.featured,
                    self.featured.loading,
                    &mut self.featured_sort,
                    "No appearances",
                    accent,
                    &mut action,
                );
            }
            ArtistTab::Albums => {
                if self.albums.loading && self.albums.data.is_none() {
                    widgets::loading(ui);
                } else if let Some(err) = self.albums.error.clone() {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 150, 150),
                        format!("Albums unavailable: {err}"),
                    );
                } else {
                    let mut albums = self.albums.data.clone().unwrap_or_default();
                    albums.sort_by(|a, b| {
                        b.release_year
                            .unwrap_or(0)
                            .cmp(&a.release_year.unwrap_or(0))
                    });
                    if albums.is_empty() {
                        widgets::empty_note(ui, "No albums yet");
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            for al in &albums {
                                ui.vertical(|ui| {
                                    ui.set_max_width(140.0);
                                    if images
                                        .show(ui, rt, al.cover_url.as_deref(), 132.0)
                                        .clicked()
                                    {
                                        action = ArtistAction::Navigate(
                                            Route::Album,
                                            Some(al.id.clone()),
                                        );
                                    }
                                    if ui.button(&al.title).clicked() {
                                        action = ArtistAction::Navigate(
                                            Route::Album,
                                            Some(al.id.clone()),
                                        );
                                    }
                                    let mut meta = al.kind_label();
                                    if al.role != "primary" {
                                        meta.push_str(" · Featured");
                                    }
                                    if let Some(y) = al.release_year {
                                        meta.push_str(&format!(" · {y}"));
                                    }
                                    ui.label(meta);
                                });
                            }
                        });
                    }
                }
            }
            ArtistTab::Related => {
                if related.is_empty() {
                    widgets::empty_note(ui, "No related artists yet");
                } else {
                    let max = related
                        .iter()
                        .map(|r| r.weight)
                        .fold(1.0f64, f64::max)
                        .max(0.01);
                    ui.horizontal_wrapped(|ui| {
                        for r in &related {
                            ui.vertical(|ui| {
                                ui.set_max_width(120.0);
                                if images
                                    .show(ui, rt, r.avatar_url.as_deref(), 64.0)
                                    .clicked()
                                {
                                    action = ArtistAction::Navigate(
                                        Route::Artist,
                                        Some(r.id.clone()),
                                    );
                                }
                                if ui.button(&r.name).clicked() {
                                    action = ArtistAction::Navigate(
                                        Route::Artist,
                                        Some(r.id.clone()),
                                    );
                                }
                                if let Some(c) = r.country.as_deref() {
                                    ui.label(c);
                                }
                                let pct =
                                    (r.weight / max).clamp(0.08, 1.0) * 100.0;
                                ui.label(format!("Affinity {pct:.0}%"));
                            });
                        }
                    });
                }
            }
            ArtistTab::About => {
                widgets::section_header(ui, "About", None);
                match artist.bio.as_deref() {
                    Some(bio) if !bio.is_empty() => {
                        ui.label(bio);
                    }
                    _ => {
                        widgets::empty_note(ui, "No bio yet");
                    }
                }
                ui.horizontal(|ui| {
                    if let Some(country) = artist.country.as_deref() {
                        ui.label(format!("Country: {country}"));
                    }
                    ui.label(format!(
                        "Confidence: {:.0}%",
                        artist.confidence * 100.0
                    ));
                });
                if !artist.sc_accounts.is_empty() {
                    widgets::section_header(ui, "SoundCloud accounts", None);
                    for acc in &artist.sc_accounts {
                        let role = match acc.role.as_str() {
                            "main" => "Main",
                            "demo" => "Demo",
                            r => r,
                        };
                        if ui
                            .button(format!("{role} (ID {})", acc.sc_user_id))
                            .clicked()
                        {
                            action = ArtistAction::Navigate(
                                Route::User,
                                Some(format!("soundcloud:users:{}", acc.sc_user_id)),
                            );
                        }
                    }
                }
                if !artist.socials.is_empty() {
                    widgets::section_header(ui, "Links", None);
                    for s in &artist.socials {
                        ui.horizontal(|ui| {
                            ui.hyperlink_to(social_label(&s.kind), &s.url);
                            ui.label(&s.source);
                        });
                    }
                }
            }
        }

        action
    }

    fn request_detail(&mut self, rt: &tokio::runtime::Handle, api: &ApiClient, enc: &str) {
        let api = api.clone();
        let url = format!("/artists/{enc}");
        self.detail.request(rt, async move {
            api.get_json(&url)
                .await
                .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn tracks_section(
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &PlayerState,
        query: &Query<Vec<Track>>,
        loading: bool,
        sort: &mut TracksSort,
        empty_text: &str,
        accent: egui::Color32,
        action: &mut ArtistAction,
    ) {
        ui.horizontal(|ui| {
            ui.label("Sort:");
            ui.selectable_value(sort, TracksSort::Popular, "Popular");
            ui.selectable_value(sort, TracksSort::Recent, "Recent");
            if loading {
                ui.spinner();
            }
            if let Some(tracks) = query.data.as_ref() {
                let total: i64 = tracks.iter().map(|t| t.duration).sum();
                ui.label(format!("{} · {}", tracks.len(), fmt_dur_ms(total)));
            }
        });
        if loading && query.data.is_none() {
            widgets::loading(ui);
        } else if let Some(err) = query.error.clone() {
            ui.colored_label(
                egui::Color32::from_rgb(255, 150, 150),
                format!("Tracks unavailable: {err}"),
            );
        } else if let Some(tracks) = query.data.as_ref() {
            if tracks.is_empty() {
                ui.label(empty_text);
            }
            for (i, t) in tracks.iter().enumerate() {
                let hit = Self::track_row(ui, rt, images, player, t, accent);
                if hit.play_clicked() {
                    *action = ArtistAction::PlayList(tracks.clone(), i);
                } else if hit.title_clicked() {
                    *action = ArtistAction::Navigate(Route::Track, Some(t.urn.clone()));
                } else if hit.artist_clicked() {
                    if let Some(u) = t.user.as_ref() {
                        *action = ArtistAction::Navigate(Route::User, Some(u.urn.clone()));
                    }
                } else if hit.menu_clicked() {
                    *action = ArtistAction::OpenMenu(t.clone());
                }
            }
        }
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
}
