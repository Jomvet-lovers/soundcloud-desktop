//! Phase 2 シェル: Sidebar + 中央プレースホルダ + NowPlayingBar。
//! 対応: `desktop/src/components/layout/{AppShell,Sidebar,NowPlayingBar}.tsx`。
//! トランスポート (再生/停止/シーク/音量) とファイル読込を backend に直結する。

use super::state::{AppState, LoadState, RepeatMode, Route};
use crate::backend::audio::engine;
use crate::backend::track_cache::ExportFormat;
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
    handle_shortcuts(state, ui.ctx());
    state.drain_events();
    state.drain_backend();
    state.poll_load();
    // NowPlaying バーの like 状態を回収 (最新のトラックで上書き)。
    if state.now_like.poll() {
        if let Some(st) = state.now_like.data {
            let fav = state.now_liked.unwrap_or(false);
            state.now_liked = Some(st.liked || fav);
        }
    }

    let has_audio = state.audio().is_some();
    let playing = state.player.is_playing;
    let loading = matches!(state.load, LoadState::Loading { .. });
    if playing || loading {
        ui.ctx().request_repaint();
    }

    let mut account_action: Option<LoginAction> = None;
    if state.sidebar_open {
        egui::Panel::left("sidebar").resizable(false).show(ui, |ui| {
        ui.heading("SCD-Direct");
        let signed_in = state.api.as_ref().and_then(|a| a.session_token()).is_some();
        if signed_in {
            if ui.button("Sign out").clicked() {
                account_action = Some(LoginAction::Logout);
            }
        } else if ui.button("Sign in").clicked() {
            account_action = Some(LoginAction::OpenLogin);
        }
        ui.separator();
        for route in Route::ALL {
            ui.selectable_value(&mut state.route, *route, route.title());
        }
        if !state.settings.pinned_playlists.is_empty() {
            ui.separator();
            ui.label("Quick access");
            for pin in &state.settings.pinned_playlists {
                if ui.selectable_label(false, &pin.title).clicked() {
                    state.route = Route::Playlist;
                    state.nav_param = Some(pin.urn.clone());
                }
            }
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
    }
    if let Some(action) = account_action {
        apply_login_action(state, action);
    }

    if state.queue_open {
        egui::Panel::right("queue").show(ui, |ui| {
            ui.heading("Queue");
            let mut jump: Option<usize> = None;
            let mut remove: Option<usize> = None;
            let mut reorder: Option<(usize, usize)> = None;
            let current = state.player.queue_index;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, t) in state.player.queue.iter().enumerate() {
                    let label = format!("{} — {}", t.display_title(), t.artist_name());
                    let row_id = egui::Id::new(("queue-row", i));
                    let (_inner, dropped) =
                        ui.dnd_drop_zone::<usize, _>(egui::Frame::NONE, |ui| {
                            ui.horizontal(|ui| {
                                let src = ui.dnd_drag_source(row_id, i, |ui| {
                                    ui.selectable_label(Some(i) == current, label);
                                });
                                if src.response.clicked() {
                                    jump = Some(i);
                                }
                                if ui.small_button("x").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    if let Some(from) = dropped {
                        reorder = Some((*from, i));
                    }
                }
            });
            if let Some((from, to)) = reorder {
                state.player.move_queue_item(from, to);
            }
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
        let accent = crate::widgets::accent_color(&state.settings);
        // 再生中トラック (Tauri 版 NowPlayingBar: タイトル/アーティストは各ページへのリンク)。
        ui.horizontal(|ui| {
            let current = state
                .player
                .queue_index
                .and_then(|i| state.player.queue.get(i))
                .cloned();
            match current {
                Some(track) => {
                    let is_sc = track.urn.starts_with("soundcloud:");
                    if is_sc {
                        if ui.link(track.display_title()).clicked() {
                            state.route = Route::Track;
                            state.nav_param = Some(track.urn.clone());
                        }
                    } else {
                        ui.label(track.display_title());
                    }
                    if let Some(user) = track.user.as_ref() {
                        ui.label("—");
                        if is_sc {
                            if ui.link(&user.username).clicked() {
                                state.route = Route::User;
                                state.nav_param = Some(user.urn.clone());
                            }
                        } else {
                            ui.label(&user.username);
                        }
                    }
                    if is_sc {
                        let liked = state.now_liked.unwrap_or(false);
                        if crate::widgets::like_button(ui, liked, accent) {
                            let next = !liked;
                            state.now_liked = Some(next);
                            if let Some(api) = state.api.clone() {
                                let rt = state.runtime().handle().clone();
                                let path = format!(
                                    "/likes/tracks/{}",
                                    urlencoding::encode(&track.urn)
                                );
                                rt.spawn(async move {
                                    let method = if next { "POST" } else { "DELETE" };
                                    let _ = api.request_json(method, &path, None).await;
                                });
                            }
                        }
                    }
                }
                None => {
                    ui.label("Nothing playing");
                }
            }
        });
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
            let ab_label = match (state.ab_a, state.ab_b) {
                (Some(_), Some(_)) => "A-B: on",
                (Some(_), None) => "A-B: A",
                _ => "A-B",
            };
            if ui
                .selectable_label(state.ab_b.is_some(), ab_label)
                .clicked()
            {
                cycle_ab_loop(state);
            }
            match (state.ab_a, state.ab_b) {
                (Some(a), Some(b)) => {
                    ui.label(format!("{}–{}", fmt_time(a), fmt_time(b)));
                }
                (Some(a), None) => {
                    ui.label(format!("A {}", fmt_time(a)));
                }
                _ => {}
            }
            let queue_label = format!("Queue ({})", state.player.queue.len());
            if ui.button(queue_label).clicked() {
                state.queue_open = !state.queue_open;
            }
            if ui.selectable_label(state.show_eq, "EQ").clicked() {
                state.show_eq = !state.show_eq;
            }
            if ui.selectable_label(state.show_tuning, "Tune").clicked() {
                state.show_tuning = !state.show_tuning;
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
            let resp = ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).text("Volume"));
            if resp.changed() {
                if vol > 0.0 {
                    state.player.volume_before_mute = vol;
                }
                state.set_volume(vol);
            }
            let muted = state.player.volume <= 0.0;
            if ui
                .button(if muted { "Unmute" } else { "Mute" })
                .clicked()
            {
                state.toggle_mute();
            }
            // ドラッグ終了 (またはクリック等の単発変更) で永続化する。
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                if let Err(e) = crate::backend::prefs::save(&state.settings) {
                    eprintln!("[prefs] save failed: {e}");
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
                        state.play_list(vec![track], 0);
                    }
                    HomeAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    HomeAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    HomeAction::None => {}
                },
                Route::Search => match state.search.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    SearchAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    SearchAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    SearchAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                        state.play_list(vec![track], 0);
                    }
                    TagAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    TagAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                        state.play_list(vec![track], 0);
                    }
                    LibraryAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    LibraryAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    LibraryAction::ShuffleLikes(tracks) => {
                        state.player.shuffle = true;
                        state.play_list(tracks, 0);
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
                        state.play_list(vec![track], 0);
                    }
                    CollectionAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    CollectionAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                        state.play_list(vec![track], 0);
                    }
                    TrackAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    TrackAction::AddNextUp(track) => {
                        state.player.insert_next(vec![track]);
                    }
                    TrackAction::AddToPlaylist(track) => {
                        state.open_add_to_playlist(track);
                    }
                    TrackAction::OpenDownload(track) => {
                        state.open_download(track);
                    }
                    TrackAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                    api_ref,
                    &rt,
                    images,
                    player,
                    audio_ref,
                    param_ref,
                    cache,
                    accent,
                    &state.settings,
                    ui,
                ) {
                    PlaylistAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    PlaylistAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    PlaylistAction::ShufflePlay(tracks) => {
                        state.player.shuffle = true;
                        state.play_list(tracks, 0);
                    }
                    PlaylistAction::TogglePin(urn, title) => {
                        state.toggle_pin_playlist(urn, title);
                    }
                    PlaylistAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                        state.play_list(vec![track], 0);
                    }
                    AlbumAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    AlbumAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                        state.play_list(vec![track], 0);
                    }
                    UserAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    UserAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                        state.play_list(vec![track], 0);
                    }
                    ArtistAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    ArtistAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
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
                Route::Login => {
                    let action = state.login.show(
                        api_ref, &rt, images, player, audio_ref, param_ref, cache, ui,
                    );
                    apply_login_action(state, action);
                }
            }
        });
    });

    if state.show_shortcuts {
        let mut open = true;
        egui::Window::new("Keyboard Shortcuts")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                egui::Grid::new("shortcuts")
                    .num_columns(2)
                    .spacing([28.0, 6.0])
                    .show(ui, |ui| {
                        for (key, label) in SHORTCUTS {
                            ui.strong(*key);
                            ui.label(*label);
                            ui.end_row();
                        }
                    });
            });
        if !open {
            state.show_shortcuts = false;
        }
    }
    if state.show_eq {
        show_eq_window(state, ui.ctx());
    }
    if state.show_tuning {
        show_tuning_window(state, ui.ctx());
    }
    if state.add_to_playlist.is_some() {
        show_add_to_playlist(state, ui.ctx());
    }
    if state.track_menu.is_some() {
        show_track_menu(state, ui.ctx());
    }
    if state.download_track.is_some() {
        show_download_window(state, ui.ctx());
    }
}

