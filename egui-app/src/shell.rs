//! Phase 2 シェル: Sidebar + 中央プレースホルダ + NowPlayingBar。
//! 対応: `desktop/src/components/layout/{AppShell,Sidebar,NowPlayingBar}.tsx`。
//! トランスポート (再生/停止/シーク/音量) とファイル読込を backend に直結する。

use super::state::{AppState, LoadState, RepeatMode, Route};
use crate::backend::audio::engine;
use crate::views::{
    album::AlbumAction, artist::ArtistAction, collection::CollectionAction, home::HomeAction,
    library::LibraryAction, login::LoginAction, offline::OfflineAction, playlist::PlaylistAction,
    search::SearchAction, settings::SettingsAction, tag::TagAction, track::TrackAction,
    user::UserAction,
};

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn show_shell(state: &mut AppState, ui: &mut egui::Ui) {
    crate::theme::ensure_applied(ui.ctx(), &state.settings, &mut state.theme_applied);
    state.drain_events();
    state.drain_backend();
    state.poll_load();

    let has_audio = state.audio().is_some();
    let playing = state.player.is_playing;
    let loading = matches!(state.load, LoadState::Loading { .. });
    if playing || loading {
        ui.ctx().request_repaint();
    }

    egui::Panel::left("sidebar").resizable(false).show(ui, |ui| {
        ui.heading("SCD-Direct");
        ui.separator();
        for route in Route::ALL {
            ui.selectable_value(&mut state.route, *route, route.title());
        }
        ui.separator();
        if let Some(backend) = &state.backend {
            let s = &backend.servers;
            ui.label(format!(
                "api :{}  static :{}  proxy :{}",
                s.api_port, s.static_port, s.proxy_port
            ));
            if backend.audio.is_none() {
                ui.label("audio: unavailable");
            }
        }
        if let Some(e) = &state.boot_error {
            ui.colored_label(egui::Color32::RED, format!("boot: {e}"));
        }
        if let Some(e) = &state.backend.as_ref().and_then(|b| b.audio_error.clone()) {
            ui.colored_label(egui::Color32::RED, format!("audio: {e}"));
        }
        if let Some(e) = &state.last_sync_error {
            ui.colored_label(egui::Color32::YELLOW, format!("sync: {e}"));
        }
    });

    if state.queue_open {
        egui::Panel::right("queue").show(ui, |ui| {
            ui.heading("Queue");
            let mut jump: Option<usize> = None;
            let mut remove: Option<usize> = None;
            let current = state.player.queue_index;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, t) in state.player.queue.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let label = format!("{} — {}", t.display_title(), t.artist_name());
                        if ui.selectable_label(Some(i) == current, label).clicked() {
                            jump = Some(i);
                        }
                        if ui.small_button("x").clicked() {
                            remove = Some(i);
                        }
                    });
                }
            });
            if let Some(i) = remove {
                if i < state.player.queue.len() {
                    state.player.queue.remove(i);
                }
                match state.player.queue_index {
                    Some(_) if state.player.queue.is_empty() => {
                        state.player.queue_index = None;
                    }
                    Some(c) if c == i => {
                        state.player.queue_index =
                            Some(i.min(state.player.queue.len().saturating_sub(1)));
                    }
                    Some(c) if c > i => {
                        state.player.queue_index = Some(c - 1);
                    }
                    _ => {}
                }
            }
            if let Some(i) = jump {
                state.play_queue_index(i);
            }
            ui.horizontal(|ui| {
                if ui.button("Clear").clicked() {
                    state.player.clear_queue();
                }
                ui.label(format!("{} tracks", state.player.queue.len()));
            });
        });
    }

    egui::Panel::bottom("now_playing").show(ui, |ui| {
        ui.horizontal(|ui| {
            let audio = state.audio().cloned();
            let label = if playing { "Pause" } else { "Play" };
            if ui
                .add_enabled(has_audio, egui::Button::new(label))
                .clicked()
            {
                if let Some(audio) = &audio {
                    if playing {
                        engine::pause(audio);
                        state.player.is_playing = false;
                    } else {
                        engine::play(audio);
                        state.player.is_playing = true;
                    }
                }
            }
            if ui
                .add_enabled(has_audio, egui::Button::new("Stop"))
                .clicked()
            {
                if let Some(audio) = &audio {
                    engine::stop(audio);
                    state.player.is_playing = false;
                }
            }
            let pos0 = audio.as_ref().map(|a| engine::get_position(a)).unwrap_or(0.0);
            if ui
                .add_enabled(has_audio, egui::Button::new("Prev"))
                .clicked()
            {
                state.prev_track(pos0);
            }
            if ui
                .add_enabled(has_audio, egui::Button::new("Next"))
                .clicked()
            {
                state.next_track();
            }
            if ui
                .selectable_label(state.player.shuffle, "Shuffle")
                .clicked()
            {
                state.player.toggle_shuffle();
            }
            let repeat_label = match state.player.repeat {
                RepeatMode::Off => "Repeat: Off",
                RepeatMode::All => "Repeat: All",
                RepeatMode::One => "Repeat: One",
            };
            if ui
                .selectable_label(state.player.repeat != RepeatMode::Off, repeat_label)
                .clicked()
            {
                state.player.cycle_repeat();
            }
            let queue_label = format!("Queue ({})", state.player.queue.len());
            if ui.button(queue_label).clicked() {
                state.queue_open = !state.queue_open;
            }

            let (pos, dur) = match &audio {
                Some(a) => (engine::get_position(a), state.player.duration_secs),
                None => (0.0, None),
            };
            match dur {
                Some(d) if d > 0.0 => {
                    let mut p = pos.min(d);
                    if ui
                        .add(egui::Slider::new(&mut p, 0.0..=d).text("Position"))
                        .changed()
                    {
                        if let Some(a) = &audio {
                            let _ = engine::seek(p, a);
                        }
                    }
                    ui.label(format!("{} / {}", fmt_time(p), fmt_time(d)));
                }
                _ => {
                    ui.label(fmt_time(pos));
                }
            }

            let mut vol = state.player.volume;
            if ui
                .add(egui::Slider::new(&mut vol, 0.0..=100.0).text("Volume"))
                .changed()
            {
                state.player.volume = vol;
                if let Some(a) = &audio {
                    engine::set_volume(vol as f64, a);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("File:");
            ui.text_edit_singleline(&mut state.file_path_input);
            if ui
                .add_enabled(has_audio && !loading, egui::Button::new("Load"))
                .clicked()
            {
                state.start_file_load();
            }
            if loading {
                ui.spinner();
            }
            if let Some(e) = state.load_error.clone() {
                ui.colored_label(egui::Color32::RED, e);
            }
        });
    });

    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let rt = state.runtime().handle().clone();
            let audio = state.audio().cloned();
            let audio_ref = audio.as_ref();
            state.sync_api_session();
            let api = state.api.clone();
            let api_ref = api.as_ref();
            let param = state.nav_param.clone();
            let param_ref = param.as_deref();
            let accent = crate::widgets::accent_color(&state.settings);
            // `backend` と各ビューは disjoint field のため同時借用できる。
            let cache = state.backend.as_ref().map(|b| &b.track_cache);
            let images = &mut state.images;
            let player = &mut state.player;

            match state.route {
                Route::Home => match state.home.show(
                    api_ref,
                    &rt,
                    images,
                    player,
                    audio_ref,
                    &state.settings,
                    ui,
                ) {
                    HomeAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    HomeAction::None => {}
                },
                Route::Search => match state.search.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    SearchAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    SearchAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    SearchAction::None => {}
                },
                Route::Tag => match state.tag.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    TagAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    TagAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    TagAction::None => {}
                },
                Route::Library => match state.library.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    LibraryAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    LibraryAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    LibraryAction::None => {}
                },
                Route::LibraryCollection => match state.collection.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    CollectionAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    CollectionAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    CollectionAction::None => {}
                },
                Route::Track => match state.track.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    TrackAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    TrackAction::Seek(frac) => {
                        if let (Some(audio), Some(dur)) =
                            (audio.as_ref(), state.player.duration_secs)
                        {
                            if dur > 0.0 {
                                let _ = engine::seek(
                                    (f64::from(frac) * dur).clamp(0.0, dur),
                                    audio,
                                );
                            }
                        }
                    }
                    TrackAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    TrackAction::None => {}
                },
                Route::Playlist => match state.playlist.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    PlaylistAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    PlaylistAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    PlaylistAction::None => {}
                },
                Route::Album => match state.album.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    AlbumAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    AlbumAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    AlbumAction::None => {}
                },
                Route::User => match state.user.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    UserAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    UserAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    UserAction::None => {}
                },
                Route::Artist => match state.artist.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    ArtistAction::PlayTrack(track) => {
                        if let Some(api) = &api {
                            let url = api.stream_url(&track.urn, false);
                            state.play_stream(&track, url);
                        }
                    }
                    ArtistAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    ArtistAction::None => {}
                },
                Route::Settings => {
                    let settings = &mut state.settings;
                    let view = &mut state.settings_view;
                    match view.show(
                        api_ref, &rt, images, player, audio_ref, param_ref, cache, settings, ui,
                    ) {
                        SettingsAction::None => {}
                    }
                }
                Route::Offline => match state.offline.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    OfflineAction::PlayFile(path) => {
                        state.play_file(path);
                    }
                    OfflineAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    OfflineAction::None => {}
                },
                Route::Login => match state.login.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, ui,
                ) {
                    LoginAction::OpenLogin => {
                        if let Some(b) = &state.backend {
                            crate::backend::weblogin::open_login_window(
                                &rt,
                                b.direct.clone(),
                                b.session.clone(),
                                b.bus.clone(),
                            );
                        }
                    }
                    LoginAction::SetToken(token) => {
                        if let Some(b) = &state.backend {
                            let session = b.session.clone();
                            let bus = b.bus.clone();
                            rt.spawn(async move {
                                let _ = session.set_token(&bus, token).await;
                            });
                        }
                        state.reset_views();
                    }
                    LoginAction::Logout => {
                        if let (Some(b), Some(api)) = (&state.backend, &api) {
                            let session = b.session.clone();
                            let bus = b.bus.clone();
                            let base = api.base().to_string();
                            rt.spawn(async move {
                                let _ =
                                    crate::backend::auth::auth_logout(base, &bus, &session).await;
                            });
                        }
                        state.reset_views();
                    }
                    LoginAction::None => {}
                },
            }
        });
    });
}