/// ログイン系アクションの共通処理 (Login 画面とサイドバーの両方から使う)。
fn apply_login_action(state: &mut AppState, action: LoginAction) {
    let rt = state.runtime().handle().clone();
    match action {
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
            if let (Some(b), Some(api)) = (&state.backend, &state.api) {
                let session = b.session.clone();
                let bus = b.bus.clone();
                let base = api.base().to_string();
                rt.spawn(async move {
                    let _ = crate::backend::auth::auth_logout(base, &bus, &session).await;
                });
            }
            state.reset_views();
            // ログアウト後はログイン画面に戻す。
            state.route = Route::Login;
        }
        LoginAction::None => {}
    }
}

pub(crate) const FOCUS_SEARCH_ID: &str = "scd_focus_search";

const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play / Pause"),
    ("← / →", "Seek back / forward 5s"),
    ("N / P", "Next / Previous track"),
    ("S / R", "Toggle shuffle / repeat"),
    ("B", "Cycle A-B loop point"),
    ("↑ / ↓", "Volume up / down"),
    ("M", "Mute / Unmute"),
    ("/ or Ctrl+K", "Search"),
    ("Q", "Toggle queue"),
    ("[", "Toggle sidebar"),
    ("F11", "Toggle fullscreen"),
    ("Esc", "Close panel"),
    ("Ctrl+/", "Show shortcuts"),
];

#[derive(Default)]
struct ShortcutHits {
    space: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    next: bool,
    prev: bool,
    shuffle: bool,
    repeat: bool,
    ab: bool,
    mute: bool,
    queue: bool,
    sidebar: bool,
    fullscreen: bool,
    escape: bool,
    search: bool,
    search_ctrl: bool,
    shorts: bool,
}

/// キーボードショートカット (Tauri 版 AppShell 相当)。
fn handle_shortcuts(state: &mut AppState, ctx: &egui::Context) {
    use egui::{Key, Modifiers};

    let typing = ctx.egui_wants_keyboard_input();
    let mut hits = ShortcutHits::default();
    ctx.input_mut(|i| {
        hits.shorts = i.consume_key(Modifiers::COMMAND, Key::Slash);
        hits.search_ctrl = i.consume_key(Modifiers::COMMAND, Key::K);
        if typing {
            return;
        }
        hits.space = i.consume_key(Modifiers::NONE, Key::Space);
        hits.left = i.consume_key(Modifiers::NONE, Key::ArrowLeft);
        hits.right = i.consume_key(Modifiers::NONE, Key::ArrowRight);
        hits.up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
        hits.down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
        hits.next = i.consume_key(Modifiers::NONE, Key::N);
        hits.prev = i.consume_key(Modifiers::NONE, Key::P);
        hits.shuffle = i.consume_key(Modifiers::NONE, Key::S);
        hits.repeat = i.consume_key(Modifiers::NONE, Key::R);
        hits.ab = i.consume_key(Modifiers::NONE, Key::B);
        hits.mute = i.consume_key(Modifiers::NONE, Key::M);
        hits.queue = i.consume_key(Modifiers::NONE, Key::Q);
        hits.sidebar = i.consume_key(Modifiers::NONE, Key::OpenBracket);
        hits.fullscreen = i.consume_key(Modifiers::NONE, Key::F11);
        hits.escape = i.consume_key(Modifiers::NONE, Key::Escape);
        hits.search = i.consume_key(Modifiers::NONE, Key::Slash);
    });

    if hits.shorts {
        state.show_shortcuts = !state.show_shortcuts;
    }
    if hits.search || hits.search_ctrl {
        state.route = Route::Search;
        ctx.memory_mut(|m| {
            m.data.insert_temp(egui::Id::new(FOCUS_SEARCH_ID), true);
        });
    }
    if hits.space {
        if let Some(audio) = state.audio().cloned() {
            if state.player.is_playing {
                engine::pause(&audio);
                state.player.is_playing = false;
            } else {
                engine::play(&audio);
                state.player.is_playing = true;
            }
        }
    }
    if hits.left || hits.right {
        if let (Some(audio), Some(dur)) = (state.audio().cloned(), state.player.duration_secs) {
            let pos = engine::get_position(&audio);
            let delta = if hits.right { 5.0 } else { -5.0 };
            let _ = engine::seek((pos + delta).clamp(0.0, dur.max(0.0)), &audio);
        }
    }
    if hits.next {
        state.next_track();
    }
    if hits.prev {
        if let Some(audio) = state.audio().cloned() {
            let pos = engine::get_position(&audio);
            state.prev_track(pos);
        }
    }
    if hits.shuffle {
        state.player.toggle_shuffle();
    }
    if hits.repeat {
        state.player.cycle_repeat();
    }
    if hits.ab {
        cycle_ab_loop(state);
    }
    if hits.up || hits.down {
        let delta = if hits.up { 5.0 } else { -5.0 };
        let vol = (state.player.volume + delta).clamp(0.0, 100.0);
        if vol > 0.0 {
            state.player.volume_before_mute = vol;
        }
        state.set_volume(vol);
        if let Err(e) = crate::backend::prefs::save(&state.settings) {
            eprintln!("[prefs] save failed: {e}");
        }
    }
    if hits.mute {
        state.toggle_mute();
    }
    if hits.queue {
        state.queue_open = !state.queue_open;
    }
    if hits.sidebar {
        state.sidebar_open = !state.sidebar_open;
    }
    if hits.fullscreen {
        state.fullscreen = !state.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(state.fullscreen));
    }
    if hits.escape {
        if state.track_menu.is_some() {
            state.track_menu = None;
        } else if state.queue_open {
            state.queue_open = false;
        } else if state.show_shortcuts {
            state.show_shortcuts = false;
        }
    }
}

/// A-B ループの循環操作: A 設定 → B 設定 → 解除 (Tauri 版 B キー相当)。
fn cycle_ab_loop(state: &mut AppState) {
    let Some(audio) = state.audio().cloned() else {
        return;
    };
    let pos = engine::get_position(&audio);
    if state.ab_a.is_none() {
        state.ab_a = Some(pos);
    } else if state.ab_b.is_none() {
        let a = state.ab_a.unwrap_or(0.0);
        if pos > a + 0.5 {
            state.ab_b = Some(pos);
        } else {
            // B は A より後ろのみ有効。短すぎる場合はやり直し。
            state.ab_a = None;
            state.ab_b = None;
        }
    } else {
        state.ab_a = None;
        state.ab_b = None;
    }
    crate::backend::audio::engine::set_ab_loop(state.ab_a, state.ab_b, &audio);
}

const EQ_LABELS: [&str; 10] = ["32", "64", "125", "250", "500", "1K", "2K", "4K", "8K", "16K"];

/// Tauri 版 `equalizer.ts` のプリセット。
const EQ_PRESETS: &[(&str, [f64; 10])] = &[
    ("Flat", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Bass Boost", [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    (
        "Bass Destroyer",
        [12.0, 12.0, 10.0, 7.0, 3.0, 0.0, -2.0, -4.0, -4.0, -5.0],
    ),
    ("Treble Boost", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0]),
    ("Vocal", [-2.0, -1.0, 0.0, 2.0, 4.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
    ("Rock", [4.0, 3.0, 1.0, 0.0, -1.0, 0.0, 2.0, 3.0, 4.0, 4.0]),
    ("Electronic", [5.0, 4.0, 2.0, 0.0, -1.0, 0.0, 1.0, 3.0, 4.0, 5.0]),
    ("Classical", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -3.0, -3.0, -4.0]),
    ("Loudness", [5.0, 4.0, 1.0, 0.0, -1.0, 0.0, -1.0, 0.0, 3.0, 4.0]),
    ("V-Shape", [5.0, 3.0, 1.0, -1.0, -3.0, -3.0, -1.0, 1.0, 3.0, 5.0]),
    ("Night", [-3.0, -2.0, 0.0, 2.0, 3.0, 3.0, 2.0, 0.0, -2.0, -4.0]),
];

/// イコライザー窓 (Tauri 版 `EqualizerPanel` 相当)。
fn show_eq_window(state: &mut AppState, ctx: &egui::Context) {
    let mut open = true;
    let mut changed = false;
    egui::Window::new("Equalizer")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            changed |= ui
                .checkbox(&mut state.settings.eq_enabled, "Enable")
                .changed();
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Preset")
                    .selected_text("Custom")
                    .show_ui(ui, |ui| {
                        for (name, gains) in EQ_PRESETS {
                            if ui.selectable_label(false, *name).clicked() {
                                state.settings.eq_gains = gains.to_vec();
                                changed = true;
                            }
                        }
                    });
                if ui.button("Flat").clicked() {
                    state.settings.eq_gains = vec![0.0; 10];
                    changed = true;
                }
            });
            let mut gains = state.settings.eq_gains_or_default();
            let mut sliders_changed = false;
            ui.horizontal(|ui| {
                for (i, g) in gains.iter_mut().enumerate() {
                    ui.vertical(|ui| {
                        sliders_changed |= ui
                            .add(egui::Slider::new(g, -12.0..=12.0).vertical())
                            .changed();
                        ui.small(EQ_LABELS[i]);
                    });
                }
            });
            if sliders_changed {
                state.settings.eq_gains = gains;
                changed = true;
            }
        });
    state.show_eq = open;
    if changed {
        state.apply_eq();
    }
}

/// サウンドチューニング窓 (Tauri 版 `SoundTuningPopover` 相当)。
fn show_tuning_window(state: &mut AppState, ctx: &egui::Context) {
    let mut open = true;
    let mut changed = false;
    egui::Window::new("Sound tuning")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Speed");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut state.settings.playback_rate, 0.5..=2.0)
                            .fixed_decimals(2)
                            .suffix("x"),
                    )
                    .changed();
            });
            let mut manual = !state.settings.pitch_auto;
            if ui.checkbox(&mut manual, "Manual pitch control").changed() {
                state.settings.pitch_auto = !manual;
                changed = true;
            }
            ui.add_enabled_ui(manual, |ui| {
                changed |= ui
                    .add(
                        egui::Slider::new(&mut state.settings.pitch_semitones, -12.0..=12.0)
                            .step_by(0.5)
                            .suffix(" st"),
                    )
                    .changed();
            });
            if ui.button("Reset").clicked() {
                state.settings.playback_rate = 1.0;
                state.settings.pitch_semitones = 0.0;
                state.settings.pitch_auto = true;
                changed = true;
            }
            ui.label(format!(
                "Effective rate: {:.3}x",
                state.settings.effective_rate()
            ));
        });
    state.show_tuning = open;
    if changed {
        state.apply_rate();
    }
}

/// 「プレイリストに追加」ダイアログ (Tauri 版 `AddToPlaylistDialog` 相当)。
fn show_add_to_playlist(state: &mut AppState, ctx: &egui::Context) {
    let Some(track) = state.add_to_playlist.clone() else {
        return;
    };
    let api = state.api.clone();
    if !state.dialog_playlists.requested() {
        if let Some(api) = api.clone() {
            let rt = state.runtime().handle().clone();
            state.dialog_playlists.request(&rt, async move {
                let v = api.get_json("/me/playlists?limit=50&page=0").await?;
                serde_json::from_value(v).map_err(|e| e.to_string())
            });
        }
    }
    state.dialog_playlists.poll();

    let mut open = true;
    let mut add_to: Option<String> = None;
    let mut create = false;
    egui::Window::new("Add to playlist")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!("Track: {}", track.display_title()));
            ui.separator();
            if state.dialog_playlists.loading && state.dialog_playlists.data.is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading playlists...");
                });
            }
            if let Some(err) = state.dialog_playlists.error.clone() {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 150, 150),
                    format!("Playlists unavailable: {err}"),
                );
            }
            if let Some(page) = state.dialog_playlists.data.as_ref() {
                if page.collection.is_empty() {
                    ui.label("No playlists yet");
                }
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for p in &page.collection {
                            ui.horizontal(|ui| {
                                ui.label(&p.title);
                                if ui.small_button("Add").clicked() {
                                    add_to = Some(p.urn.clone());
                                }
                            });
                        }
                    });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("New playlist:");
                ui.text_edit_singleline(&mut state.new_playlist_title);
            });
            let can_create = !state.new_playlist_title.trim().is_empty();
            if ui
                .add_enabled(can_create, egui::Button::new("Create & add"))
                .clicked()
            {
                create = true;
            }
        });

    if let Some(urn) = add_to {
        if let Some(api) = api {
            let rt = state.runtime().handle().clone();
            let path = format!("/playlists/{}/tracks", urlencoding::encode(&urn));
            let body = serde_json::json!({ "add": track.urn });
            rt.spawn(async move {
                let _ = api.request_json("POST", &path, Some(&body)).await;
            });
        }
        state.add_to_playlist = None;
        return;
    }
    if create {
        let title = state.new_playlist_title.trim().to_string();
        if let Some(api) = api {
            let rt = state.runtime().handle().clone();
            let body = serde_json::json!({
                "playlist": {
                    "title": title,
                    "sharing": "private",
                    "tracks": [ { "urn": track.urn } ],
                }
            });
            rt.spawn(async move {
                let _ = api.request_json("POST", "/playlists", Some(&body)).await;
            });
        }
        state.add_to_playlist = None;
        return;
    }
    if !open {
        state.add_to_playlist = None;
    }
}

/// 右クリック位置にトラックメニューを開く。
fn open_menu(state: &mut AppState, ui: &egui::Ui, track: crate::backend::models::Track) {
    let pos = ui
        .ctx()
        .pointer_interact_pos()
        .unwrap_or_else(|| egui::pos2(240.0, 240.0));
    state.open_track_menu(track, pos);
}

/// トラックの右クリックメニュー (Tauri 版 `TrackContextMenu` 相当)。
fn show_track_menu(state: &mut AppState, ctx: &egui::Context) {
    let Some(menu) = state.track_menu.clone() else {
        return;
    };
    state.menu_like.poll();
    state.menu_dislike.poll();

    let track = menu.track.clone();
    let liked = state.menu_like.data.map(|f| f.liked).unwrap_or(false);
    let disliked = state.menu_dislike.data.map(|f| f.disliked).unwrap_or(false);

    enum Act {
        SetLike(bool),
        SetDislike(bool),
        PlayNext,
        AddToPlaylist,
        Download,
        CopyLink,
        GoTrack,
        GoArtist,
    }
    let mut act: Option<Act> = None;

    let area = egui::Area::new(egui::Id::new("scd-track-menu"))
        .fixed_pos(menu.pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(210.0);
                let like_label = if liked {
                    "Remove from library"
                } else {
                    "Add to library"
                };
                if ui.button(like_label).clicked() {
                    act = Some(Act::SetLike(!liked));
                }
                let dis_label = if disliked {
                    "Remove dislike"
                } else {
                    "Not interested"
                };
                if ui.button(dis_label).clicked() {
                    act = Some(Act::SetDislike(!disliked));
                }
                if ui.button("Play next").clicked() {
                    act = Some(Act::PlayNext);
                }
                if ui.button("Add to playlist").clicked() {
                    act = Some(Act::AddToPlaylist);
                }
                if ui.button("Download...").clicked() {
                    act = Some(Act::Download);
                }
                ui.separator();
                if ui.button("Copy link").clicked() {
                    act = Some(Act::CopyLink);
                }
                if ui.button("Go to track").clicked() {
                    act = Some(Act::GoTrack);
                }
                if track.user.is_some() && ui.button("Go to artist").clicked() {
                    act = Some(Act::GoArtist);
                }
            });
        })
        .response;

    // メニュー外クリックで閉じる (Esc は handle_shortcuts 側)。
    if ctx.input(|i| i.pointer.any_click()) {
        let inside = ctx
            .input(|i| i.pointer.interact_pos())
            .map(|p| area.rect.contains(p))
            .unwrap_or(false);
        if !inside {
            state.track_menu = None;
            return;
        }
    }

    let Some(act) = act else {
        return;
    };
    match act {
        Act::SetLike(next) => {
            state.toggle_track_like(&track.urn, next);
            if state.player.current_queued().map(|t| t.urn.as_str())
                == Some(track.urn.as_str())
            {
                state.now_liked = Some(next);
            }
        }
        Act::SetDislike(next) => {
            state.toggle_track_dislike(&track.urn, next);
        }
        Act::PlayNext => {
            state.player.insert_next(vec![track.clone()]);
        }
        Act::AddToPlaylist => {
            state.open_add_to_playlist(track.clone());
        }
        Act::Download => {
            state.open_download(track.clone());
        }
        Act::CopyLink => {
            if let Some(url) = track.permalink_url.clone() {
                ctx.copy_text(url);
            } else if let Some(api) = state.api.clone() {
                let rt = state.runtime().handle().clone();
                let ctx2 = ctx.clone();
                let path = format!("/tracks/{}", urlencoding::encode(&track.urn));
                rt.spawn(async move {
                    if let Ok(v) = api.get_json(&path).await {
                        if let Some(u) = v.get("permalink_url").and_then(|x| x.as_str()) {
                            ctx2.copy_text(u.to_string());
                        }
                    }
                });
            }
        }
        Act::GoTrack => {
            state.route = Route::Track;
            state.nav_param = Some(track.urn.clone());
        }
        Act::GoArtist => {
            if let Some(user) = track.user.as_ref() {
                state.route = Route::User;
                state.nav_param = Some(user.urn.clone());
            }
        }
    }
    state.track_menu = None;
}

/// 単曲ダウンロード (保存先はネイティブダイアログ、形式は 4 種)。
fn show_download_window(state: &mut AppState, ctx: &egui::Context) {
    let Some(track) = state.download_track.clone() else {
        return;
    };
    state.download_status.poll();

    let mut open = true;
    let mut choose = false;
    egui::Window::new("Download")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!("Track: {}", track.display_title()));
            ui.horizontal(|ui| {
                ui.label("Format:");
                for (fmt, label) in [
                    (ExportFormat::M4a, "M4A (AAC)"),
                    (ExportFormat::Mp3, "MP3 320"),
                    (ExportFormat::Flac, "FLAC"),
                    (ExportFormat::Wav, "WAV"),
                ] {
                    if ui
                        .selectable_label(state.download_format == fmt, label)
                        .clicked()
                    {
                        state.download_format = fmt;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Save as...").clicked() {
                    choose = true;
                }
                if state.download_status.loading {
                    ui.spinner();
                    ui.label("Exporting...");
                } else if let Some(status) = state.download_status.data.as_ref() {
                    ui.label(status.as_str());
                } else if let Some(err) = state.download_status.error.as_ref() {
                    ui.colored_label(egui::Color32::from_rgb(255, 150, 150), err.as_str());
                }
            });
        });

    if choose {
        let fmt = state.download_format;
        let ext = match fmt {
            ExportFormat::M4a => "m4a",
            ExportFormat::Mp3 => "mp3",
            ExportFormat::Flac => "flac",
            ExportFormat::Wav => "wav",
        };
        let safe = |s: &str| {
            s.chars()
                .map(|c| {
                    if r#"\/:*?"<>|"#.contains(c) {
                        '_'
                    } else {
                        c
                    }
                })
                .collect::<String>()
        };
        let default_name = format!(
            "{} - {}.{ext}",
            safe(track.artist_name()),
            safe(track.display_title())
        );
        let picked = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(ext.to_uppercase(), &[ext])
            .save_file();
        if let Some(path) = picked {
            let dest = path.to_string_lossy().into_owned();
            if let Some(cache) = state.backend.as_ref().map(|b| b.track_cache.clone()) {
                let session = state
                    .api
                    .as_ref()
                    .and_then(|a| a.session_token().map(str::to_string));
                let hq = state.settings.hq_streaming;
                let cover = track.artwork("t500x500");
                let urn = track.urn.clone();
                let expected = (track.duration > 0).then_some(track.duration as u64);
                let rt = state.runtime().handle().clone();
                state.download_status = crate::query::Query::default();
                state.download_status.request(&rt, async move {
                    let req = crate::backend::track_cache::CacheRequest {
                        urn: &urn,
                        urls: &[],
                        download_urls: &[],
                        storage_urls: &[],
                        session_id: session.as_deref(),
                        hq,
                        liked: false,
                        expected_duration_ms: expected,
                    };
                    cache
                        .export_track(req, dest, cover, fmt)
                        .await
                        .map(|p| format!("Saved: {p}"))
                });
            }
        }
    }
    if !open {
        state.download_track = None;
    }
}
